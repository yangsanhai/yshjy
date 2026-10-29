//! RAR 解压（含加密、含分卷）
//!
//! # 读取方式（L1 批改造）
//! `rars` 0.11 的 `ArchiveReader::read_path` / `read_reader` 是**按需读取**（解析头 + 解压时读载荷），
//! 不像 `read(&[u8])` 那样要求整包进内存。所以：
//! - **有文件路径** → 走 `read_path`，流式，**没有大小上限**（以前 256 MiB 的限制已删）
//! - **只有一条不可 seek 的流**（管道 / stdin）→ 只能回退到整读内存，仍受 [`STREAM_FALLBACK_MAX`] 约束，
//!   并如实告诉用户「请直接给文件路径」
//! - **分卷** → 每卷 `read_path` 得到 `Archive`，再用 `rars::extract_volumes_to` 拼成一条逻辑流
//!
//! # RAR 的时间戳
//!
//! 走流式 API 时 `ExtractedEntryMeta` 带 `file_time` / `mtime_refinement`，**修改时间已经还原**：
//! 回调里按 RAR 版本（RAR4 = DOS 时间、RAR5 = Unix 秒）换算成 `SystemTime` 记下来，
//! 整包解完之后统一 `set_modified`（解压途中改会被库后续写入冲掉）。
//!
//! 权限位**还没做**：Unix 权限得从 `attr` + `host_os` 推（Windows 的 attr 是另一套语义），
//! 属于「做了要能保证不错」，先留 TODO。
//!
//! # 许可
//! RAR 的压缩算法是专有的，`rars` 是 Apache-2.0 的**纯 Rust 读取实现** ——
//! 我们只做解压、不生成 RAR（测试用的样本除外），符合许可边界。

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, ItemResult, Result};
use crate::safety::{self, Limits};
use crate::select::Selection;
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

/// 只有「不可 seek 的流」时才整读内存，此上限仅对该回退路径生效
///
/// 正常文件路径走 `read_path`，**不受这个数限制**（网盘 1–4 GB 的 RAR 照解）。
pub const STREAM_FALLBACK_MAX: u64 = 256 * 1024 * 1024;

/// 兼容旧名字（有调用方 / 测试引用过）
#[deprecated(note = "改用 STREAM_FALLBACK_MAX：只约束「流式回退」路径，不约束文件路径")]
pub const MAX_SIZE: u64 = STREAM_FALLBACK_MAX;

/// 打开一个 RAR 条目时给库的 Writer 工厂
type 打开条目<'a> =
    Box<dyn FnMut(&rars::ExtractedEntryMeta) -> std::result::Result<Box<dyn std::io::Write>, rars::Error> + 'a>;

/// RAR 解压参数
#[derive(Debug, Clone)]
pub struct ExtractRarOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 密码
    pub password: Option<String>,
    /// 同名策略
    pub overwrite: crate::extract::Overwrite,
    /// 挑哪些条目（默认全要）
    pub selection: Selection,
    /// 安全限值
    pub limits: Limits,
}

impl ExtractRarOptions {
    /// 以输出目录构造默认参数
    pub fn new<P: Into<PathBuf>>(dest: P) -> Self {
        Self {
            dest: dest.into(),
            password: None,
            overwrite: crate::extract::Overwrite::default(),
            selection: Selection::all(),
            limits: Limits::default(),
        }
    }
}

/// 解压一个 RAR 文件
pub fn extract_rar_file(
    path: &Path,
    opts: &ExtractRarOptions,
) -> Result<crate::extract::ExtractReport> {
    extract_rar_file_ctx(path, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_rar_file`]，但带上事件 / 进度 / 取消上下文
pub fn extract_rar_file_ctx(
    path: &Path,
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    let packed = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    // 流式读：read_path 按需取载荷，不再整包进内存（X2：RAR 大小上限已删除）
    let archive = rars::ArchiveReader::read_path(path).map_err(|e| {
        Error::new(ErrorCode::NotArchive)
            .with("file", path.display().to_string())
            .context(format!("不是有效的 RAR（或版本不支持）: {e}"))
    })?;
    // 只给了一卷、但它自己末尾挂着「内容续到下一卷」→ 就是缺卷。
    // 不在这里拦，库会解到一半才失败，最后被归类成「这不是压缩包」（自相矛盾且误导）
    if 还有后续卷(&archive) {
        return Err(Error::new(ErrorCode::MissingVolume)
            .with("file", path.display().to_string())
            .context("这是分卷压缩包的一卷，后面还有卷没下回来（RAR 分卷要整组放齐）"));
    }
    单卷解压(archive, packed, opts, ctx)
}

/// 解压一组 RAR 分卷（`volumes` 必须按序，**第一卷在前**）
///
/// 分卷是 L1 批补上的能力（X1）：每卷单独 `read_path` 解析成 `Archive`，
/// 再交给 `rars::extract_volumes_to` 当成一条逻辑流解开。
/// 单卷（长度 1）直接走 [`extract_rar_file_ctx`]。
pub fn extract_rar_volumes_ctx(
    volumes: &[PathBuf],
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    let Some(首卷) = volumes.first() else {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "rar_no_volumes")
            .context("没给 RAR 分卷路径"));
    };
    if volumes.len() == 1 {
        return extract_rar_file_ctx(首卷, opts, ctx);
    }
    let mut archives = Vec::with_capacity(volumes.len());
    let mut packed = 0u64;
    for (i, p) in volumes.iter().enumerate() {
        let a = rars::ArchiveReader::read_path(p).map_err(|e| {
            Error::new(ErrorCode::MissingVolume)
                .with("volume", p.display().to_string())
                .with("index", (i + 1).to_string())
                .context(format!(
                    "第 {} 卷读不出来（{}）—— 可能没下全或下坏了: {e}",
                    i + 1,
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ))
        })?;
        packed = packed.saturating_add(fs::metadata(p).map(|m| m.len()).unwrap_or(0));
        archives.push(a);
    }

    // 缺卷自查：最后一卷末尾若还挂着「内容续到下一卷」，说明后面还有卷没给。
    // 以前没有这一步，缺卷的表现是「这不是压缩包」—— 用户完全看不懂自己缺了什么
    if let Some(末) = archives.last()
        && 还有后续卷(末)
    {
        return Err(Error::new(ErrorCode::MissingVolume)
            .with(
                "last_volume",
                volumes
                    .last()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )
            .context(format!(
                "分卷不完整：只找到 {} 卷，最后一卷后面还有卷没下回来",
                archives.len()
            )));
    }
    多卷解压(&archives, packed, opts, ctx)
}

/// 这一卷的末尾是不是还挂着「内容续到下一卷」的标记（RAR 的 `split_after` 位）？
///
/// 缺卷自查靠它：分卷包少给最后几卷时，前面每一卷都读得出来、也「看起来像」完整包，
/// 只有这个位能说明「后面还有」。拿不到成员列表就返回 false —— 宁可漏报，绝不乱报
/// （乱报会把好好的完整包拒收，比漏报还糟）。
fn 还有后续卷(a: &rars::Archive) -> bool {
    a.members()
        .last()
        .map(|m| m.meta.is_split_after)
        .unwrap_or(false)
}

/// 单卷：先 `test` 验密码（`rars` 解密失败不报错、只静默不写 → 必须先验），再流式解
fn 单卷解压(
    archive: rars::Archive,
    packed_total: u64,
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    // 还原修改时间要按 RAR 版本换算（RAR4 存 DOS 时间、RAR5 存 Unix 时间），先问库
    let family = archive.family();
    执行解压(
        packed_total,
        family,
        |pw| archive.test_with_options(读选项(pw)),
        |pw, cb| archive.extract_to_with_options(读选项(pw), cb),
        opts,
        ctx,
    )
}

/// 多卷：整组一起验密码、一起解（跨卷的逻辑成员也能正确拼起来）
fn 多卷解压(
    archives: &[rars::Archive],
    packed_total: u64,
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    // 同一组卷必然同版本，取第一卷的就行
    let family = archives
        .first()
        .map(|a| a.family())
        .unwrap_or(rars::ArchiveFamily::Rar50Plus);
    执行解压(
        packed_total,
        family,
        |pw| rars::test_volumes_with_options(archives, 读选项(pw)),
        |pw, cb| rars::extract_volumes_to_with_options(archives, 读选项(pw), cb),
        opts,
        ctx,
    )
}

/// 读选项：只带密码。解压炸弹限额仍由我们自己的 `ExtractBudget` 管，
/// 避免「两套限额语义打架、错误类型还对不上」。
fn 读选项(pw: Option<&[u8]>) -> rars::ArchiveReadOptions<'_> {
    match pw {
        Some(p) => rars::ArchiveReadOptions::with_password(p),
        None => rars::ArchiveReadOptions::new(),
    }
}

/// 从**一条流**解 RAR —— 只在「手上真的没有文件路径」时退化成整读
///
/// `rars` 的流式入口要求 `Read + Seek + Send + 'static`，统一接口给我们的是
/// `&mut dyn ReadSeek`，转不成 owned。所以这条路径退回「整读进内存」，
/// 上限是 [`STREAM_FALLBACK_MAX`]（管道 stdin 喂 300 MB 的 RAR 本来就不是正常用法）。
///
/// **正常解文件不走这里**：有路径时走 [`extract_rar_file_ctx`]（流式、无上限）。
pub fn extract_rar_stream_ctx<R: Read + Seek>(
    src: &mut R,
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    use std::io::SeekFrom;

    let size = src
        .seek(SeekFrom::End(0))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("取流长度失败: {e}")))?;
    if size > STREAM_FALLBACK_MAX {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "rar_stream_too_large")
            .with("size", size)
            .with("limit", STREAM_FALLBACK_MAX)
            .context(format!(
                "这条输入流喂了 {:.0} MB 的 RAR，超过管道方式的上限（{} MB）。\
                 请直接把这个 RAR 的**文件路径**交给程序（给路径时没有大小限制）。",
                size as f64 / 1_048_576.0,
                STREAM_FALLBACK_MAX / 1_048_576
            )));
    }
    src.seek(SeekFrom::Start(0))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("回到流开头失败: {e}")))?;
    let mut data = Vec::with_capacity(size.min(STREAM_FALLBACK_MAX) as usize);
    // 多读 1 字节：用来发现「流比 seek 报的长度还长」这种怪情况，别默默截断
    src.take(STREAM_FALLBACK_MAX + 1)
        .read_to_end(&mut data)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("读取失败: {e}")))?;
    extract_rar_ctx(&data, opts, ctx)
}

/// 从内存字节解压 RAR
///
/// 说明：`rars` 的提取回调语义是「库主动写、我们提供 Writer」，
/// 所以我们在这里包一层**带限额的 Writer** 来做解压炸弹防护。
pub fn extract_rar(data: &[u8], opts: &ExtractRarOptions) -> Result<crate::extract::ExtractReport> {
    extract_rar_ctx(data, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_rar`]，但带上事件 / 进度 / 取消上下文
///
/// 只有「手上已经是字节」的场景才走这条（管道 / 测试）；有文件路径请用
/// [`extract_rar_file_ctx`]（流式、无大小上限）。
pub fn extract_rar_ctx(
    data: &[u8],
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    let archive = rars::ArchiveReader::read(data).map_err(|e| {
        Error::new(ErrorCode::NotArchive).context(format!("不是有效的 RAR（或版本不支持）: {e}"))
    })?;
    单卷解压(archive, data.len().max(1) as u64, opts, ctx)
}

/// 解压主流程：验密码 → 逐条目写盘（单卷 / 多卷共用这一份）
///
/// `测` / `跑` 把「怎么用库」（单卷 `test` + `extract_to`、多卷 `test_volumes` +
/// `extract_volumes_to`）与「解出来往哪写、怎么记账」解耦，避免同一套落盘逻辑写两遍。
fn 执行解压<测, 跑>(
    packed_total: u64,
    family: rars::ArchiveFamily,
    测: 测,
    跑: 跑,
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport>
where
    测: Fn(Option<&[u8]>) -> std::result::Result<(), rars::Error>,
    跑: FnOnce(Option<&[u8]>, 打开条目<'_>) -> std::result::Result<(), rars::Error>,
{
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    fs::create_dir_all(&opts.dest)
        .map_err(|e| { let m = format!("建输出目录失败: {e}"); Error::from(e).context(m) })?;

    // 密码候选（UTF-8 → GBK → UTF-16LE）：RAR 与 ZIP 一样，密码只是任意字节，
    // 中文 Windows 的 WinRAR 写的是 CP936 —— 单用 UTF-8 必然报「密码错」。
    let 候选 = opts.password.as_ref().map(|p| crate::extract::密码候选(p));
    let mut pw_bytes: Option<Vec<u8>> = None;
    let dest = opts.dest.clone();
    let overwrite = opts.overwrite;
    let limits = opts.limits.clone();
    let has_pw = opts.password.is_some();
    let selection = opts.selection.clone();

    // 修改时间登记表：回调里记下 (落盘路径, 原始时间)，整包解完再统一补 ——
    // 解压途中改会被库后续的写入冲掉，所以必须等全部写完
    let 时间表 = Rc::new(RefCell::new(Vec::<(std::path::PathBuf, std::time::SystemTime)>::new()));
    let 时间表2 = 时间表.clone();
    // RAR 回调不给条目序号，我们自己数（预览→解压的挑选锚点就靠它）
    let idx_cell = Rc::new(std::cell::Cell::new(0u64));
    let idx_ref = idx_cell.clone();
    // 上下文里全是 Arc / 可克隆量，放进回调没有生命周期顾虑
    // （外层仍用借用版的 ctx，闭包里用这份 owned 的）
    let ctx_owned = ctx.clone();

    // ── 先验证密码 ──
    // 重要：`rars` 在解密失败时**不报错**，只是静默不写数据（会产出空文件）。
    // 所以必须先主动 test 一遍，否则用户会得到一堆 0 字节的"成功"产物。
    // test 对每个编码候选各跑一次，命中就用它去 extract。
    let mut 校验错误: Option<String> = None;
    match &候选 {
        Some(list) => {
            for cand in list {
                match 测(Some(cand)) {
                    Ok(()) => {
                        pw_bytes = Some(cand.clone());
                        break;
                    }
                    Err(e) => 校验错误 = Some(e.to_string()),
                }
            }
        }
        None => {
            if let Err(e) = 测(None) {
                校验错误 = Some(e.to_string());
            }
        }
    }
    if pw_bytes.is_none()
        && let Some(msg) = 校验错误
    {
        let low = msg.to_lowercase();
        let is_pw = low.contains("password")
            || low.contains("crypto")
            || low.contains("checksum")
            || low.contains("crc")
            || msg.contains("密码");
        let code = if is_pw {
            if opts.password.is_some() {
                ErrorCode::WrongPassword
            } else {
                ErrorCode::NeedPassword
            }
        } else {
            ErrorCode::NotArchive
        };
        return Err(Error::new(code).context(format!("RAR 校验失败（密码可能不对）: {msg}")));
    }

    // 回调里要改报告，用 Rc<RefCell> 共享（单线程提取）
    let report = Rc::new(RefCell::new(crate::extract::ExtractReport::default()));
    // 解压总量预算（分母 = 真实压缩后的总字节数：文件大小 / 各卷之和）
    let budget = Rc::new(RefCell::new(crate::safety::ExtractBudget::new(
        limits,
        packed_total.max(1),
    )));

    let rep2 = report.clone();
    let bud2 = budget.clone();
    // 闭包里要用「有没有密码」这个事实，但 `pw_bytes` 已经被 move 进去了 —— 先取值
    let 有密码 = pw_bytes.is_some();

    let res = 跑(pw_bytes.as_deref(), Box::new(move |meta| {
        // RAR 回调不给条目序号，我们自己数（**1-based**，与预览显示的号一致）
        let idx = idx_ref.get() + 1;
        idx_ref.set(idx);
        let name = String::from_utf8_lossy(&meta.name).into_owned();

        // 用户取消：后面的条目一律不写盘（这里没法真正打断库，只能让它空转完）
        if ctx_owned.is_cancelled() {
            rep2.borrow_mut().cancelled = true;
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }

        if meta.is_directory {
            let dir = safety::resolve_safe(&dest, &name)
                .unwrap_or_else(|_| dest.join(name.replace(['/', '\\'], "_")));
            // 断点续传：上次已经建过 → 跳
            if ctx_owned.resume_skip(&name, &dir, crate::resume::条目种类::目录) {
                ctx_owned.entry_done(idx, &name, ItemResult::Ok, 0);
                return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
            }
            let _ = fs::create_dir_all(&dir);
            ctx_owned.resume_record(&name, &dir, crate::resume::条目种类::目录);
            ctx_owned.entry_done(idx, &name, ItemResult::Ok, 0);
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }

        // 挑选：没被勾中的条目直接跳过（不写盘、不算错）
        if !selection.matches(idx, &name, false) {
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }

        // 安全路径（防穿越 / 保留名 / ADS）
        let safe_path = match safety::resolve_safe(&dest, &name) {
            Ok(p) => p,
            Err(e) => {
                rep2.borrow_mut()
                    .skipped
                    .push((name.clone(), format!("路径不安全被拒: {e}")));
                return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
            }
        };

        // 同名策略
        let mut out_path = safe_path;

        // 断点续传：上次完整解过这条 → 不重解（必须在同名处理之前，理由同 7z）
        if ctx_owned.resume_skip(&name, &out_path, crate::resume::条目种类::文件) {
            let 大小 = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            ctx_owned.entry_done(idx, &name, ItemResult::Ok, 0);
            {
                let mut r = rep2.borrow_mut();
                r.files.push(crate::extract::ExtractedFile {
                    name: name.clone(),
                    out_path: out_path.clone(),
                    size: 大小,
                    encrypted: 有密码,
                });
            }
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }

        if out_path.exists() {
            match ctx_owned.同名策略(overwrite) {
                crate::extract::Overwrite::Skip => {
                    rep2.borrow_mut()
                        .skipped
                        .push((name.clone(), "已存在，按策略跳过".into()));
                    return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
                }
                crate::extract::Overwrite::Replace => {}
                crate::extract::Overwrite::Rename => out_path = unique_path(&out_path),
            }
        }
        if !ctx.verify_only
            && let Some(parent) = out_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            rep2.borrow_mut()
                .skipped
                .push((name.clone(), format!("建父目录失败: {e}")));
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }
        if safety::should_warn_executable(&opts.limits, &name) {
            ctx.diagnostic(crate::event::DiagnosticCode::RiskyEntry, name.clone());
        }

        let f = match crate::io::PartFile::create_with(&out_path, ctx_owned.verify_only) {
            Ok(f) => f,
            Err(e) => {
                rep2.borrow_mut().note_io_error(&e);
                rep2.borrow_mut()
                    .skipped
                    .push((name.clone(), format!("创建失败: {e}")));
                return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
            }
        };

        // 文件建出来了才登记修改时间（创建失败的条目不该留登记，否则解完会冒出假警告）。
        // `extracted_system_time` 自己按 RAR 版本换算：RAR4 是 DOS 时间、RAR5 是 Unix 秒
        if let Some(t) =
            rars::timestamp::extracted_system_time(
                family,
                meta.file_time,
                meta.mtime_refinement,
            )
        {
            时间表2.borrow_mut().push((out_path.clone(), t));
        }

        // 包一层：边写边记限额与统计
        Ok(Box::new(CountingWriter {
            part: Some(f),
            written: 0,
            budget: bud2.clone(),
            report: rep2.clone(),
            name: name.clone(),
            out_path,
            encrypted: has_pw,
            ctx: ctx_owned.clone(),
            idx,
            cancelled: false,
        }) as Box<dyn Write>)
    }));

    // 回调结束后把 Rc 里的结果取出来
    let mut final_report = Rc::try_unwrap(report)
        .map(|c| c.into_inner())
        .unwrap_or_default();

    if let Err(e) = res {
        // 用户取消：不是错误，如实返回「已解出多少」（台账 §5.8）
        if final_report.cancelled {
            ctx.force_progress(None);
            return Ok(final_report);
        }
        if final_report.files.is_empty() {
            let msg = e.to_string();
            let low = msg.to_lowercase();
            let is_pw = low.contains("password") || low.contains("crypto") || msg.contains("密码");
            let code = if is_pw {
                if opts.password.is_some() {
                    ErrorCode::WrongPassword
                } else {
                    ErrorCode::NeedPassword
                }
            } else {
                ErrorCode::NotArchive
            };
            return Err(Error::new(code).context(format!("RAR 解压失败: {msg}")));
        }
        final_report
            .warnings
            .push(format!("解压过程报错（已解出部分）: {e}"));
    }
    // RAR 现在也能还原修改时间了：整包解完再统一补 —— 此刻文件都已关闭，
    // 改 mtime 不会被后续写入冲掉。失败只记警告，不影响「解出来了」这个结论。
    for (p, t) in 时间表.borrow().iter() {
        let 结果 = fs::OpenOptions::new()
            .write(true)
            .open(p)
            .and_then(|f| f.set_modified(*t));
        if let Err(e) = 结果 {
            final_report
                .warnings
                .push(format!("还原修改时间失败 {}: {e}", p.display()));
        }
    }

    final_report.files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(final_report)
}

/// 带限额与统计的 Writer：库每写一段，我们就记账
///
/// # 为什么写 `.yshjy-part` 而不是直接写正式名（梯队四第 12 项）
///
/// RAR 是**唯一**一条以前直接写正式名的落盘路径（另外六条都走 `PartFile`）。
/// 后果：解到一半被 `kill -9` / 断电时，磁盘上留下的是**顶着正式文件名**的半截文件，
/// 外观与好文件无异，重试还会因为「同名改名」策略绕开它、长出 `(1)` 副本。
///
/// # 为什么在 `Drop` 里就 `finish()`，而不是等整包结束
///
/// `rars` 的 `ExtractedEntryMeta` **只给名字/时间/属性/是否目录，没有条目大小，
/// 也没有 CRC** —— 回调这一层拿不到「应该有 734003200 字节」这种信息。
/// 所以没法在 `Drop` 里判断「库是写完了还是中途出错」，`Drop` 只能当作
/// 「库对这个条目的写入结束了」。
///
/// 权衡（有意接受的）：库因为 CRC / 解密失败而提前 `drop` writer 时，
/// 已经写出的字节仍会落地（和改之前一样会落地，**不是新增的退步**）；
/// 换来的是「进程被杀 / 断电不再留半截正式名」这个真实收益。
/// 想彻底解决「坏数据落地」，得迁到 `extract_with_control_and_errors` 拿逐条目错误
/// —— 那要重写整个回调层（`decide` 用 `ArchiveMember`，写出机制也不同），
/// 风险和收益不匹配，先记账（见 `docs/TODO.md` 第 12 项备注）。
struct CountingWriter {
    /// 原子写入守卫：写 `<名字>.yshjy-part`，`Drop` 时被关闭
    part: Option<crate::io::PartFile>,
    written: u64,
    budget: std::rc::Rc<std::cell::RefCell<crate::safety::ExtractBudget>>,
    report: std::rc::Rc<std::cell::RefCell<crate::extract::ExtractReport>>,
    name: String,
    out_path: PathBuf,
    encrypted: bool,
    /// 事件 / 进度 / 取消上下文（内部全是 Arc，克隆很轻）
    ctx: UnpackCtx,
    /// 条目序号（事件里用）
    idx: u64,
    /// 是否因用户取消而中途停了
    cancelled: bool,
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // 取消检查下沉到数据块级：正在解一个大文件时也能及时停
        if self.ctx.is_cancelled() {
            self.cancelled = true;
            self.report.borrow_mut().cancelled = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled by user",
            ));
        }
        // 解压炸弹防护
        if let Err(e) = self.budget.borrow_mut().record(buf.len() as u64) {
            let mut r = self.report.borrow_mut();
            r.budget_hit = true;
            r.skipped
                .push((self.name.clone(), format!("安全限额中止: {e}")));
            return Err(std::io::Error::other(format!("安全限额中止: {e}")));
        }
        let Some(part) = self.part.as_mut() else {
            // 已经 finish/abort 过了，库不该再写；静默吞掉比 panic 好（不能因为
            // 库多写一次就把已经解好的整包弄崩）
            return Ok(buf.len());
        };
        let n = part.as_file().write(buf)?;
        self.written += n as u64;
        self.ctx.advance(n as u64, Some(&self.name));
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.part.as_mut() {
            Some(part) => part.as_file().flush(),
            None => Ok(()),
        }
    }
}

impl Drop for CountingWriter {
    fn drop(&mut self) {
        let Some(part) = self.part.take() else {
            return;
        };
        // 取消 / 限额中止：part 由 `Drop` 删掉，**正式文件名不会出现**
        // （以前这里是 `remove_file(&out_path)` —— 也就是先写坏正式名再删，
        //  进程在中间被杀就永久留下半截文件）
        if self.cancelled {
            drop(part);
            return;
        }
        // 原子落地：改名成功之前，正式文件名还不存在
        if let Err(e) = part.finish() {
            let mut r = self.report.borrow_mut();
            r.note_io_error(&e);
            r.skipped
                .push((self.name.clone(), format!("落盘改名失败: {e}")));
            self.ctx
                .entry_done(self.idx, &self.name, ItemResult::DataError, 0);
            return;
        }
        // 成功落地才登记
        let mut r = self.report.borrow_mut();
        r.bytes_written += self.written;
        // 断点续传：Drop 只在「写成功、没被取消」这条路上走到这里，正是该记的时刻
        self.ctx
            .resume_record(&self.name, &self.out_path, crate::resume::条目种类::文件);
        self.ctx
            .entry_done(self.idx, &self.name, ItemResult::Ok, self.written);
        r.files.push(crate::extract::ExtractedFile {
            name: self.name.clone(),
            out_path: self.out_path.clone(),
            size: self.written,
            encrypted: self.encrypted,
        });
    }
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let cand = match ext {
            Some(e) => parent.join(format!("{stem} ({i}).{e}")),
            None => parent.join(format!("{stem} ({i})")),
        };
        if !cand.exists() {
            return cand;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rar_解出的文件带包内原始修改时间() {
        // 阶段 1 梯队四·第 1 项：以前 RAR 解出的文件时间就是「解压那一刻」，
        // 现在要还原成包里记的原始时间。样本里的时间是 2012-07-13。
        use crate::ctx::UnpackCtx;

        let data = fs::read("tests/data/rar_mtime_sample.rar").expect("样本不见了");
        let dest = scratch("rar_mtime");
        let ctx = UnpackCtx::quiet(1);
        let opts = ExtractRarOptions::new(&dest);

        let rep = extract_rar_ctx(&data, &opts, &ctx).unwrap();
        assert!(!rep.files.is_empty(), "样本该能解出文件：{:?}", rep.warnings);

        // 2013-01-01 00:00:00 UTC = 1356998400。原始时间是 2012 年，必须早于这个点；
        // 如果还原失败，文件时间会是「现在」（2026 年），这条就会挂。
        for f in &rep.files {
            let t = fs::metadata(&f.out_path)
                .and_then(|m| m.modified())
                .expect("拿不到修改时间");
            let secs = t
                .duration_since(std::time::UNIX_EPOCH)
                .expect("时间早于 1970")
                .as_secs();
            assert!(
                secs < 1_356_998_400,
                "{} 的修改时间没被还原（{secs}，2013 年之后 = 还是解压当下）",
                f.name
            );
            assert!(secs > 0, "{} 的时间成了 1970 年，别把缺时间写成 0", f.name);
        }
    }

    #[test]
    fn 挑选与进度在_rar_路径上也生效() {
        use crate::ctx::UnpackCtx;
        use crate::event::{ChannelSink, Event};

        let data = make_rar(&[("a.txt", b"aaa"), ("b.txt", b"bbb")], None);
        let dest = scratch("ctx_out");
        let sink = ChannelSink::new(256);
        let ctx = UnpackCtx::quiet(1).with_sink(sink.clone());

        let mut opts = ExtractRarOptions::new(&dest);
        opts.selection = Selection::matching("a.txt");

        let rep = extract_rar_ctx(&data, &opts, &ctx).unwrap();
        assert_eq!(rep.files.len(), 1, "只该解出 a.txt：{:?}", rep.files);
        let evs = sink.drain();
        assert!(
            evs.iter()
                .any(|e| matches!(e, Event::EntryFinished { .. })),
            "应当有条目级事件"
        );
        assert!(ctx.progress.view().done_bytes > 0, "应当累计了字节数");
    }

    #[test]
    fn 一上来就取消时_rar_不报错也不解东西() {
        use crate::ctx::UnpackCtx;
        use crate::progress::CancelToken;

        let data = make_rar(&[("a.txt", b"aaa")], None);
        let dest = scratch("cancel_out");
        let cancel = CancelToken::new();
        cancel.cancel();
        let ctx = UnpackCtx::quiet(1).with_cancel(cancel);

        let rep = extract_rar_ctx(&data, &ExtractRarOptions::new(&dest), &ctx).unwrap();
        assert!(rep.cancelled, "应当标记为已取消");
        assert!(rep.files.is_empty());
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_rar_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// X2 回归：超过旧上限（256 MiB）的 RAR 现在能正常解（流式，不再整包进内存）
    ///
    /// 慢测试，默认不跑：`cargo test --release -- --ignored 大rar`
    #[test]
    #[ignore = "慢测试：造 300 MiB 的 RAR（跑法：cargo test --release -- --ignored 大rar）"]
    fn 大rar不再被大小上限拦住() {
        use rars::ArchiveVersion;
        let dir = scratch("big");
        let p = dir.join("big.rar");
        let 尺寸 = 300 * 1024 * 1024u64;
        {
            let mut b = rars::Builder::new(ArchiveVersion::Rar50).store(true);
            b.add_bytes(
                b"big.bin".to_vec(),
                vec![7u8; 尺寸 as usize],
                None,
                None,
            )
            .unwrap();
            fs::write(&p, b.to_bytes().unwrap()).unwrap();
        }
        let 实际 = fs::metadata(&p).unwrap().len();
        assert!(
            实际 > STREAM_FALLBACK_MAX,
            "样本必须超过旧上限才有意义：{实际} 字节"
        );
        let dest = scratch("big_out");
        let rep = extract_rar_file(&p, &ExtractRarOptions::new(&dest)).unwrap();
        assert_eq!(rep.files.len(), 1, "应解出 1 个：{:?}", rep.skipped);
        let 解出 = fs::metadata(dest.join("big.bin")).unwrap().len();
        assert_eq!(解出, 尺寸, "解出的字节数要对得上");
        println!("大 RAR 实测：包 {实际} 字节 → 解出 {解出} 字节");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    /// 用 rars 的写侧造一个 RAR（仅测试用；主程序不含写侧）
    fn make_rar(files: &[(&str, &[u8])], password: Option<&str>) -> Vec<u8> {
        use rars::ArchiveVersion;
        let mut b = rars::Builder::new(ArchiveVersion::Rar50);
        if let Some(pw) = password {
            b = b.password(Some(pw.as_bytes().to_vec()));
        }
        for (name, data) in files {
            b.add_bytes(name.as_bytes().to_vec(), data.to_vec(), None, None)
                .unwrap();
        }
        b.to_bytes().unwrap()
    }

    #[test]
    fn 无密码_rar_能解出() {
        let dir = scratch("plain");
        let data = make_rar(
            &[
                ("hello.txt", b"hello rar world"),
                ("b.bin", &vec![3u8; 2000]),
            ],
            None,
        );
        let p = dir.join("t.rar");
        fs::write(&p, &data).unwrap();

        let dest = scratch("plain_out");
        let opts = ExtractRarOptions::new(&dest);
        let rep = extract_rar_file(&p, &opts).unwrap();
        assert_eq!(rep.files.len(), 2, "应解出 2 个: {:?}", rep.skipped);
        assert_eq!(
            fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "hello rar world"
        );
        assert_eq!(fs::read(dest.join("b.bin")).unwrap().len(), 2000);

        // 梯队四第 12 项：RAR 也改成「先写 .yshjy-part，写完再原子改名」了 ——
        // 解完目录里不该留下任何中间态
        let 残留: Vec<String> = fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".yshjy-part"))
            .collect();
        assert!(残留.is_empty(), "解完不该残留 part：{残留:?}");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 带密码的_rar_能解() {
        let dir = scratch("pw");
        let data = make_rar(&[("secret.txt", "机密内容".as_bytes())], Some("密码abc"));
        let p = dir.join("pw.rar");
        fs::write(&p, &data).unwrap();

        let dest = scratch("pw_out");
        let mut opts = ExtractRarOptions::new(&dest);

        // 不给密码 → 要么报错，要么解出的内容不对（不能"悄悄解成垃圾"）
        let no_pw = extract_rar_file(&p, &opts);
        if let Ok(rep) = &no_pw
            && !rep.files.is_empty()
        {
            let got = fs::read(dest.join("secret.txt")).unwrap_or_default();
            assert_ne!(
                got,
                "机密内容".as_bytes(),
                "没给密码却解出了正确明文 —— 加密没生效，这是严重问题"
            );
        }

        // 给对密码 → 解出
        opts.password = Some("密码abc".into());
        let rep = extract_rar_file(&p, &opts).unwrap();
        assert_eq!(rep.files.len(), 1, "给对密码应解出: {:?}", rep.skipped);
        assert_eq!(
            fs::read_to_string(dest.join("secret.txt")).unwrap(),
            "机密内容"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    /// X2 的回归：**不再有「太大」这道门**
    ///
    /// 造一个「名字是 .rar、体积 300 MiB、内容不是 RAR」的稀疏文件：
    /// 旧代码会先报「超过 256 MB 上限」，新代码不该再有大小门，而是老实说「这不是 RAR」。
    #[test]
    fn 超大_rar_不再被大小检查拦住() {
        let dir = scratch("huge");
        let p = dir.join("huge.rar");
        let f = fs::File::create(&p).unwrap();
        f.set_len(300 * 1024 * 1024).unwrap();
        drop(f);

        let opts = ExtractRarOptions::new(dir.join("out"));
        let e = extract_rar_file(&p, &opts).unwrap_err();
        let msg = format!("{e}");
        assert!(!msg.contains("上限"), "不该再因为大小拒绝：{msg}");
        assert!(
            msg.contains("不是有效的 RAR"),
            "要说清是内容不对（不是太大）：{msg}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 不是_rar_的文件报错而不是崩() {
        let opts = ExtractRarOptions::new(scratch("bad_out"));
        let r = extract_rar(&[0u8; 100], &opts);
        assert!(r.is_err());
    }

    #[test]
    fn 路径穿越有防护() {
        // 说明：rars 的**写侧**本身就拒绝造 `../` 这样的包（返回 UnsafePath），
        // 所以我们没法用它造出恶意样本。这里验证两件事：
        // 1) 写侧确实会拒绝（说明库有防护，是好事）
        // 2) 我们的安全路径检查函数确实能拦住穿越路径
        use rars::ArchiveVersion;
        let mut b = rars::Builder::new(ArchiveVersion::Rar50);
        let rejected = b
            .add_bytes(b"../../evil.txt".to_vec(), b"pwned".to_vec(), None, None)
            .is_err();
        assert!(rejected, "写侧应当拒绝不安全的路径");

        // 我们的解析器（防穿越的核心逻辑）
        let root = scratch("slip_root");
        assert!(safety::resolve_safe(&root, "../../evil.txt").is_err());
        assert!(safety::resolve_safe(&root, "/etc/passwd").is_err());
        assert!(safety::resolve_safe(&root, "ok/inside.txt").is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    // ── 梯队四第 12 项：RAR 落盘改走 .yshjy-part 之后的两条语义 ──
    //
    // 为什么用单测盯这件事：这台机器上没有 `rar` 命令，仓库里的 RAR 样本又只有几 KB，
    // **造不出「解到一半被 kill -9」的实测场景**。而这两条语义（写的时候不许有正式名、
    // 取消时什么都不留）正是那次改造的全部意义，必须靠单测钉死。

    /// 造一个只差「写多少」的 CountingWriter
    fn 造写入器(目标: &std::path::Path) -> CountingWriter {
        CountingWriter {
            part: Some(crate::io::PartFile::create(目标).unwrap()),
            written: 0,
            budget: std::rc::Rc::new(std::cell::RefCell::new(
                crate::safety::ExtractBudget::new(crate::safety::Limits::default(), 1),
            )),
            report: std::rc::Rc::new(std::cell::RefCell::new(
                crate::extract::ExtractReport::default(),
            )),
            name: "a.bin".into(),
            out_path: 目标.to_path_buf(),
            encrypted: false,
            ctx: crate::ctx::UnpackCtx::quiet(0),
            idx: 1,
            cancelled: false,
        }
    }

    #[test]
    fn rar写入器_写的过程中只有part_结束后才出现正式名() {
        use std::io::Write;
        let d = scratch("rar_atomic_ok");
        let out = d.join("a.bin");
        let part = d.join("a.bin.yshjy-part");
        {
            let mut w = 造写入器(&out);
            w.write_all(b"hello rar").unwrap();
            assert!(!out.exists(), "还在写的时候绝不能有正式文件名（这正是要修的老毛病）");
            assert!(part.exists(), "应该先写 .yshjy-part");
        } // drop → 原子改名
        assert!(out.exists(), "写完并 Drop 后必须有正式文件名");
        assert!(!part.exists(), "改名后不该留 part");
        assert_eq!(fs::read(&out).unwrap(), b"hello rar");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rar写入器_取消时既不留正式名也不留part() {
        use std::io::Write;
        let d = scratch("rar_atomic_cancel");
        let out = d.join("a.bin");
        let part = d.join("a.bin.yshjy-part");
        {
            let mut w = 造写入器(&out);
            w.write_all(b"half").unwrap();
            w.cancelled = true; // 用户取消 / 限额中止
        }
        assert!(!out.exists(), "取消后不该有正式文件名");
        assert!(!part.exists(), "取消后 part 也该清掉");
        let _ = fs::remove_dir_all(&d);
    }
}
