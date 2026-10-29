//! 7z 解压 —— 含**分卷虚接**
//!
//! # 为什么单独一个模块
//! 7z 和 ZIP 的定位方式完全不同：
//! - ZIP 有中央目录，我们靠自己算偏移（见 [`crate::layout::zip`]）
//! - 7z 的目录在**尾部**的 NextHeader，而且分卷是**裸字节切开**（`.002` 连签名都没有）
//!
//! 所以流程是：分卷首尾拼成一个逻辑流 → 交给 `sevenz-rust2` 解码。
//!
//! # 关键实现（对齐调研结论）
//! - `7z` 总长 = `32 + NextHeaderOffset + NextHeaderSize`，且头 32 字节自带 CRC32
//!   （我们已在 [`crate::scan`] 里用它做高置信度识别 → 几乎零误报）
//! - 分卷只是裸切，**直接拼接即可**，不需要任何特殊逻辑
//! - 密码由库处理（内部转 UTF-16LE，符合 7z 规范）；中文密码无需用户配置
//!
//! # 与 ZIP 路径的共同点
//! 安全落盘、限额、同名策略、结果分级，全部复用同一套（[`crate::safety`] + 报告结构）。

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, ItemResult, Result};
use crate::safety::{self, ExtractBudget, Limits};
use crate::select::Selection;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 多个文件首尾相接，当成**一个逻辑流**（7z 分卷用）
///
/// 7z 的分卷是"裸字节切分"，所以拼接语义就是简单串接。
pub struct MultiFile {
    files: Vec<File>,
    /// 每个分卷的长度
    lens: Vec<u64>,
    /// 每个分卷在逻辑流中的起始偏移
    starts: Vec<u64>,
    /// 逻辑流总长
    total: u64,
    /// 当前逻辑位置
    pos: u64,
}

impl MultiFile {
    /// 按顺序打开多个分卷（顺序就是它们拼接的顺序）
    pub fn open(paths: &[PathBuf]) -> Result<Self> {
        if paths.is_empty() {
            return Err(Error::new(ErrorCode::Internal).with("reason", "没有分卷文件"));
        }
        let mut files = Vec::new();
        let mut lens = Vec::new();
        let mut starts = Vec::new();
        let mut acc = 0u64;
        for p in paths {
            let f = File::open(p).map_err(|e| {
                Error::new(ErrorCode::Internal).context(format!("打开分卷 {p:?} 失败: {e}"))
            })?;
            let n = f
                .metadata()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("读分卷信息: {e}")))?
                .len();
            starts.push(acc);
            lens.push(n);
            files.push(f);
            acc += n;
        }
        Ok(Self {
            files,
            lens,
            starts,
            total: acc,
            pos: 0,
        })
    }

    /// 逻辑流总长
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 分卷个数
    pub fn volumes(&self) -> usize {
        self.files.len()
    }
}

impl Read for MultiFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.total || buf.is_empty() {
            return Ok(0);
        }
        // 找到 pos 落在哪个分卷
        let idx = match self.starts.binary_search(&self.pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let within = self.pos - self.starts[idx];
        let avail = self.lens[idx] - within;
        let want = buf.len().min(avail as usize);
        self.files[idx].seek(SeekFrom::Start(within))?;
        let n = self.files[idx].read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for MultiFile {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let target: i128 = match from {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(p) => self.total as i128 + p as i128,
            SeekFrom::Current(p) => self.pos as i128 + p as i128,
        };
        if target < 0 || target > self.total as i128 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "MultiFile: seek 越界",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// 7z 解压参数
#[derive(Debug, Clone)]
pub struct Extract7zOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 密码（`None` = 无密码）
    pub password: Option<String>,
    /// 同名策略（复用 ZIP 那套语义）
    pub overwrite: crate::extract::Overwrite,
    /// 挑哪些条目（默认全要）
    pub selection: Selection,
    /// 安全限值
    pub limits: Limits,
}

impl Extract7zOptions {
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

/// 从多个分卷解出 7z（只有一卷时传一个路径即可）
pub fn extract_7z_volumes(
    volumes: &[PathBuf],
    opts: &Extract7zOptions,
) -> Result<crate::extract::ExtractReport> {
    extract_7z_volumes_ctx(volumes, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_7z_volumes`]，但带上事件 / 进度 / 取消上下文
pub fn extract_7z_volumes_ctx(
    volumes: &[PathBuf],
    opts: &Extract7zOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    let multi = MultiFile::open(volumes)?;
    // 用**分卷实际总大小**作为解压炸弹防护的基准
    // （7z 没有 ZIP 那样的"中央目录先验"，只能拿压缩后的体积当分母）
    let packed_total = multi.total();
    extract_7z_with_hint_ctx(multi, packed_total, opts, ctx)
}

/// 从任意流解出 7z（`packed_total` 传压缩后的字节数，用于炸弹防护）
pub fn extract_7z<R: Read + Seek>(
    src: R,
    opts: &Extract7zOptions,
) -> Result<crate::extract::ExtractReport> {
    extract_7z_ctx(src, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_7z`]，但带上事件 / 进度 / 取消上下文
pub fn extract_7z_ctx<R: Read + Seek>(
    src: R,
    opts: &Extract7zOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    extract_7z_with_hint_ctx(src, 1, opts, ctx)
}

/// 从**一条流**解 7z —— 统一接口（[`crate::unpack`]）用的入口
///
/// 与 [`extract_7z_volumes_ctx`] 的唯一区别是「谁是数据源」：那边收分卷路径列表、
/// 自己拼 `MultiFile`；这边收**一条已经拼好的流**。统一接口只认一条流
/// （`xadmaster.md` ①-01「handle 是唯一 I/O 抽象」），所以「多卷拼接」留在编排层、
/// 「解码」留在 handler —— 分工清楚，handler 不必知道卷名规则。
///
/// `packed_total` 必须是**真实的压缩后总大小**（炸弹防护的分母，这个坑踩过两次）。
pub fn extract_7z_stream_ctx<R: Read + Seek>(
    src: R,
    packed_total: u64,
    opts: &Extract7zOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    extract_7z_with_hint_ctx(src, packed_total, opts, ctx)
}

/// LZMA2 字典大小（coder properties[0] 的官方算法）
fn lzma2_dict_size(prop: u8) -> u64 {
    if prop > 40 {
        return 0;
    }
    if prop == 40 {
        return 4 * 1024 * 1024 * 1024;
    }
    (2 | (u64::from(prop) & 1)) << (prop / 2 + 11)
}

/// 从 7z 头估算解码峰值内存：LZMA / LZMA2 的字典基本就是解码器要吃的内存
fn 估算解码内存(archive: &sevenz_rust2::Archive) -> Option<u64> {
    let mut max_dict: u64 = 0;
    for block in &archive.blocks {
        for coder in &block.coders {
            let id = coder.encoder_method_id();
            let props = coder.properties();
            let dict = if id == [0x21u8] && !props.is_empty() {
                // LZMA2
                lzma2_dict_size(props[0])
            } else if id == [0x03u8, 0x01, 0x01] && props.len() >= 5 {
                // LZMA：props[1..5] 是小端 u32 字典大小
                u32::from_le_bytes([props[1], props[2], props[3], props[4]]) as u64
            } else {
                0
            };
            max_dict = max_dict.max(dict);
        }
    }
    (max_dict > 0).then_some(max_dict)
}

/// 解压前的内存预检
///
/// 7z 用的是**流式**解压，正常路径拿不到字典大小就先开跑了；大字典（1 GB+）的包
/// 会被内核 OOM 直接杀掉整个引擎，界面只看到「引擎断开」，比报错难懂得多。
/// 这里先单独读一遍文件头（只读头，不碰数据），读完把读指针退回去。
fn 内存预检<R: std::io::Read + std::io::Seek>(
    src: &mut R,
    password: &sevenz_rust2::Password,
) -> Result<()> {
    let 起点 = src.stream_position().unwrap_or(0);
    let 需要 = match sevenz_rust2::ArchiveReader::new(&mut *src, password.clone()) {
        Ok(reader) => 估算解码内存(reader.archive()),
        Err(_) => None, // 读头失败就让后面的正常流程去报真正的错
    };
    let _ = src.seek(std::io::SeekFrom::Start(起点));
    let (Some(need), Some(avail_mib)) = (需要, crate::hardware::available_mem_mib()) else {
        return Ok(());
    };
    let avail = avail_mib * 1024 * 1024;
    // 留 256 MB 给系统和其他程序，别把机器榨干
    let 预留 = 256 * 1024 * 1024;
    if need + 预留 > avail {
        return Err(Error::new(ErrorCode::NotEnoughSpace)
            .with("kind", "memory")
            .with("need_mib", (need / 1048576).to_string())
            .with("available_mib", avail_mib.to_string())
            .context(format!(
                "这个 7z 的压缩字典约 {} MB，解压至少要 {} MB 空闲内存，现在只有 {} MB —— 先关几个程序再试",
                need / 1048576,
                need / 1048576 + 256,
                avail_mib
            )));
    }
    Ok(())
}

/// 内部实现（带上下文）：`packed_total` 是"压缩包多大"，用于算压缩比
fn extract_7z_with_hint_ctx<R: Read + Seek>(
    mut src: R,
    packed_total: u64,
    opts: &Extract7zOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    use std::cell::Cell;
    use std::io::Write;

    // 只测不写：不建输出目录
    if !ctx.verify_only {
        std::fs::create_dir_all(&opts.dest)
            .map_err(|e| { let m = format!("建输出目录失败: {e}"); Error::from(e).context(m) })?;
    }

    let password = match &opts.password {
        Some(p) => sevenz_rust2::Password::new(p),
        None => sevenz_rust2::Password::empty(),
    };

    // 解压前先估算内存（只读头），不够就提前说清楚，别被 OOM 打死
    内存预检(&mut src, &password)?;

    let mut report = crate::extract::ExtractReport::default();
    // 以"压缩包体积"为分母算压缩比，防解压炸弹（比率为 0 表示不做比率检查）
    let mut budget = ExtractBudget::new(opts.limits.clone(), packed_total.max(1));

    let dest = opts.dest.clone();
    let overwrite = opts.overwrite;
    let selection = &opts.selection;
    // 7z 的回调不给条目序号，我们自己数（预览到解压的挑选锚点就靠它）
    let entry_index = Cell::new(0u64);

    let res = sevenz_rust2::decompress_with_extract_fn_and_password(
        src,
        &dest,
        password,
        |entry, reader, dest_path| {
            // 7z 的回调不给条目序号，我们自己数（**1-based**，与预览显示的号一致）
            let idx = entry_index.get() + 1;
            entry_index.set(idx);
            let name = entry.name().to_string();

            // 用户取消：返回 false 让库停下；已解出的文件保留
            if ctx.is_cancelled() {
                report.cancelled = true;
                return Ok(false);
            }

            if entry.is_directory() {
                // 断点续传：上次已经建过 → 跳
                if ctx.resume_skip(&name, dest_path, crate::resume::条目种类::目录) {
                    ctx.entry_done(idx, &name, ItemResult::Ok, 0);
                    return Ok(true);
                }
                if !ctx.verify_only && let Err(e) = std::fs::create_dir_all(dest_path) {
                    report
                        .skipped
                        .push((name.clone(), format!("建目录失败: {e}")));
                }
                ctx.resume_record(&name, dest_path, crate::resume::条目种类::目录);
                ctx.entry_done(idx, &name, ItemResult::Ok, 0);
                return Ok(true);
            }

            // 挑选：没被勾中的条目直接跳过（不写盘、不算错）
            if !selection.matches(idx, &name, false) {
                return Ok(true);
            }

            // 我们的安全护栏：路径穿越（库已挡一层，这里再走我们的规则）
            let safe_path = match safety::resolve_safe(&dest, &name) {
                Ok(p) => p,
                Err(e) => {
                    report
                        .skipped
                        .push((name.clone(), format!("路径不安全被拒: {e}")));
                    return Ok(true);
                }
            };

            // 同名策略
            let mut out_path = safe_path;

            // 断点续传：上次完整解过这条（磁盘大小也对）→ 跳过写盘
            //
            // 注意这一步**必须**在同名处理之前：否则「同名跳过」策略会把续传要跳的
            // 文件当成「已存在冲突」记进跳过列表，用户会看到一堆莫名其妙的「已跳过」。
            //
            // 7z 固实压缩无法真的跳过解码（后面的条目位置依赖前面的数据流），
            // 这里省掉的是写盘那一步；非固实包则连解码都省了。
            if ctx.resume_skip(&name, &out_path, crate::resume::条目种类::文件) {
                let 大小 = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
                ctx.entry_done(idx, &name, ItemResult::Ok, 0);
                report.files.push(crate::extract::ExtractedFile {
                    name,
                    out_path,
                    size: 大小,
                    encrypted: opts.password.is_some(),
                });
                return Ok(true);
            }

            if out_path.exists() {
                match ctx.同名策略(overwrite) {
                    crate::extract::Overwrite::Skip => {
                        report
                            .skipped
                            .push((name.clone(), "已存在，按策略跳过".into()));
                        return Ok(true);
                    }
                    crate::extract::Overwrite::Replace => {}
                    crate::extract::Overwrite::Rename => {
                        out_path = unique_path(&out_path);
                    }
                }
            }
            if !ctx.verify_only
                && let Some(parent) = out_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                report
                    .skipped
                    .push((name.clone(), format!("建父目录失败: {e}")));
                return Ok(true);
            }

            if safety::should_warn_executable(&opts.limits, &name) {
                // 走**事件**而不是往内部列表里塞字符串：
            // ① 事件外壳才看得见（否则这条提醒等于没发）
            // ② 文案由外壳查语言包渲染（引擎不产句子）
            ctx.diagnostic(crate::event::DiagnosticCode::RiskyEntry, name.clone());
            }

            // 注：7z 条目里其实记着修改时间（`entry.last_modified_date`），但
            // `sevenz-rust2` 把它的原始值定义成 `pub(crate)` —— **外部取不出来**，
            // 所以 7z 解出来的文件暂时保留不了原始时间戳（ZIP / TAR 已支持）。
            // 等这个库暴露访问器（或换库）再补，不要用"格式化再解析"那种脏办法。

            // T9-P1/T8-P6：先写 .yshjy-part，写完再原子改名
            let mut part = match crate::io::PartFile::create_with(&out_path, ctx.verify_only) {
                Ok(f) => f,
                Err(e) => {
                    report.note_io_error(&e);
                    report
                        .skipped
                        .push((name.clone(), format!("创建失败: {e}")));
                    return Ok(true);
                }
            };

            let mut buf = vec![0u8; 64 * 1024];
            let mut written = 0u64;
            // 条目在 7z 头里声明的大小：给输出提前封顶（同 libarchive 的
            // `entry_bytes_declared`，见 refs/tech/libarchive.md §4）。
            // 声明 0 的条目（空文件 / 目录）不封顶。
            let declared = entry.size();
            loop {
                // 取消检查下沉到数据块级：单个大文件也能及时停
                if ctx.is_cancelled() {
                    drop(part);
                    report.cancelled = true;
                    ctx.entry_done(idx, &name, ItemResult::Skipped, 0);
                    return Ok(false);
                }
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // 少写：声明了却没读够 —— 数据被截断，这一条算坏
                        if declared > 0 && written != declared {
                            report.skipped.push((
                                name.clone(),
                                format!(
                                    "条目声明 {declared} 字节，实际读出 {written} 字节 —— 数据被截断"
                                ),
                            ));
                            drop(part);
                            ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                            return Ok(true);
                        }
                        break;
                    }
                    Ok(n) => {
                        // 只接受「声明范围内」的字节数（多出来的一个字节都不落盘）
                        let 允许 = if declared > 0 {
                            declared.saturating_sub(written).min(n as u64) as usize
                        } else {
                            n
                        };
                        if 允许 > 0 {
                            if let Err(e) = budget.record(允许 as u64) {
                                // 跳过 / 取消：按取消收尾（不是"数据坏"）
                                if e.is_cancelled() {
                                    drop(part);
                                    report.cancelled = true;
                                    ctx.entry_done(idx, &name, ItemResult::Skipped, 0);
                                    return Ok(false);
                                }
                                report.budget_hit = true;
                                report
                                    .skipped
                                    .push((name.clone(), format!("安全限额中止: {e}")));
                                drop(part);
                                ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                                return Ok(false); // 停止
                            }
                            if let Err(e) = part.as_file().write_all(&buf[..允许]) {
                                report.note_io_error(&e);
                                report
                                    .skipped
                                    .push((name.clone(), format!("写盘失败: {e}")));
                                drop(part);
                                ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                                return Ok(true);
                            }
                            written += 允许 as u64;
                            ctx.advance(允许 as u64, Some(&name));
                        }
                        if 允许 < n {
                            report.skipped.push((
                                name.clone(),
                                format!(
                                    "条目声明 {declared} 字节，实际数据更多 —— 已丢弃超出的 {} 字节",
                                    n - 允许
                                ),
                            ));
                            drop(part);
                            ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                            return Ok(true);
                        }
                    }
                    Err(e) => {
                        report
                            .skipped
                            .push((name.clone(), format!("读取失败: {e}")));
                        drop(part);
                        ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                        return Ok(true);
                    }
                }
            }

            // 原子落地：改名成功之前，正式文件名还不存在
            if let Err(e) = part.finish() {
                report
                    .skipped
                    .push((name.clone(), format!("落盘改名失败: {e}")));
                ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                return Ok(true);
            }
            report.bytes_written += written;
            // 断点续传：原子改名成功才算完整落地
            ctx.resume_record(&name, &out_path, crate::resume::条目种类::文件);
            ctx.entry_done(idx, &name, ItemResult::Ok, written);
            report.files.push(crate::extract::ExtractedFile {
                name,
                out_path,
                size: written,
                encrypted: !opts.password.is_none(),
            });
            Ok(true)
        },
    );

    if let Err(e) = res {
        // 用户取消：不是错误，如实把「已解出多少」交回去（台账 §5.8）
        if report.cancelled {
            ctx.force_progress(None);
            return Ok(report);
        }
        // 一个文件都没解出来 → 当作整体失败（密码错 / 格式不支持 / 数据坏）
        if report.files.is_empty() {
            let msg = e.to_string();
            let is_pw = msg.to_lowercase().contains("password")
                || msg.contains("密码")
                || msg.to_lowercase().contains("crypto");
            let code = if is_pw {
                // 有密码参数说明是密码不对，没有就是缺密码
                if opts.password.is_some() {
                    ErrorCode::WrongPassword
                } else {
                    ErrorCode::NeedPassword
                }
            } else {
                ErrorCode::NotArchive
            };
            return Err(Error::new(code).context(format!("7z 解压失败: {msg}")));
        }
        // 部分成功：记下来但不算失败
        report
            .warnings
            .push(format!("解压过程报错（已解出部分）: {e}"));
    }

    Ok(report)
}

/// 同名时生成不冲突的名字（与 ZIP 路径保持一致的语义）
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let candidate = match ext {
            Some(e) => parent.join(format!("{stem} ({i}).{e}")),
            None => parent.join(format!("{stem} ({i})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    #[test]
    fn lzma2字典大小_官方公式() {
        // 官方算法：(2 | (p & 1)) << (p / 2 + 11)
        assert_eq!(super::lzma2_dict_size(0), 4 << 10); // 4 KB
        assert_eq!(super::lzma2_dict_size(16), 1 << 20); // 1 MB
        assert_eq!(super::lzma2_dict_size(32), 256 << 20); // 256 MB
        assert_eq!(super::lzma2_dict_size(40), 4u64 << 30); // 4 GB
        assert_eq!(super::lzma2_dict_size(41), 0); // 非法值
    }

    use super::*;
    use std::io::Cursor;

    #[test]
    fn 挑选与进度在_7z_路径上也生效() {
        use crate::ctx::UnpackCtx;
        use crate::event::{ChannelSink, Event};

        let dir = scratch("ctx");
        let data = make_7z(&dir, &[("a.txt", b"aaa"), ("b.txt", b"bbb")]);
        let dest = scratch("ctx_out");
        let sink = ChannelSink::new(256);
        let ctx = UnpackCtx::quiet(1).with_sink(sink.clone());

        let mut opts = Extract7zOptions::new(&dest);
        opts.selection = Selection::matching("a.txt");

        let rep = extract_7z_ctx(Cursor::new(data), &opts, &ctx).unwrap();
        assert_eq!(rep.files.len(), 1, "只该解出 a.txt：{:?}", rep.files);
        assert_eq!(rep.files[0].name, "a.txt");
        let evs = sink.drain();
        assert!(
            evs.iter()
                .any(|e| matches!(e, Event::EntryFinished { .. })),
            "应当有条目级事件"
        );
        assert!(ctx.progress.view().done_bytes > 0, "应当累计了字节数");
    }

    #[test]
    fn 一上来就取消时_7z_不报错也不解东西() {
        use crate::ctx::UnpackCtx;
        use crate::progress::CancelToken;

        let dir = scratch("cancel");
        let data = make_7z(&dir, &[("a.txt", b"aaa")]);
        let dest = scratch("cancel_out");
        let cancel = CancelToken::new();
        cancel.cancel();
        let ctx = UnpackCtx::quiet(1).with_cancel(cancel);

        let rep = extract_7z_ctx(Cursor::new(data), &Extract7zOptions::new(&dest), &ctx).unwrap();
        assert!(rep.cancelled, "应当标记为已取消");
        assert!(rep.files.is_empty());
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_7z_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// 用库自带的写侧造一个**无密码** 7z（测试用；加密包用 7z 命令行造样本）
    fn make_7z(dir: &Path, files: &[(&str, &[u8])]) -> Vec<u8> {
        use sevenz_rust2::*;
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = ArchiveWriter::new(&mut cursor).unwrap();
            for (name, data) in files {
                let entry = ArchiveEntry::new_file(name);
                w.push_archive_entry(entry, Some(Cursor::new(data.to_vec())))
                    .unwrap();
            }
            w.finish().unwrap();
        }
        let _ = dir;
        cursor.into_inner()
    }

    #[test]
    fn multifile_虚接正确() {
        let a = Cursor::new(b"AAAA".to_vec());
        let b = Cursor::new(b"BBBB".to_vec());
        drop((a, b));
        // 用真实文件测（MultiFile 需要 File）
        let dir = scratch("multi");
        let p1 = dir.join("a.bin");
        let p2 = dir.join("b.bin");
        std::fs::write(&p1, b"AAAA").unwrap();
        std::fs::write(&p2, b"BBBB").unwrap();
        let mut m = MultiFile::open(&[p1, p2]).unwrap();
        assert_eq!(m.total(), 8);
        assert_eq!(m.volumes(), 2);
        let mut all = Vec::new();
        m.read_to_end(&mut all).unwrap();
        assert_eq!(&all, b"AAAABBBB");

        // 跨界 seek + 读
        m.seek(SeekFrom::Start(2)).unwrap();
        let mut b4 = [0u8; 4];
        m.read_exact(&mut b4).unwrap();
        assert_eq!(&b4, b"AABB");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 单卷_7z_能解出() {
        let dir = scratch("one");
        let data = make_7z(&dir, &[("hello.txt", b"hello 7z world")]);
        let f = dir.join("t.7z");
        std::fs::write(&f, &data).unwrap();

        let dest = scratch("one_out");
        let opts = Extract7zOptions::new(&dest);
        let rep = extract_7z_volumes(std::slice::from_ref(&f), &opts).unwrap();
        assert_eq!(rep.files.len(), 1, "应解出 1 个: {:?}", rep.skipped);
        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "hello 7z world"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn 带密码的7z能解() {
        // 用 7z 命令行造的样本（库的写侧没有加密接口）
        let vol = PathBuf::from("tests/data/sevenz_encrypted.7z");
        if !vol.exists() {
            eprintln!("样本不存在，跳过");
            return;
        }
        let dest = scratch("pw_out");
        let mut opts = Extract7zOptions::new(&dest);

        // 没给密码 → 应当失败或一个都没解出
        let no_pw = extract_7z_volumes(std::slice::from_ref(&vol), &opts);
        assert!(
            no_pw.is_err() || no_pw.as_ref().map(|r| r.files.is_empty()).unwrap_or(false),
            "没密码不该成功解出"
        );

        // 给对密码 → 全解出，内容对
        opts.password = Some("secret7z".into());
        let rep = extract_7z_volumes(&[vol], &opts).unwrap();
        assert_eq!(rep.files.len(), 2, "给对密码应解出 2 个: {:?}", rep.skipped);
        assert_eq!(
            std::fs::read_to_string(dest.join("secret.txt"))
                .unwrap()
                .trim(),
            "hello 7z encrypted content"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn 分卷被切成两半也能解() {
        let dir = scratch("split");
        let data = make_7z(
            &dir,
            &[
                ("a.txt", b"split volume test content"),
                ("b.bin", &vec![9u8; 5000]),
            ],
        );
        // 裸切成两半（7z 分卷就是这个语义）
        let mid = data.len() / 2;
        let p1 = dir.join("x.7z.001");
        let p2 = dir.join("x.7z.002");
        std::fs::write(&p1, &data[..mid]).unwrap();
        std::fs::write(&p2, &data[mid..]).unwrap();

        let dest = scratch("split_out");
        let opts = Extract7zOptions::new(&dest);
        let rep = extract_7z_volumes(&[p1, p2], &opts).unwrap();
        assert_eq!(rep.files.len(), 2, "分卷应解出 2 个: {:?}", rep.skipped);
        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt")).unwrap(),
            "split volume test content"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dest);
    }
}
