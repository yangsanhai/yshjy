//! 单流压缩格式 + TAR 解压
//!
//! # 覆盖范围
//! | 格式 | 签名 | 实现 |
//! |---|---|---|
//! | GZIP | `1f 8b` | `flate2`（我们已有） |
//! | XZ | `fd 37 7a 58 5a 00` | `lzma-rs`（纯 Rust） |
//! | ZSTD | `28 b5 2f fd` | `ruzstd`（纯 Rust） |
//! | BZIP2 | `42 5a 68`（BZh） | `bzip2-rs`（纯 Rust） |
//! | TAR | 偏移 257 处 `ustar` | `tar` crate |
//!
//! # 与容器格式的区别
//! ZIP / 7z / RAR 是**容器**（一个文件里装很多条目）；
//! GZ / XZ / ZST / BZ2 是**单流**（压的是"一个字节流"，本身不含文件名，
//! 名字来自外层文件名，例如 `photo.jpg.gz` → `photo.jpg`）。
//!
//! 所以 `.tar.gz` 这种是**两层**：先解压缩流得到 `.tar`，再把 tar 展开成目录树。
//! 本模块会自动识别并剥这两层。

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, ItemResult, Result};
use crate::layout::ArchiveFormat;
use crate::safety::{self, Limits};
use crate::select::Selection;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 单流格式的检测结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    /// 外层压缩格式
    pub format: ArchiveFormat,
    /// 是不是 tar 家族（`.tar.gz` 这类）
    pub is_tar: bool,
}

/// 看头部字节判断是什么格式
///
/// 比 [`crate::scan`] 更严格：这里只认"**文件开头**就是签名"的情况
/// （单流格式不可能藏在中间 —— 中间藏着的是容器，归 scan 管）。
pub fn detect(head: &[u8]) -> Option<Detected> {
    if head.len() < 4 {
        return None;
    }
    let fmt = if head.starts_with(b"\x1f\x8b\x08") {
        Some(ArchiveFormat::Gzip)
    } else if head.starts_with(b"\xfd7zXZ\x00") {
        Some(ArchiveFormat::Xz)
    } else if head.starts_with(b"\x28\xb5\x2f\xfd") {
        Some(ArchiveFormat::Zstd)
    } else if head.starts_with(b"BZh") {
        Some(ArchiveFormat::Bzip2)
    } else if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        // LZ4 帧格式：魔数 0x184D2204（小端）
        Some(ArchiveFormat::Lz4)
    } else {
        None
    };
    // 注：Brotli **没有魔数**，光看头部认不出来 —— 只能按扩展名 `.br`（见 `looks_like_brotli_by_name`）

    // 裸 tar：偏移 257 处有 ustar（且开头不是压缩格式）
    let is_tar = head.len() > 262 && &head[257..262] == b"ustar";

    match (fmt, is_tar) {
        (Some(f), t) => Some(Detected {
            format: f,
            is_tar: t,
        }),
        (None, true) => Some(Detected {
            format: ArchiveFormat::Tar,
            is_tar: true,
        }),
        (None, false) => None,
    }
}

/// 按**文件名**判断是不是 tar 家族
///
/// 为什么不能只看头部：`.tar.gz` 的头部是 gzip 签名，里面装的是什么
/// 光看头部是看不出的（必须解开才知道）。所以这里按名字的约定判断 ——
/// 这也是所有归档工具的通行做法。
pub fn looks_like_tar_by_name(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_lowercase()) else {
        return false;
    };
    // 纯 .tar
    if name.ends_with(".tar") {
        return true;
    }
    // 压缩的 tar：.tar.gz / .tar.xz / .tar.zst / .tar.bz2 / .tgz / .txz ...
    const TAR_SUFFIXES: &[&str] = &[
        ".tar.gz",
        ".tar.gzip",
        ".tar.xz",
        ".tar.lzma",
        ".tar.zst",
        ".tar.zstd",
        ".tar.bz2",
        ".tar.bz",
        ".tgz",
        ".txz",
        ".tzst",
        ".tbz2",
        ".tbz",
        ".tar.br",
        ".tar.lz4",
    ];
    TAR_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// 按**文件名**判断是不是 Brotli（`.br`）
///
/// 为什么只能看名字：**Brotli 压根没有文件头魔数**（格式就是这么设计的），
/// 光看字节认不出来 —— 所有工具都是靠扩展名这条约定。
pub fn looks_like_brotli_by_name(path: &Path) -> bool {
    path.file_name()
        .map(|n| {
            let l = n.to_string_lossy().to_lowercase();
            l.ends_with(".br") || l.ends_with(".brotli")
        })
        .unwrap_or(false)
}

/// 从文件名推测"解出来应该叫什么"
///
/// `a.txt.gz` → `a.txt`；`a.tar.gz` → （tar 会展开，返回 None）
pub fn strip_suffix(path: &Path, fmt: ArchiveFormat) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let lower = name.to_lowercase();
    let suffixes: &[&str] = match fmt {
        ArchiveFormat::Gzip => &[".gz", ".gzip", ".tgz"],
        ArchiveFormat::Xz => &[".xz", ".lzma", ".txz"],
        ArchiveFormat::Zstd => &[".zst", ".zstd", ".tzst"],
        ArchiveFormat::Bzip2 => &[".bz2", ".bz", ".tbz2", ".tbz"],
        ArchiveFormat::Lz4 => &[".lz4"],
        ArchiveFormat::Brotli => &[".br"],
        _ => &[],
    };
    for s in suffixes {
        if lower.ends_with(s) {
            let stem = &name[..name.len() - s.len()];
            if stem.is_empty() {
                return None;
            }
            // .tgz / .txz 这类是 tar 的简写 → 交给 tar 展开
            if matches!(*s, ".tgz" | ".txz" | ".tzst" | ".tbz2" | ".tbz") {
                return None;
            }
            return Some(PathBuf::from(stem));
        }
    }
    None
}

/// 解压参数
#[derive(Debug, Clone)]
pub struct ExtractStreamOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 同名策略
    pub overwrite: crate::extract::Overwrite,
    /// 安全限值
    pub limits: Limits,
    /// 单流格式解出的文件名（`None` = 从源文件名推）
    pub out_name: Option<String>,
    /// 压缩数据大小（用于压缩比防护；0 = 不检查）
    pub packed_hint: u64,
    /// 挑哪些条目（单流只有一个条目，但 tar 家族用得上）
    pub selection: Selection,
}

impl ExtractStreamOptions {
    /// 以输出目录构造默认参数
    pub fn new<P: Into<PathBuf>>(dest: P) -> Self {
        Self {
            dest: dest.into(),
            overwrite: crate::extract::Overwrite::default(),
            limits: Limits::default(),
            out_name: None,
            packed_hint: 0,
            selection: Selection::all(),
        }
    }
}

/// **解压单流压缩格式**（gz / xz / zst / bz2）
///
/// 返回写出的字节数。
pub fn decompress_stream<R: Read>(
    fmt: ArchiveFormat,
    src: R,
    out_path: &Path,
    limits: &Limits,
    packed_hint: u64,
) -> Result<u64> {
    decompress_stream_ctx(
        fmt,
        src,
        out_path,
        limits,
        packed_hint,
        &UnpackCtx::quiet(0),
    )
}

/// 同 [`decompress_stream`]，但带上事件 / 进度 / 取消上下文
pub fn decompress_stream_ctx<R: Read>(
    fmt: ArchiveFormat,
    src: R,
    out_path: &Path,
    limits: &Limits,
    packed_hint: u64,
    ctx: &UnpackCtx,
) -> Result<u64> {
    if !ctx.verify_only && let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("建目录失败: {e}")))?;
    }

    // `packed_hint` = 压缩数据大小（0 = 不检查压缩比）
    let mut budget = crate::safety::ExtractBudget::new(limits.clone(), packed_hint);
    let out_label = out_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());

    // 断点续传：单流压缩整包只有一个输出文件，上次解完 → 直接算完成（0 秒）
    //
    // 单流**没法**中途续（破坏性压缩没有「第几个文件」的概念），
    // 但「解完过一次」这个事实仍然值得记住：上次解到 90% 断了要重来，
    // 上次其实已经解完了却被判失败（比如设置属性时出错），下次就该瞬间过。
    if ctx.resume_skip(&out_label, out_path, crate::resume::条目种类::文件) {
        return Ok(std::fs::metadata(out_path).map(|m| m.len()).unwrap_or(0));
    }

    // XZ 特殊：`lzma-rs` 的接口是「输入 reader + 输出 writer」，不走 Read 适配
    if fmt == ArchiveFormat::Xz {
        let src = src;
        let mut out = BudgetWriter {
            inner: crate::io::PartWriter::create_with(out_path, ctx.verify_only).map_err(|e| {
                { let m = format!("创建输出失败: {e}"); Error::from(e).context(m) }
            })?,
            budget: &mut budget,
            written: 0,
            ctx: ctx.clone(),
            label: out_label.clone(),
            cancelled: false,
        };
        let mut src = std::io::BufReader::new(src);
        lzma_rs::xz_decompress(&mut src, &mut out)
            .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("xz 解压失败: {e}")))?;
        let written = out.written;
        if out.cancelled {
            // part 由 Drop 删掉：不留半截正式文件
            return Err(Error::cancelled());
        }
        // 原子落地：改名成功之前，正式文件名还不存在
        out.inner.finish().map_err(|e| {
            Error::new(ErrorCode::Internal).context(format!("落盘改名失败: {e}"))
        })?;
        ctx.resume_record(&out_label, out_path, crate::resume::条目种类::文件);
        ctx.entry_done(0, &out_label, ItemResult::Ok, written);
        return Ok(written);
    }

    // 其它都是标准 Read 解码器（一行一个，新格式往这儿加就行）
    let mut reader: Box<dyn Read> = match fmt {
        ArchiveFormat::Gzip => Box::new(flate2::read::GzDecoder::new(src)),
        ArchiveFormat::Zstd => {
            Box::new(ruzstd::decoding::StreamingDecoder::new(src).map_err(|e| {
                Error::new(ErrorCode::NotArchive).context(format!("zstd 头无效: {e}"))
            })?)
        }
        ArchiveFormat::Bzip2 => Box::new(bzip2_rs::DecoderReader::new(src)),
        // LZ4 帧格式（64 KiB 一块，块本身自带校验）
        ArchiveFormat::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(src)),
        // Brotli（没有魔数，靠扩展名进来的）
        ArchiveFormat::Brotli => Box::new(brotli::Decompressor::new(src, 64 * 1024)),
        other => {
            return Err(Error::new(ErrorCode::NotArchive)
                .with("reason", format!("不是单流格式: {other:?}")));
        }
    };

    // T9-P1/T8-P6：先写 .yshjy-part，写完再原子改名
    let mut part = crate::io::PartFile::create_with(out_path, ctx.verify_only)
        .map_err(|e| { let m = format!("创建输出失败: {e}"); Error::from(e).context(m) })?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        // 取消检查下沉到数据块级
        if ctx.is_cancelled() {
            // part 由 Drop 删掉
            return Err(Error::cancelled());
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                return Err(Error::new(ErrorCode::NotArchive)
                    .context(format!("解压 {fmt:?} 数据出错: {e}")));
            }
        };
        budget.record(n as u64)?;
        part.as_file().write_all(&buf[..n]).map_err(|e| {
            // 单流没有「跳过单条」的概念，失败即整体失败 —— 直接用分类后的码返回
            let m = format!("写盘失败: {e}");
            Error::from(e).context(m)
        })?;
        total += n as u64;
        ctx.advance(n as u64, Some(&out_label));
    }
    // 原子落地
    part.finish()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("落盘改名失败: {e}")))?;
    ctx.resume_record(&out_label, out_path, crate::resume::条目种类::文件);
    ctx.entry_done(0, &out_label, ItemResult::Ok, total);
    Ok(total)
}

/// 带限额统计的 Writer（给 XZ 用）
struct BudgetWriter<'a> {
    /// 原子写入守卫（写 `.yshjy-part`，成功后再改名）
    inner: crate::io::PartWriter,
    budget: &'a mut crate::safety::ExtractBudget,
    written: u64,
    /// 事件 / 进度 / 取消上下文
    ctx: UnpackCtx,
    /// 条目名（事件里用）
    label: String,
    /// 是否因取消而中途停了
    cancelled: bool,
}

impl Write for BudgetWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // 取消检查：解到一半也能停（xz 走 writer 接口，只能在这里拦）
        if self.ctx.is_cancelled() {
            self.cancelled = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled by user",
            ));
        }
        self.budget
            .record(buf.len() as u64)
            .map_err(std::io::Error::other)?;
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        self.ctx.advance(n as u64, Some(&self.label));
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// **剥掉 tar 的外层压缩壳**，得到一条"未压缩的 tar 字节流"
///
/// 解压（[`extract_tar_ctx`]）和列清单（`unpack::TarStreamUnpacker::list`）
/// **必须**共用这一份逻辑 —— 否则两边对"支持哪些外层格式"的支持会悄悄分叉：
/// 解压认得 `.tar.br`、预览却不认，用户看到的就是"这包没法预览"。
///
/// `outer` 传 `None` = 裸 tar；`Some(fmt)` = 先解 `fmt` 这一层再当 tar 读。
pub(crate) fn open_tar_stream<'a, R: Read + 'a>(
    src: R,
    outer: Option<ArchiveFormat>,
) -> Result<Box<dyn Read + 'a>> {
    Ok(match outer {
        None => Box::new(src),
        Some(ArchiveFormat::Gzip) => Box::new(flate2::read::GzDecoder::new(src)),
        Some(ArchiveFormat::Zstd) => {
            Box::new(ruzstd::decoding::StreamingDecoder::new(src).map_err(|e| {
                Error::new(ErrorCode::NotArchive).context(format!("zstd 头无效: {e}"))
            })?)
        }
        Some(ArchiveFormat::Bzip2) => Box::new(bzip2_rs::DecoderReader::new(src)),
        Some(ArchiveFormat::Xz) => {
            // xz 又要 writer 接口 → 先解到临时文件再当 read 用
            let tmp = std::env::temp_dir().join(format!(
                "yshjy_tar_xz_{}_{}.tar",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
            ));
            {
                let mut f = fs::File::create(&tmp).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("建临时文件失败: {e}"))
                })?;
                let mut src = std::io::BufReader::new(src);
                lzma_rs::xz_decompress(&mut src, &mut f).map_err(|e| {
                    Error::new(ErrorCode::NotArchive).context(format!("xz 解压失败: {e}"))
                })?;
            }
            let f = fs::File::open(&tmp).map_err(|e| {
                Error::new(ErrorCode::Internal).context(format!("读临时文件失败: {e}"))
            })?;
            Box::new(TempFileReader { file: f, path: tmp })
        }
        Some(other) => {
            return Err(Error::new(ErrorCode::NotArchive)
                .with("reason", format!("不支持的 tar 外层格式: {other:?}")));
        }
    })
}

/// **解压 TAR**（含 `.tar.gz` / `.tar.xz` / `.tar.zst` / `.tar.bz2` 两层）
///
/// `outer` 传 `None` 表示裸 tar；传 `Some(fmt)` 表示先解外层压缩流再展开 tar。
/// # 为什么是 `R: Read + 'a` 而不是 `R: Read + 'static`
/// 早先写成了 `'static`，只是因为内部把源装进了 `Box<dyn Read>`（默认带 `'static`）。
/// 这让**借来的流**（`&mut File`、`&mut Window`…）没法传进来 —— 而统一接口
/// （`crate::unpack::Source`）恰恰只提供借来的流。放宽成 `'a` 后两种都收，
/// 对原有调用点（传 `File` 这类拥有型）完全兼容：约束更松，调用更自由。
pub fn extract_tar<'a, R: Read + 'a>(
    src: R,
    outer: Option<ArchiveFormat>,
    opts: &ExtractStreamOptions,
) -> Result<crate::extract::ExtractReport> {
    extract_tar_ctx(src, outer, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_tar`]，但带上事件 / 进度 / 取消上下文
pub fn extract_tar_ctx<'a, R: Read + 'a>(
    src: R,
    outer: Option<ArchiveFormat>,
    opts: &ExtractStreamOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    // 只测不写：不建输出目录
    if !ctx.verify_only {
        fs::create_dir_all(&opts.dest)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("建目录失败: {e}")))?;
    }

    // ── 剥外层压缩流（与「列清单」共用同一份逻辑，见 open_tar_stream）──
    let reader = open_tar_stream(src, outer)?;

    let mut archive = tar::Archive::new(reader);
    let mut report = crate::extract::ExtractReport::default();
    let mut budget = crate::safety::ExtractBudget::new(opts.limits.clone(), opts.packed_hint);

    let entries = archive
        .entries()
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("读 tar 条目失败: {e}")))?;

    for (zero_based, ent) in entries.enumerate() {
        // tar 的迭代也不给"用户看到的号"，自己算（**1-based**，与预览一致）
        let idx = zero_based as u64 + 1;
        // 用户取消：已解出的保留，不再往下做
        if ctx.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let mut ent = match ent {
            Ok(e) => e,
            Err(e) => {
                report.warnings.push(format!("跳过损坏条目: {e}"));
                continue;
            }
        };
        let raw_path = match ent.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                report
                    .skipped
                    .push(("?".into(), format!("路径无法解析: {e}")));
                continue;
            }
        };
        let name = raw_path.clone();

        // 挑选：没被勾中的条目直接跳过（不写盘、不算错）
        if !opts
            .selection
            .matches(idx, &name, ent.header().entry_type().is_dir())
        {
            continue;
        }

        // 目录
        if ent.header().entry_type().is_dir() {
            if let Ok(d) = safety::resolve_safe(&opts.dest, &name) {
                // 断点续传：上次已经建过 → 跳
                if ctx.resume_skip(&name, &d, crate::resume::条目种类::目录) {
                    ctx.entry_done(idx, &name, ItemResult::Ok, 0);
                    continue;
                }
                if !ctx.verify_only {
                    let _ = fs::create_dir_all(&d);
                }
                ctx.resume_record(&name, &d, crate::resume::条目种类::目录);
            }
            ctx.entry_done(idx, &name, ItemResult::Ok, 0);
            continue;
        }
        // 符号链接 / 硬链接：**安全地还原**（Linux 软件包的标准结构，X3）
        //
        // 规则：只接受「相对路径 + 逻辑规范化后仍在解压目录内」的链接；
        // 绝对路径、用 `..` 爬出目标目录的一律拒绝并说明（不静默、不假装解出来了）。
        let et = ent.header().entry_type();
        if et.is_symlink() || et.is_hard_link() {
            let 链接目标 = ent
                .link_name()
                .ok()
                .flatten()
                .map(|p| p.to_string_lossy().into_owned());
            let Some(目标) = 链接目标 else {
                report
                    .skipped
                    .push((name.clone(), "链接条目没写目标，已跳过".into()));
                continue;
            };
            let 链接路径 = match safety::resolve_safe(&opts.dest, &name) {
                Ok(p) => p,
                Err(e) => {
                    report
                        .skipped
                        .push((name.clone(), format!("链接的位置不安全，已跳过: {e}")));
                    continue;
                }
            };
            if !链接目标安全(&opts.dest, &链接路径, &目标) {
                report.skipped.push((
                    name.clone(),
                    format!("链接指向「{目标}」，会跑到解压目录外，已拒绝（防逃逸）"),
                ));
                continue;
            }
            if !ctx.verify_only && let Some(parent) = 链接路径.parent() {
                let _ = fs::create_dir_all(parent);
            }
            // 断点续传：上次链接已经建好 → 跳
            if ctx.resume_skip(&name, &链接路径, crate::resume::条目种类::链接) {
                ctx.entry_done(idx, &name, ItemResult::Ok, 0);
                report.files.push(crate::extract::ExtractedFile {
                    name,
                    out_path: 链接路径,
                    size: 0,
                    encrypted: false,
                });
                continue;
            }
            let r = if et.is_symlink() {
                #[cfg(unix)]
                {
                    let _ = fs::remove_file(&链接路径);
                    std::os::unix::fs::symlink(&目标, &链接路径)
                }
                #[cfg(not(unix))]
                {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "这个系统不支持创建符号链接",
                    ))
                }
            } else {
                // 硬链接：目标必须是同一个解压目录里已经解出来的文件
                let base = 链接路径.parent().unwrap_or(&opts.dest);
                fs::hard_link(base.join(&目标), &链接路径)
            };
            match r {
                Ok(()) => {
                    ctx.resume_record(&name, &链接路径, crate::resume::条目种类::链接);
                    report.files.push(crate::extract::ExtractedFile {
                        name,
                        out_path: 链接路径,
                        size: 0,
                        encrypted: false,
                    });
                }
                Err(e) => {
                    report
                        .skipped
                        .push((name.clone(), format!("链接建不起来: {e}")));
                }
            }
            continue;
        }

        // 安全路径
        let mut out_path = match safety::resolve_safe(&opts.dest, &name) {
            Ok(p) => p,
            Err(e) => {
                report
                    .skipped
                    .push((name.clone(), format!("路径不安全被拒: {e}")));
                continue;
            }
        };
        // 断点续传：上次完整解过这条 → 不重解（必须在同名处理之前，理由同 7z）
        if ctx.resume_skip(&name, &out_path, crate::resume::条目种类::文件) {
            let 大小 = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            ctx.entry_done(idx, &name, ItemResult::Ok, 0);
            report.files.push(crate::extract::ExtractedFile {
                name,
                out_path,
                size: 大小,
                encrypted: false,
            });
            continue;
        }

        if out_path.exists() {
            match ctx.同名策略(opts.overwrite) {
                crate::extract::Overwrite::Skip => {
                    report
                        .skipped
                        .push((name.clone(), "已存在，按策略跳过".into()));
                    continue;
                }
                crate::extract::Overwrite::Replace => {}
                crate::extract::Overwrite::Rename => out_path = unique_path_stream(&out_path),
            }
        }
        if !ctx.verify_only
            && let Some(parent) = out_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report
                .skipped
                .push((name.clone(), format!("建父目录失败: {e}")));
            continue;
        }
        if safety::should_warn_executable(&opts.limits, &name) {
            // 走**事件**而不是往内部列表里塞字符串：
            // ① 事件外壳才看得见（否则这条提醒等于没发）
            // ② 文案由外壳查语言包渲染（引擎不产句子）
            ctx.diagnostic(crate::event::DiagnosticCode::RiskyEntry, name.clone());
        }

        // tar 头里记的时间戳 / 权限（落盘后再设上去，见下面成功分支）
        let attrs = {
            let h = ent.header();
            crate::restore::EntryAttrs {
                mtime: h.mtime().ok().map(|s| s as i64),
                mode: h.mode().ok(),
            }
        };

        // T9-P1/T8-P6：先写 .yshjy-part，写完再原子改名
        let mut part = match crate::io::PartFile::create_with(&out_path, ctx.verify_only) {
            Ok(f) => f,
            Err(e) => {
                report.note_io_error(&e);
                report
                    .skipped
                    .push((name.clone(), format!("创建失败: {e}")));
                continue;
            }
        };
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        let mut err: Option<String> = None;
        let mut cancelled_now = false;
        loop {
            // 取消检查下沉到数据块级
            if ctx.is_cancelled() {
                cancelled_now = true;
                break;
            }
            match ent.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = budget.record(n as u64) {
                        if e.is_cancelled() {
                            cancelled_now = true;
                        } else {
                            err = Some(format!("安全限额中止: {e}"));
                        }
                        break;
                    }
                    if let Err(e) = part.as_file().write_all(&buf[..n]) {
                        report.note_io_error(&e);
                        err = Some(format!("写盘失败: {e}"));
                        break;
                    }
                    written += n as u64;
                    ctx.advance(n as u64, Some(&name));
                }
                Err(e) => {
                    err = Some(format!("读取失败: {e}"));
                    break;
                }
            }
        }
        if cancelled_now {
            drop(part);
            report.cancelled = true;
            break;
        }
        match err {
            Some(msg) => {
                drop(part);
                report.skipped.push((name.clone(), msg));
                ctx.entry_done(idx, &name, ItemResult::DataError, 0);
            }
            None => {
                // 原子落地：改名成功之前，正式文件名还不存在
                if let Err(e) = part.finish() {
                    report
                        .skipped
                        .push((name.clone(), format!("落盘改名失败: {e}")));
                    ctx.entry_done(idx, &name, ItemResult::DataError, 0);
                    continue;
                }
                report.bytes_written += written;
                // 还原归档里记的时间戳 / 权限（尽力而为：设不上不影响"解压成功"这个结论）
                if !ctx.verify_only
                    && !attrs.is_empty()
                    && let Err(e) = crate::restore::apply(&out_path, &attrs)
                    && report.warnings.len() < 3
                {
                    report.warnings.push(format!("属性没还原上：{e}"));
                }
                // 断点续传：原子改名 + 属性还原都过了才算完整落地
                ctx.resume_record(&name, &out_path, crate::resume::条目种类::文件);
                ctx.entry_done(idx, &name, ItemResult::Ok, written);
                report.files.push(crate::extract::ExtractedFile {
                    name,
                    out_path,
                    size: written,
                    encrypted: false,
                });
            }
        }
    }

    Ok(report)
}

/// 用完即删的临时文件 reader（xz + tar 的组合路径用）
struct TempFileReader {
    file: fs::File,
    path: PathBuf,
}

impl Read for TempFileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }
}

impl Drop for TempFileReader {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// 链接目标是否安全：必须是相对路径，且逻辑规范化后仍在**解压目录内**
///
/// 为什么不跟随真实符号链接：解压过程中目录才刚建，很多目标文件还没落盘，
/// `canonicalize` 会失败。这里做的是「纯字面规范化」—— 足够挡住 `../../etc/passwd`
/// 这类逃逸，也不会因为文件还没解出来而误判。
pub(crate) fn 链接目标安全(dest: &Path, 链接路径: &Path, 目标: &str) -> bool {
    let t = Path::new(目标);
    if t.is_absolute() {
        return false;
    }
    // Windows 盘符式（`C:\...`）：在 Linux 上不算绝对路径，但同样是逃逸
    if 目标.len() >= 2
        && 目标.as_bytes()[1] == b':'
        && 目标.as_bytes()[0].is_ascii_alphabetic()
    {
        return false;
    }
    let base = 链接路径.parent().unwrap_or(dest);
    let Ok(绝对) = std::path::absolute(base.join(t)) else {
        return false;
    };
    let Ok(根) = std::path::absolute(dest) else {
        return false;
    };
    消点(&绝对).starts_with(消点(&根))
}

/// 纯字面消除 `.` / `..`（不碰文件系统）
fn 消点(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(crate) fn unique_path_stream(path: &Path) -> PathBuf {
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
    use std::io::Cursor;

    #[test]
    fn 能认出_gzip_签名() {
        let d = detect(b"\x1f\x8b\x08\x00rest");
        assert_eq!(d.map(|x| x.format), Some(ArchiveFormat::Gzip));
        assert!(!d.unwrap().is_tar);
    }

    #[test]
    fn 能认出_xz_签名() {
        assert_eq!(
            detect(b"\xfd7zXZ\x00\x00rest").map(|x| x.format),
            Some(ArchiveFormat::Xz)
        );
    }

    #[test]
    fn 能认出_zstd_签名() {
        assert_eq!(
            detect(b"\x28\xb5\x2f\xfd\x00rest").map(|x| x.format),
            Some(ArchiveFormat::Zstd)
        );
    }

    #[test]
    fn 能认出_bzip2_签名() {
        assert_eq!(
            detect(b"BZh9rest").map(|x| x.format),
            Some(ArchiveFormat::Bzip2)
        );
    }

    #[test]
    fn 能认出裸_tar() {
        let mut v = vec![0u8; 512];
        v[257..262].copy_from_slice(b"ustar");
        let d = detect(&v).unwrap();
        assert_eq!(d.format, ArchiveFormat::Tar);
        assert!(d.is_tar);
    }

    #[test]
    fn 普通文件不会被误判() {
        assert!(detect(b"hello world, this is not an archive").is_none());
        assert!(detect(b"PK\x03\x04").is_none(), "ZIP 归容器路径管");
    }

    #[test]
    fn 后缀推断正确() {
        assert_eq!(
            strip_suffix(Path::new("a.txt.gz"), ArchiveFormat::Gzip),
            Some(PathBuf::from("a.txt"))
        );
        assert_eq!(
            strip_suffix(Path::new("b.bin.xz"), ArchiveFormat::Xz),
            Some(PathBuf::from("b.bin"))
        );
        // .tgz 是 tar 的简写 → 交给 tar 展开
        assert_eq!(strip_suffix(Path::new("c.tgz"), ArchiveFormat::Gzip), None);
    }

    /// 用系统工具造各格式样本（测试环境一般都有）
    fn make_with(cmd: &str, input: &[u8], ext: &str) -> Option<Vec<u8>> {
        let dir = std::env::temp_dir().join(format!(
            "yshjy_fmt_{}_{}",
            std::process::id(),
            ext.replace('.', "")
        ));
        let _ = fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let src = dir.join("in.bin");
        std::fs::write(&src, input).ok()?;
        let ok = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!("cd {dir:?} && {cmd} in.bin 2>/dev/null && ls"))
            .output()
            .ok()?;
        if !ok.status.success() {
            let _ = fs::remove_dir_all(&dir);
            return None;
        }
        let out = dir.join(format!("in.bin{ext}"));
        let data = std::fs::read(&out).ok();
        let _ = fs::remove_dir_all(&dir);
        data
    }

    /// 通用：拿一段压缩数据，用我们的解码器解出来，比对内容
    fn roundtrip(fmt: ArchiveFormat, compressed: Vec<u8>, expect: &[u8]) {
        let dest = std::env::temp_dir().join(format!("yshjy_rt_{}_{:?}", std::process::id(), fmt));
        let _ = fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let out = dest.join("out.bin");
        let n =
            decompress_stream(fmt, Cursor::new(compressed), &out, &Limits::default(), 0).unwrap();
        assert_eq!(n as usize, expect.len(), "{fmt:?} 解出长度不对");
        assert_eq!(fs::read(&out).unwrap(), expect, "{fmt:?} 内容不对");
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn xz_能真的解开() {
        let payload = b"hello xz content, this is a test payload for xz";
        match make_with("xz -k -f", payload, ".xz") {
            Some(c) => roundtrip(ArchiveFormat::Xz, c, payload),
            None => eprintln!("环境没有 xz 命令，跳过"),
        }
    }

    #[test]
    fn zstd_能真的解开() {
        let payload = b"hello zstd content, this is a test payload for zstd";
        match make_with("zstd -k -f", payload, ".zst") {
            Some(c) => roundtrip(ArchiveFormat::Zstd, c, payload),
            None => eprintln!("环境没有 zstd 命令，跳过"),
        }
    }

    #[test]
    fn bzip2_能真的解开() {
        let payload = b"hello bzip2 content, this is a test payload for bzip2";
        match make_with("bzip2 -k -f", payload, ".bz2") {
            Some(c) => roundtrip(ArchiveFormat::Bzip2, c, payload),
            None => eprintln!("环境没有 bzip2 命令，跳过"),
        }
    }

    #[test]
    fn tar_能展开成目录树() {
        let dir = std::env::temp_dir().join(format!("yshjy_tar_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.txt"), b"tar content a").unwrap();
        std::fs::write(dir.join("b.txt"), b"tar content b").unwrap();

        // 造 tar（+ 各种外层压缩）
        let made = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!("cd {dir:?} && tar cf plain.tar src b.txt"))
            .output()
            .unwrap();
        assert!(made.status.success(), "tar 命令应当可用");

        for (name, outer) in [
            ("plain.tar", None),
            ("plain.tar.gz", Some(ArchiveFormat::Gzip)),
            ("plain.tar.xz", Some(ArchiveFormat::Xz)),
            ("plain.tar.bz2", Some(ArchiveFormat::Bzip2)),
            ("plain.tar.zst", Some(ArchiveFormat::Zstd)),
        ] {
            if let Some(fmt) = outer {
                let cmd = match fmt {
                    ArchiveFormat::Gzip => "gzip -k -f",
                    ArchiveFormat::Xz => "xz -k -f",
                    ArchiveFormat::Bzip2 => "bzip2 -k -f",
                    ArchiveFormat::Zstd => "zstd -k -f",
                    _ => continue,
                };
                let r = std::process::Command::new("bash")
                    .arg("-c")
                    .arg(format!("cd {dir:?} && {cmd} plain.tar 2>/dev/null"))
                    .output()
                    .unwrap();
                if !r.status.success() {
                    continue; // 环境没这个命令
                }
            }
            let src = dir.join(name);
            if !src.exists() {
                continue;
            }
            let out = dir.join(format!("out_{}", name.replace('.', "_")));
            let _ = fs::remove_dir_all(&out);
            let f = fs::File::open(&src).unwrap();
            let opts = ExtractStreamOptions::new(&out);
            let rep = extract_tar(f, outer, &opts).unwrap();
            assert!(
                rep.files.len() >= 2,
                "{name} 应解出至少 2 个文件，实际 {:?}",
                rep.skipped
            );
            // 内容抽一个验证
            let got = fs::read(out.join("a.txt"))
                .unwrap_or_else(|_| fs::read(out.join("src/a.txt")).unwrap_or_default());
            assert_eq!(got, b"tar content a", "{name} 内容不对");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lz4_单流能靠魔数认出并解开() {
        use std::io::Write;
        // 用「不太能压」的数据：压缩炸弹防护会拦高压缩比（这是防护正常工作，
        // 别为了测试去放宽安全限值）
        let data: Vec<u8> = (0..15000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 17) as u8)
            .collect();

        let mut enc = lz4_flex::frame::FrameEncoder::new(Vec::new());
        enc.write_all(&data).unwrap();
        let packed = enc.finish().unwrap();

        // ① 有魔数 → 光看头部就能认
        let d = detect(&packed).expect("LZ4 应当能靠魔数认出来");
        assert_eq!(d.format, ArchiveFormat::Lz4);

        // ② 解开逐字节一致
        let p = 测试临时目录("lz4");
        let out = p.join("a.txt");
        let n = decompress_stream(
            ArchiveFormat::Lz4,
            std::io::Cursor::new(&packed),
            &out,
            &Limits::default(),
            packed.len() as u64,
        )
        .unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(fs::read(&out).unwrap(), data);

        // ③ 解出来的名字：去掉 .lz4
        assert_eq!(
            strip_suffix(Path::new("a.txt.lz4"), ArchiveFormat::Lz4),
            Some(PathBuf::from("a.txt"))
        );
        let _ = fs::remove_dir_all(&p);
    }

    #[test]
    fn brotli_没有魔数只能按扩展名认并解开() {
        use std::io::Write;
        // 同上：数据要「不太能压」，否则会被压缩炸弹防护拦下
        let data: Vec<u8> = (0..15000u32)
            .map(|i| (i.wrapping_mul(40503) >> 11) as u8)
            .collect();

        let mut packed = Vec::new();
        {
            let mut w = brotli::CompressorWriter::new(&mut packed, 4096, 5, 22);
            w.write_all(&data).unwrap();
        }

        // ① 光看头部**认不出来** —— 这是格式特性不是 bug，代码注释和文档都写清楚
        assert_ne!(
            detect(&packed).map(|d| d.format),
            Some(ArchiveFormat::Brotli),
            "brotli 不该靠魔数认出来（它根本没有魔数）"
        );
        // ② 所以只能按文件名认
        assert!(looks_like_brotli_by_name(Path::new("a.txt.br")));
        assert!(looks_like_brotli_by_name(Path::new("备份.BR")));
        assert!(!looks_like_brotli_by_name(Path::new("a.txt.zip")));

        // ③ 解开逐字节一致
        let p = 测试临时目录("br");
        let out = p.join("a.txt");
        let n = decompress_stream(
            ArchiveFormat::Brotli,
            std::io::Cursor::new(&packed),
            &out,
            &Limits::default(),
            packed.len() as u64,
        )
        .unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(fs::read(&out).unwrap(), data);
        assert_eq!(
            strip_suffix(Path::new("a.txt.br"), ArchiveFormat::Brotli),
            Some(PathBuf::from("a.txt"))
        );
        let _ = fs::remove_dir_all(&p);
    }

    /// 测试用临时目录（每个用例独立，跑完自己删）
    fn 测试临时目录(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_stream_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn gzip_能真的解开() {
        use flate2::write::GzEncoder;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"hello gzip content").unwrap();
        let compressed = enc.finish().unwrap();

        let dest = std::env::temp_dir().join(format!("yshjy_gz_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dest);
        fs::create_dir_all(&dest).unwrap();
        let out = dest.join("out.txt");
        let n = decompress_stream(
            ArchiveFormat::Gzip,
            Cursor::new(compressed),
            &out,
            &Limits::default(),
            0,
        )
        .unwrap();
        assert_eq!(n, 18);
        assert_eq!(fs::read_to_string(&out).unwrap(), "hello gzip content");
        let _ = fs::remove_dir_all(&dest);
    }
}
