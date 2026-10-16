//! 单任务编排：一个文件从「认出是什么」到「解出来」的完整流程
//!
//! # 为什么编排必须在引擎里（台账 `7zip-工程层.md` §7.7）
//!
//! v0.4 的编排逻辑全在 `main.rs` 里（近千行 `do_extract_*` 分支）。
//! 那意味着**GUI 想解一个包，得把 CLI 的逻辑抄一遍** —— 这正是 7-Zip 踩过的坑
//! （两个壳各写一份落盘逻辑，结果行为不一致）。
//!
//! 所以 v0.5 把「识别 → 定位 → 分派 → 结果映射」整体搬进引擎：
//!
//! ```text
//! CLI ─┐
//!      ├─→ job::run_job ─→ identify（认出格式/算出偏移）
//! GUI ─┘                 └─→ extract_*_ctx（带事件/进度/取消）
//! ```
//!
//! 外壳只负责两件事：**把事件渲染成人话**、**把结果码变成退出码**。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::batch::{BatchOptions, Job, JobOutcome, JobStats};
use crate::ctx::UnpackCtx;
use crate::error::{ArchiveFlags, Error, ErrorCode, ItemResult, Outcome, Result};
use crate::event::{DiagnosticCode, EventSink, Stage};
use crate::extract::{ExtractOptions, ExtractReport};
use crate::extract_rar::ExtractRarOptions;
use crate::extract_stream::{self, ExtractStreamOptions};
use crate::extract7z::Extract7zOptions;
use crate::layout::ArchiveFormat;
use crate::progress::CancelToken;

/// 认出来的目标
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// ZIP（含伪装包）：`base`/`len` 是压缩数据在文件里的位置
    Zip {
        /// 数据起点
        base: u64,
        /// 数据长度
        len: u64,
    },
    /// 7z（可能是分卷，`volumes` 已按序排好）
    SevenZ {
        /// 分卷路径列表
        volumes: Vec<PathBuf>,
    },
    /// ZIP 分卷（`.z01` / `.z02` … 最后一块是 `.zip`）
    ///
    /// 拼成一个逻辑流后，就和一个普通 ZIP 一模一样（中央目录里的偏移是相对整条流的）。
    ZipSplit {
        /// 分卷路径（已按序排好，最后一块是 `.zip`）
        volumes: Vec<PathBuf>,
    },
    /// RAR（只解不压）
    Rar,
    /// 单流压缩 / TAR 家族
    Stream {
        /// 外层压缩格式
        format: ArchiveFormat,
        /// 是不是 tar 家族（`.tar.gz` 这类要剥两层）
        is_tar: bool,
        /// 压缩数据大小（炸弹防护的分母）
        packed: u64,
    },
}

impl Target {
    /// 中性名字（日志/诊断用）
    pub fn as_str(&self) -> &'static str {
        match self {
            Target::Zip { .. } => "zip",
            Target::ZipSplit { .. } => "zip",
            Target::SevenZ { .. } => "7z",
            Target::Rar => "rar",
            Target::Stream { format, is_tar, .. } => {
                if *is_tar {
                    "tar"
                } else {
                    match format {
                        ArchiveFormat::Gzip => "gzip",
                        ArchiveFormat::Xz => "xz",
                        ArchiveFormat::Zstd => "zstd",
                        ArchiveFormat::Bzip2 => "bzip2",
                        ArchiveFormat::Tar => "tar",
                        _ => "stream",
                    }
                }
            }
        }
    }
}

// ───────────────────────────── 识别 ─────────────────────────────

/// 认出这是什么（会发阶段事件）
pub fn identify(path: &Path, ctx: &UnpackCtx) -> Result<Target> {
    ctx.stage_started(Stage::Locate);
    let t0 = Instant::now();
    let r = identify_inner(path, ctx);
    ctx.stage_finished(Stage::Locate, elapsed_ms(t0));
    r
}

fn identify_inner(path: &Path, ctx: &UnpackCtx) -> Result<Target> {
    // ①' ZIP 分卷（`.z01`/`.z02`… + `.zip`）
    //     **必须在 7z 分卷之前判**：`.z01` 是 ZIP 分卷的专用后缀，
    //     而 7z 分卷的编号从 `.001` 起（常带 `.7z` 前缀）—— 顺序反了会被 7z 抢走。
    if let Some(volumes) = as_zip_volumes(path) {
        return Ok(Target::ZipSplit { volumes });
    }
    // ① 7z（含分卷）
    if let Some(volumes) = as_7z_volumes(path) {
        return Ok(Target::SevenZ { volumes });
    }
    // ② RAR
    if is_rar(path) {
        return Ok(Target::Rar);
    }
    // ③ 单流 / TAR
    if let Some(d) = sniff_stream(path) {
        let packed = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        // `.tar.gz` 的头部只有 gzip 签名，光看头部判断不出里面是 tar → 按文件名补判
        let is_tar = d.is_tar || extract_stream::looks_like_tar_by_name(path);
        return Ok(Target::Stream {
            format: d.format,
            is_tar,
            packed,
        });
    }
    // ③' Brotli：**没有魔数**（格式就这么设计的），只能按扩展名 `.br` 认
    if extract_stream::looks_like_brotli_by_name(path) {
        let packed = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        return Ok(Target::Stream {
            format: ArchiveFormat::Brotli,
            is_tar: extract_stream::looks_like_tar_by_name(path),
            packed,
        });
    }
    // ④ ZIP：先算偏移（快），不行再全文件扫描（慢）
    locate_zip(path, ctx)
}

fn locate_zip(path: &Path, ctx: &UnpackCtx) -> Result<Target> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open: {e}")))?;
    let size = f
        .metadata()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("stat: {e}")))?
        .len();

    if let Ok(l) = crate::layout::zip::locate(&mut f, size) {
        return Ok(Target::Zip {
            base: l.base,
            len: l.data_len(),
        });
    }

    // 兜底：包藏在文件中间 → 全文件扫描（慢路，要告诉用户）
    ctx.diagnostic(DiagnosticCode::SlowPath, "locate failed; scanning whole file");
    let hits = crate::scan::scan_whole_file(&mut f, size, &crate::scan::ScanOptions::strict())
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("scan: {e}")))?;
    if hits.is_empty() {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("path", path.display().to_string())
            .context("no archive found in file"));
    }
    let l = crate::layout::zip::locate_with_window(&mut f, size, size)
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("locate_wide: {e}")))?;
    ctx.diagnostic(
        DiagnosticCode::HiddenAtOffset,
        format!("base={} len={}", l.base, l.data_len()),
    );
    Ok(Target::Zip {
        base: l.base,
        len: l.data_len(),
    })
}

/// 按扩展名或签名判断是不是 RAR
fn is_rar(path: &Path) -> bool {
    let by_ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase() == "rar")
        .unwrap_or(false);
    if by_ext {
        return true;
    }
    // RAR4: "Rar!\x1a\x07\x00" ，RAR5: "Rar!\x1a\x07\x01\x00"
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 8];
        if f.read_exact(&mut buf).is_ok() {
            return buf.starts_with(b"Rar!\x1a\x07");
        }
    }
    false
}

/// 认出「ZIP 分卷」（`.z01` / `.z02` … + 最后一块 `.zip`）
///
/// 两种入口都给同一个结果：
/// - 用户点 `.z01` → 往后收集 `.z02`、`.z03`… 最后接上 `.zip` 收尾块
/// - 用户点 `.zip` → 看同目录有没有同名的 `.z01`（有 → 这就是分卷的收尾块）
///
/// 和 7z 分卷的区别：7z 是「裸字节切分」；ZIP 分卷每块自成一体，
/// 但**中央目录里的偏移是相对整条逻辑流的**，所以拼起来就能当普通 ZIP 读。
fn as_zip_volumes(path: &Path) -> Option<Vec<PathBuf>> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let lower = name.to_lowercase();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    // 主干名：去掉 `.zip` 或 `.z01` 这类尾巴
    let stem = if lower.ends_with(".zip") {
        name[..name.len() - 4].to_string()
    } else {
        let pos = lower.rfind(".z")?;
        let digits = &lower[pos + 2..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        name[..pos].to_string()
    };
    if stem.is_empty() {
        return None;
    }

    // 从 .z01 开始按序收集（缺一块就停 —— 不去猜后面的编号）
    let mut volumes = Vec::new();
    for n in 1..=99u32 {
        let p = dir.join(format!("{stem}.z{n:02}"));
        if p.exists() {
            volumes.push(p);
        } else {
            break;
        }
    }
    if volumes.is_empty() {
        return None; // 连第一块都不在 → 不是分卷
    }
    // 收尾块 `.zip`：没有它说明下载缺块，老实报错比猜好
    let last = dir.join(format!("{stem}.zip"));
    if !last.exists() {
        return None;
    }
    volumes.push(last);
    Some(volumes)
}

/// 看文件头判断单流格式 / TAR
fn sniff_stream(path: &Path) -> Option<extract_stream::Detected> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 512];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    extract_stream::detect(&head)
}

/// 判断是不是 7z（含分卷），是的话返回完整的分卷列表
///
/// 支持 `x.7z` / `x.7z.001` / `x.001` 三种形式
pub fn as_7z_volumes(path: &Path) -> Option<Vec<PathBuf>> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let lower = name.to_lowercase();

    // 单卷 .7z
    if lower.ends_with(".7z") {
        return Some(vec![path.to_path_buf()]);
    }

    // 分卷：按序号往后找，序号断了就停
    let (base, _num, width) = split_volume_name(&name)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut vols = Vec::new();
    for i in 1..=9999u32 {
        let cand = dir.join(format!("{base}{i:0width$}"));
        if cand.exists() {
            vols.push(cand);
        } else if i > 1 {
            break;
        } else {
            return None; // 第一块都不在，说明不是分卷
        }
    }
    if vols.is_empty() { None } else { Some(vols) }
}

/// 从 `x.7z.001` 这样的名字里拆出 (前缀, 序号, 序号宽度)
///
/// ⚠️ 必须按**字符边界**找数字起点：`rfind` 返回的是字节位置，
/// 对「以中文结尾的名字 +1」会落在字符中间直接 panic（v0.4 的旧实现有这个坑，
/// 中文名文件会崩 —— 实测 `split_volume_name("没有数字")` 就炸）。
fn split_volume_name(name: &str) -> Option<(String, u32, usize)> {
    let digits_start = name
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, c)| i + c.len_utf8())?;
    if digits_start >= name.len() {
        return None;
    }
    let digits = &name[digits_start..];
    let num: u32 = digits.parse().ok()?;
    Some((name[..digits_start].to_string(), num, digits.len()))
}

// ───────────────────────────── 输出目录 ─────────────────────────────

/// 决定解到哪
///
/// 优先级：任务自带的目录 > 智能解压三规则 > 「去掉外壳扩展名的同名文件夹」
///
/// 智能解压三规则（UI-SPEC §9.2，源自 Bandizip，实战最省事）：
/// 1. 包里只有**一个顶层目录** → 直接解到目标目录（那层由包里的结构自己带出来）
/// 2. 只有一个文件 → 也直接放目标目录
/// 3. 一堆散文件 → 建同名子目录（免得几十个文件散在下载目录里）
pub fn resolve_dest(job: &Job, opts: &BatchOptions) -> PathBuf {
    decide_dest(job, opts, None)
}

/// 带预览结果的目录决策（`pv` 为 `None` 时退回 v0.5 的老行为）
pub fn decide_dest(job: &Job, opts: &BatchOptions, pv: Option<&crate::preview::Preview>) -> PathBuf {
    // ① 任务自带目录优先（批量时 CLI 已经给每个包分好子目录了）
    if let Some(d) = &job.dest {
        return d.clone();
    }
    let legacy = || match &opts.dest {
        Some(d) => d.clone(),
        None => PathBuf::from(output_dir_name(&job.input)),
    };
    if !opts.smart_dir {
        return legacy();
    }
    let base = match &opts.dest {
        Some(d) => d.clone(),
        None => PathBuf::from("."),
    };
    match pv {
        Some(p) => crate::preview::smart_dest(&base, p, &output_dir_name(&job.input)),
        None => legacy(),
    }
}

/// 默认输出目录名：去掉「外壳扩展名」（`.mp4` / `.mkv` / `.zip`）后的同名
pub fn output_dir_name(input: &Path) -> String {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "解压结果".into());
    let cleaned = stem
        .trim_end_matches(".mp4")
        .trim_end_matches(".mkv")
        .trim_end_matches(".zip")
        .to_string();
    if cleaned.is_empty() { stem } else { cleaned }
}

// ───────────────────────────── 主流程 ─────────────────────────────

/// 跑一个任务（签名与 [`crate::batch::run_batch`] 的 `run_one` 一致）
pub fn run_job(
    job: &Job,
    opts: &BatchOptions,
    sink: &Arc<dyn EventSink>,
    cancel: &CancelToken,
) -> JobOutcome {
    let ctx = UnpackCtx::new(job.id, sink.clone()).with_cancel(cancel.clone());
    let started = Instant::now();

    // ① 识别 + 预览
    //    预览有两个用处：喂给「智能解压三规则」判断该不该多套一层目录；
    //    同时把 `PreviewReady` 事件发给外壳（界面 S3 的数据源）
    let prepared: Result<(Target, Option<crate::preview::Preview>)> = identify(&job.input, &ctx)
        .map(|target| {
            let pv = if opts.smart_dir && job.dest.is_none() {
                crate::preview::preview(&job.input, &ctx, opts.password.as_deref())
                    .ok()
                    .map(|(_, p)| p)
            } else {
                None
            };
            (target, pv)
        });

    let dest = match &prepared {
        Ok((_, pv)) => decide_dest(job, opts, pv.as_ref()),
        Err(_) => resolve_dest(job, opts),
    };
    let mut stats = JobStats {
        dest: Some(dest.clone()),
        ..Default::default()
    };

    // ①' 空间预检（体感最直接的护栏：14 GB 的包解到一半磁盘满，前面时间全白费）
    //      只有预估得出总大小时才拦人 —— 拿不准就放行，见 safety::check_space 的说明
    if !opts.force {
        let need = prepared
            .as_ref()
            .ok()
            .and_then(|(_, pv)| pv.as_ref())
            .map(|p| p.total_size)
            .unwrap_or(0);
        if let crate::safety::SpaceCheck::NotEnough { available, need } =
            crate::safety::check_space(&dest, need)
        {
            // 结构化参数交给外壳渲染（引擎不产出句子）
            let e = Error::new(ErrorCode::NotEnoughSpace)
                .with("need", need.to_string())
                .with("available", available.to_string())
                .with("dest", dest.display().to_string());
            return JobOutcome {
                id: job.id,
                input: job.input.clone(),
                outcome: Outcome::from_error(&e),
                stats,
                note: None,
            };
        }
    }

    // ② 解压
    let outcome = match prepared {
        Err(e) => {
            ctx.diagnostic(DiagnosticCode::PartialData, format!("identify: {e}"));
            Outcome::from_error(&e)
        }
        Ok((target, _pv)) => {
            let t0 = Instant::now();
            ctx.stage_started(Stage::Extract);
            let res = extract_target(job, opts, &ctx, &dest, &target, &mut stats);
            ctx.stage_finished(Stage::Extract, elapsed_ms(t0));
            outcome_from(res, &mut stats, &ctx)
        }
    };

    // 收尾：保证界面看到的进度是最终值
    stats.millis = elapsed_ms(started);
    ctx.force_progress(None);

    JobOutcome {
        id: job.id,
        input: job.input.clone(),
        outcome,
        stats,
        note: None,
    }
}

fn extract_target(
    job: &Job,
    opts: &BatchOptions,
    ctx: &UnpackCtx,
    dest: &Path,
    target: &Target,
    stats: &mut JobStats,
) -> Result<ExtractReport> {
    let password = match &opts.password {
        Some(p) => Some(p.clone()),
        None => probe_passwords(job, target, ctx),
    };

    match target {
        Target::Zip { base, len } => {
            let mut f = std::fs::File::open(&job.input)
                .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open: {e}")))?;
            let mut eopts = ExtractOptions::new(dest);
            eopts.password = password;
            eopts.selection = opts.selection.clone();
            eopts.limits = opts.limits.clone();
            let (rep, repair_note) =
                crate::extract::extract_zip_auto_ctx(&mut f, *base, *len, &eopts, ctx)?;
            if let Some(note) = repair_note {
                ctx.diagnostic(DiagnosticCode::OffsetCorrected, note);
                stats.flags.insert(ArchiveFlags::HEADERS_ERROR);
            }
            Ok(rep)
        }
        Target::ZipSplit { volumes } => {
            // 拼成一条逻辑流 → 找中央目录 → 解（和普通 ZIP 同一条路）
            let mut mf = crate::extract7z::MultiFile::open(volumes)?;
            let size = mf.total();
            let l = crate::layout::zip::locate(&mut mf, size).map_err(|e| {
                Error::new(ErrorCode::NotArchive)
                    .context(format!("分卷里的中央目录没找到（是不是缺了一块？）: {e}"))
            })?;
            let mut eopts = ExtractOptions::new(dest);
            eopts.password = password;
            eopts.selection = opts.selection.clone();
            eopts.limits = opts.limits.clone();
            let (rep, repair_note) =
                crate::extract::extract_zip_auto_ctx(&mut mf, l.base, l.data_len(), &eopts, ctx)?;
            if let Some(note) = repair_note {
                ctx.diagnostic(DiagnosticCode::OffsetCorrected, note);
                stats.flags.insert(ArchiveFlags::HEADERS_ERROR);
            }
            Ok(rep)
        }
        Target::SevenZ { volumes } => {
            let mut eopts = Extract7zOptions::new(dest);
            eopts.password = password;
            eopts.selection = opts.selection.clone();
            eopts.limits = opts.limits.clone();
            crate::extract7z::extract_7z_volumes_ctx(volumes, &eopts, ctx)
        }
        Target::Rar => {
            let mut eopts = ExtractRarOptions::new(dest);
            eopts.password = password;
            eopts.selection = opts.selection.clone();
            eopts.limits = opts.limits.clone();
            crate::extract_rar::extract_rar_file_ctx(&job.input, &eopts, ctx)
        }
        Target::Stream {
            format,
            is_tar,
            packed,
        } => {
            let mut eopts = ExtractStreamOptions::new(dest);
            eopts.packed_hint = *packed;
            eopts.selection = opts.selection.clone();
            eopts.limits = opts.limits.clone();
            eopts.out_name = extract_stream::strip_suffix(&job.input, *format)
                .map(|x| x.to_string_lossy().to_string());

            if *is_tar {
                let outer = if *format == ArchiveFormat::Tar {
                    None
                } else {
                    Some(*format)
                };
                let f = std::fs::File::open(&job.input).map_err(|e| {
                    Error::new(ErrorCode::PathNotFound).context(format!("open: {e}"))
                })?;
                extract_stream::extract_tar_ctx(f, outer, &eopts, ctx)
            } else {
                let f = std::fs::File::open(&job.input).map_err(|e| {
                    Error::new(ErrorCode::PathNotFound).context(format!("open: {e}"))
                })?;
                let out_name = eopts
                    .out_name
                    .clone()
                    .unwrap_or_else(|| "解压结果".to_string());
                let out_path = dest.join(&out_name);
                let n = extract_stream::decompress_stream_ctx(
                    *format,
                    f,
                    &out_path,
                    &opts.limits,
                    *packed,
                    ctx,
                )?;
                // 单流只有一个产物，包装成统一报告
                let mut rep = ExtractReport {
                    bytes_written: n,
                    ..Default::default()
                };
                rep.files.push(crate::extract::ExtractedFile {
                    name: out_name,
                    out_path,
                    size: n,
                    encrypted: false,
                });
                Ok(rep)
            }
        }
    }
}

/// 从旁边的「说明.txt」抓解压码
///
/// **在临时目录里静默试**，成功了才采用 —— 不往用户的输出目录里写垃圾
/// （三层审查 B2：「试密码会污染输出目录」）。
fn probe_passwords(job: &Job, target: &Target, ctx: &UnpackCtx) -> Option<String> {
    // 候选来源：① 用户的密码本（自己维护的常用解压码）② 旁边的「说明.txt」
    // 密码本放在前面 —— 那是用户**主动**记下来的，可信度比从说明里现抓的高
    let cfg = crate::config::全局配置();
    let mut hints: Vec<crate::password_hint::PasswordHint> = Vec::new();
    let 密码本 = cfg.passwords();
    if !密码本.is_empty() {
        let src = cfg.passwords_path();
        for pw in 密码本.into_iter().take(cfg.password.max_candidates) {
            hints.push(crate::password_hint::PasswordHint {
                password: pw,
                source: src.clone(),
                line: String::new(), // 密码本没有"原文行"可核对
                confidence: 200,     // 用户自己记的，比从说明里猜的更可信
            });
        }
    }
    if cfg.password.auto_from_notes {
        hints.extend(crate::password_hint::find_hints_near(&job.input));
    }
    if hints.is_empty() {
        return None;
    }

    // 只有 ZIP 能低成本地先试一下（只读加密条目的开头 4 KiB 就能判对错）；
    // 其它格式得**真解一遍**才知道密码对不对（7z/rar 每次候选 121 ms 起），
    // 所以这里只把**置信度最高的那一个**候选交出去。
    // 这是能力取舍，不是安全策略：在 7z/RAR 上逐个试候选 = 整包解一遍又一遍，
    // 用户会以为程序死了。
    let Target::Zip { base, len } = target else {
        let first = hints.first()?;
        ctx.diagnostic(
            DiagnosticCode::PasswordFromNotes,
            format!("candidate from {}", first.source.display()),
        );
        return Some(first.password.clone());
    };

    // 逐个候选**只读开头**验证（不解整个包：8 GB 的包完整试一次要几分钟）
    //
    // 试几个由配置决定（`max_candidates`，且被夹在硬上限之内）。
    //
    // ★ 安全边界（改这段之前先读 README「本工具不做的事」）：
    //   · 候选只来自「用户自己记的密码本」和「包旁边公开的说明文件」；
    //   · **只试不存** —— 抓到的密码用完就丢，绝不回填密码本。密码本是用户亲手
    //     维护的东西，工具替用户"收集密码"是另一类东西了；
    //   · 不做字典、不做掩码/增量爆破、不联网查库。
    let 试几个 = cfg
        .password
        .max_candidates
        .min(crate::config::候选数硬上限);
    for the_hint in hints.iter().take(试几个) {
        if ctx.is_cancelled() {
            break;
        }
        let ok = match std::fs::File::open(&job.input) {
            Ok(f) => crate::extract::verify_password_at(f, *base, *len, &the_hint.password),
            Err(_) => false,
        };
        if ok {
            ctx.diagnostic(
                DiagnosticCode::PasswordFromNotes,
                format!(
                    "{}: {}",
                    the_hint.source.display(),
                    the_hint.line.trim()
                ),
            );
            return Some(the_hint.password.clone());
        }
    }
    None
}

/// 把「解压结果」映射成「任务结论」
fn outcome_from(res: Result<ExtractReport>, stats: &mut JobStats, ctx: &UnpackCtx) -> Outcome {
    match res {
        Err(e) => Outcome::from_error(&e),
        Ok(rep) => {
            let progress = ctx.progress.view();
            stats.entries_done = rep.files.len() as u64;
            stats.entries_total = progress.total_files;
            stats.bytes_written = rep.bytes_written;
            stats.failed = rep
                .skipped
                .iter()
                .map(|(name, _why)| (name.clone(), ItemResult::DataError))
                .collect();
            if rep.budget_hit {
                stats.flags.insert(ArchiveFlags::DATA_ERROR);
            }

            if rep.cancelled {
                return Outcome::Cancelled {
                    done: rep.files.len() as u64,
                    total: stats.entries_total,
                };
            }
            if rep.files.is_empty() {
                // 一个都没解出来：说清是「缺密码」还是「数据坏了」
                let pw_issue = rep
                    .skipped
                    .iter()
                    .any(|(_, why)| why.contains("密码") || why.contains("解密"));
                return Outcome::Failed(if pw_issue {
                    ErrorCode::NeedPassword
                } else if rep.skipped.is_empty() {
                    ErrorCode::NotArchive
                } else {
                    ErrorCode::DataLost
                });
            }
            if rep.skipped.is_empty() {
                Outcome::Success
            } else {
                Outcome::Partial {
                    done: rep.files.len() as u64,
                    total: (rep.files.len() + rep.skipped.len()) as u64,
                }
            }
        }
    }
}

fn elapsed_ms(t0: Instant) -> u64 {
    t0.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ChannelSink, NullSink};
    use std::io::{Cursor, Write};

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_job_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// 造一个「伪装成 mp4 的 zip」：前面 4KB 假视频头，后面真 ZIP
    fn fake_mp4_zip(dir: &Path) -> PathBuf {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file("游戏/pak.dat", opt).unwrap();
        w.write_all(b"game data here").unwrap();
        w.start_file("说明.txt", opt).unwrap();
        w.write_all("解压码：hello".as_bytes()).unwrap();
        let zip_data = w.finish().unwrap().into_inner();

        let path = dir.join("伪装包.mp4");
        let mut out = std::fs::File::create(&path).unwrap();
        let head = b"\x00\x00\x00\x18ftypmp42";
        out.write_all(head).unwrap();
        out.write_all(&vec![0xAAu8; 4096 - head.len()]).unwrap();
        out.write_all(&zip_data).unwrap();
        drop(out);
        path
    }

    #[test]
    fn 能认出伪装成_mp4_的_zip() {
        let dir = scratch("identify");
        let p = fake_mp4_zip(&dir);
        let ctx = UnpackCtx::quiet(1);
        let t = identify(&p, &ctx).unwrap();
        match t {
            Target::Zip { base, len } => {
                assert_eq!(base, 4096, "ZIP 起点应当是假 mp4 头的长度");
                assert!(len > 0);
            }
            other => panic!("应当识别成 ZIP，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_分卷能认出并拼接解开() {
        let dir = scratch("zipvol");

        // 造一个普通 ZIP，再按字节切成两块 —— 这正是 `zip -s` 的排法：
        // 数据流被切开，中央目录留在最后一块的尾部，偏移是相对整条流的
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        // 数据要「不太能压」：`'x'*5000` 这种压缩比能到 100 倍，
        // 会被压缩炸弹防护拦下（防护是对的，别改安全限值来迁就测试）
        let blob: Vec<u8> = (0..5000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        w.start_file("游戏/a.dat", opt).unwrap();
        w.write_all(&blob).unwrap();
        w.start_file("说明.txt", opt).unwrap();
        w.write_all("里面有东西".as_bytes()).unwrap();
        let bytes = w.finish().unwrap().into_inner();

        let cut = bytes.len() / 2;
        let z01 = dir.join("包.z01");
        let last = dir.join("包.zip");
        std::fs::write(&z01, &bytes[..cut]).unwrap();
        std::fs::write(&last, &bytes[cut..]).unwrap();

        let ctx = UnpackCtx::quiet(1);
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);

        // 对照组：**先确认不切分时这份 ZIP 自己能解开**
        // （不然分不清是「分卷逻辑坏了」还是「测试造的包本身有问题」）
        let whole = dir.join("整包.zip");
        std::fs::write(&whole, &bytes).unwrap();
        let out_w = scratch("zipvol_whole");
        let opts_w = BatchOptions {
            dest: Some(out_w.clone()),
            smart_dir: false,
            ..Default::default()
        };
        let o_w = run_job(
            &Job::new(1, whole.clone(), 0),
            &opts_w,
            &sink,
            &CancelToken::new(),
        );
        assert_eq!(
            o_w.outcome, Outcome::Success,
            "对照组（不切分）就失败了 → 是造数据的问题，不是分卷逻辑: {:?}",
            o_w.stats
        );

        // ① 从 .z01 入口认出来
        let t = identify(&z01, &ctx).unwrap();
        assert!(
            matches!(t, Target::ZipSplit { .. }),
            "从 .z01 应当认成分卷，实际 {t:?}"
        );
        // ② 从收尾块 .zip 入口也认出来（下载点最后一块很常见）
        let t2 = identify(&last, &ctx).unwrap();
        assert!(
            matches!(t2, Target::ZipSplit { .. }),
            "从 .zip 应当认成分卷，实际 {t2:?}"
        );

        // ③ 真解一遍：拼接后的内容和原 ZIP 一致
        let out = scratch("zipvol_out");
        let opts = BatchOptions {
            dest: Some(out.clone()),
            smart_dir: false,
            ..Default::default()
        };
        let outcome = run_job(&Job::new(1, z01.clone(), 0), &opts, &sink, &CancelToken::new());
        assert_eq!(outcome.outcome, Outcome::Success, "{:?}", outcome.stats);
        assert!(out.join("游戏/a.dat").exists(), "分卷里的文件应当解出来");
        assert_eq!(
            std::fs::read(out.join("说明.txt")).unwrap(),
            "里面有东西".as_bytes()
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn 认不出来的文件报不是压缩包() {
        let dir = scratch("nope");
        let p = dir.join("随便.bin");
        std::fs::write(&p, vec![7u8; 1024]).unwrap();
        let ctx = UnpackCtx::quiet(1);
        let e = identify(&p, &ctx).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotArchive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 跑完一个任务得到成功结论与统计() {
        let dir = scratch("run");
        let p = fake_mp4_zip(&dir);
        let out = scratch("run_out");

        // 注意：ChannelSink::new 返回的就是 Arc<ChannelSink>，别再套一层 Arc
        let sink: Arc<dyn EventSink> = ChannelSink::new(512);
        let opts = BatchOptions {
            dest: Some(out.clone()),
            // 这个用例测「基本解压」，关掉智能目录，保持断言直白
            smart_dir: false,
            ..Default::default()
        };
        let job = Job::new(1, p.clone(), 0);
        let outcome = run_job(&job, &opts, &sink, &CancelToken::new());

        assert_eq!(outcome.outcome, Outcome::Success);
        assert_eq!(outcome.stats.entries_done, 2);
        assert!(outcome.stats.bytes_written > 0);
        assert!(out.join("游戏/pak.dat").exists(), "文件应当真的落盘");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn 智能解压三规则在真实任务上生效() {
        let dir = scratch("smart");
        let p = fake_mp4_zip(&dir);
        let out = scratch("smart_out");

        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        // 默认 smart_dir = true：这个包有两个顶层条目（「游戏」和「说明.txt」）
        // → 不是单一顶层目录 → 自动建同名子目录（包名 = 去掉 .mp4 后的名字）
        let opts = BatchOptions {
            dest: Some(out.clone()),
            ..Default::default()
        };
        let outcome = run_job(&Job::new(1, p.clone(), 0), &opts, &sink, &CancelToken::new());

        assert_eq!(outcome.outcome, Outcome::Success);
        let dest = outcome.stats.dest.clone().unwrap();
        assert_ne!(dest, out, "散文件应当解到子目录里，而不是直接铺在 -o 目录");
        assert!(
            dest.join("游戏/pak.dat").exists(),
            "实际落盘位置：{:?}",
            dest
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn 挑选只解勾中的条目() {
        let dir = scratch("select");
        let p = fake_mp4_zip(&dir);
        let out = scratch("select_out");

        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let opts = BatchOptions {
            dest: Some(out.clone()),
            selection: crate::select::Selection::matching("*.txt"),
            smart_dir: false,
            ..Default::default()
        };
        let outcome = run_job(&Job::new(1, p.clone(), 0), &opts, &sink, &CancelToken::new());

        assert_eq!(outcome.outcome, Outcome::Success);
        assert!(out.join("说明.txt").exists());
        assert!(!out.join("游戏/pak.dat").exists(), "没勾中的不该解出来");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn 取消的任务结论是取消而不是失败() {
        let dir = scratch("cancel");
        let p = fake_mp4_zip(&dir);
        let out = scratch("cancel_out");

        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let cancel = CancelToken::new();
        cancel.cancel();
        let opts = BatchOptions {
            dest: Some(out.clone()),
            ..Default::default()
        };
        let outcome = run_job(&Job::new(1, p.clone(), 0), &opts, &sink, &cancel);

        assert!(!outcome.outcome.is_failure(), "取消不是失败");
        assert!(matches!(outcome.outcome, Outcome::Cancelled { .. }));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn 输出目录默认用去掉外壳扩展名的同名文件夹() {
        let opts = BatchOptions::default();
        let j = Job::new(1, "/下载/示例视频.mp4", 0);
        assert_eq!(resolve_dest(&j, &opts), PathBuf::from("示例视频"));
        let j2 = Job::new(2, "/下载/包.mp4.zip", 0);
        assert_eq!(resolve_dest(&j2, &opts), PathBuf::from("包"));

        let with_dest = BatchOptions {
            dest: Some(PathBuf::from("/tmp/out")),
            ..Default::default()
        };
        assert_eq!(resolve_dest(&j, &with_dest), PathBuf::from("/tmp/out"));

        // 任务自带的目录优先（批量时 CLI 会给每个包分一个子目录，免得互相打架）
        let j3 = Job::new(3, "/下载/x.mp4", 0).with_dest("/tmp/out/x");
        assert_eq!(resolve_dest(&j3, &with_dest), PathBuf::from("/tmp/out/x"));
    }

    #[test]
    fn 分卷名能拆出前缀与序号() {
        assert_eq!(
            split_volume_name("示例包.7z.001"),
            Some(("示例包.7z.".to_string(), 1, 3))
        );
        assert_eq!(split_volume_name("x12"), Some(("x".to_string(), 12, 2)));
        // 中文名不能崩（v0.4 的旧实现会 panic —— 按字节切字符串）
        assert_eq!(split_volume_name("没有数字"), None);
        assert_eq!(split_volume_name("示例视频"), None);
        assert_eq!(split_volume_name("魔女2"), Some(("魔女".to_string(), 2, 1)));
    }

    #[test]
    fn 中文名的文件也能安全识别() {
        let dir = scratch("cn");
        let p = dir.join("示例视频");
        std::fs::write(&p, vec![7u8; 1024]).unwrap();
        let ctx = UnpackCtx::quiet(1);
        // 不该 panic，只该报「不是压缩包」
        let e = identify(&p, &ctx).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotArchive);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
