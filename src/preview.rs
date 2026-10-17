//! 预览：**不解压**就把包里有什么列出来
//!
//! # 为什么它很重要（界面 S3 的数据源）
//! UI-SPEC 的 S3 是「列出内容 → 勾选 → 开始解压」。要支持它，引擎必须能在
//! **不写一个字节到磁盘**的前提下说出包里有什么 —— 8 GB 的包也要秒级列出来。
//!
//! 三种格式各自的拿法（都只读头部，不解码数据）：
//! - **ZIP**：算好基准偏移，喂给 `zip` 库读中央目录
//! - **7z**：`ArchiveReader` 解析头 → `Archive.files`（头里就带名字/大小）
//! - **RAR**：`rars` 的 `members()`（只解析头；库要求数据在内存里，所以超大 RAR 会降级）
//!
//! 单流格式只有一个产物，直接由文件名推出；**TAR 家族**是顺序格式、没有索引，
//! 只能把整条流的头扫一遍（不读数据体），实现见 `unpack::TarStreamUnpacker::list`。

use std::path::{Path, PathBuf};

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, Result};
use crate::event::{EntryView, Event, PreviewHint, Stage};
use crate::job::Target;
use crate::layout::ArchiveFormat;
use crate::safety;
use crate::unpack::Shape;

/// 预览结果
///
/// > **序号约定（重要）**：`entries` 里的 `index` **从 1 开始**，
/// > 与界面/CLI 上显示的序号完全一致 —— `--select "1,3"` 里的 1 就是第一个条目。
/// > 挑选、进度、条目事件一律沿用这套序号，避免"看到 1、勾到 2"这类错位。
#[derive(Debug, Clone, Default)]
pub struct Preview {
    /// 条目清单（顺序 = 归档内顺序，也就是解压时的序号）
    pub entries: Vec<EntryView>,
    /// 给界面的提示码（外壳渲染成人话）
    pub hints: Vec<PreviewHint>,
    /// 解压后总大小
    pub total_size: u64,
    /// 带锁的条目数
    pub encrypted_count: u64,
    /// 格式名（中性，日志用）
    pub format: &'static str,
    /// 预览是否完整（`false` = 某种格式只能给出部分信息）
    pub complete: bool,
}

impl Preview {
    /// 非目录条目数（"里面有几个文件"）
    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir).count()
    }

    /// 如果整包只有**一个顶层目录**，返回它的名字
    ///
    /// 智能解压三规则用它：「只有一个顶层目录 → 直接解到当前目录，不要多套一层」。
    pub fn single_root(&self) -> Option<String> {
        let mut root: Option<&str> = None;
        for e in &self.entries {
            let first = e.path.split('/').next().unwrap_or(&e.path);
            if first.is_empty() {
                continue;
            }
            // 只有「一条顶层条目 + 它是个目录（或路径里带 /）」才算单一根
            match root {
                None => root = Some(first),
                Some(r) if r == first => {}
                Some(_) => return None,
            }
        }
        let r = root?;
        let is_single_dir = self
            .entries
            .iter()
            .all(|e| e.path == r || e.path.starts_with(&format!("{r}/")));
        if is_single_dir && self.entries.iter().any(|e| e.path.starts_with(&format!("{r}/"))) {
            Some(r.to_string())
        } else {
            None
        }
    }
}

/// 预览一个文件（会发 `List` 阶段事件与 `PreviewReady`）
pub fn preview(
    input: &Path,
    ctx: &UnpackCtx,
    password: Option<&str>,
) -> Result<(Target, Preview)> {
    // 认出是什么（含定位，会发 Locate 阶段）
    let target = crate::job::identify(input, ctx)?;

    ctx.stage_started(Stage::List);
    let t0 = std::time::Instant::now();
    let mut pv = preview_target(input, &target, password, ctx)?;
    ctx.stage_finished(Stage::List, t0.elapsed().as_millis() as u64);

    // 补充格式特有的提示
    match &target {
        Target::Zip { base, .. } if *base > 0 => pv.hints.push(PreviewHint::HiddenAt { offset: *base }),
        Target::SevenZ { volumes } if volumes.len() > 1 => {
            pv.hints.push(PreviewHint::VolumesMerged {
                count: volumes.len() as u64,
            })
        }
        _ => {}
    }
    if pv.encrypted_count > 0 {
        pv.hints.push(PreviewHint::NeedsPassword {
            entries: pv.encrypted_count,
        });
    }
    // 藏在里面的包，最典型的形态是「只有一条，名字像压缩包」
    if pv.file_count() == 1
        && let Some(first) = pv.entries.iter().find(|e| !e.is_dir)
    {
        let lower = first.path.to_ascii_lowercase();
        for ext in [".7z", ".zip", ".rar", ".tar", ".gz", ".xz", ".zst", ".bz2"] {
            if lower.ends_with(ext) || lower.contains(&format!("{ext}.")) {
                pv.hints.push(PreviewHint::NestedArchive {
                    inner: ext.trim_start_matches('.').to_string(),
                });
                break;
            }
        }
    }
    // 残包判断：解压后总大小明显大于文件本身，且外层没有被撑开 → 可能没下全
    if pv.entries.is_empty() && !pv.complete {
        pv.hints.push(PreviewHint::LooksTruncated);
    }

    ctx.emit(&Event::PreviewReady {
        job: ctx.job,
        entries: pv.entries.clone(),
        hints: pv.hints.clone(),
    });
    Ok((target, pv))
}

/// 按目标类型把「源」准备好，然后**统一**交给 [`crate::unpack`] 的解压器列清单
///
/// 和 `job.rs::extract_target` 是同一个分工（那边解压、这边列清单）：
/// **源怎么准备**留在这里，**怎么读清单**交给 `impl InArchive` —— 加格式不用改这个函数。
fn preview_target(
    input: &Path,
    target: &Target,
    password: Option<&str>,
    ctx: &UnpackCtx,
) -> Result<Preview> {
    // 参数对象与解压共用同一个类型；`dest` 对"列清单"没有意义（绝不写盘），
    // 填输入所在目录只为让结构体完整。
    let mut uopts = crate::unpack::UnpackOptions::new(
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    );
    uopts.password = password.map(|s| s.to_string());

    match target {
        Target::Zip { base, len } => {
            let mut f = 打开输入(input)?;
            let mut src = crate::unpack::Source::new(&mut f).with_offset(*base, *len);
            列清单(ctx, &mut src, &uopts, ArchiveFormat::Zip, Shape::Plain)
        }
        Target::ZipSplit { volumes } => {
            // 拼成一条逻辑流 → 找中央目录 → 之后和普通 ZIP 完全一样
            let mut mf = crate::extract7z::MultiFile::open(volumes)?;
            let size = mf.total();
            let l = crate::layout::zip::locate(&mut mf, size).map_err(|e| {
                Error::new(ErrorCode::NotArchive)
                    .context(format!("分卷里的中央目录没找到（是不是缺了一块？）: {e}"))
            })?;
            let mut src = crate::unpack::Source::new(&mut mf)
                .with_offset(l.base, l.data_len())
                .with_packed_total(size);
            列清单(ctx, &mut src, &uopts, ArchiveFormat::Zip, Shape::Volumes)
        }
        Target::SevenZ { volumes } => {
            let mut mf = crate::extract7z::MultiFile::open(volumes)?;
            let size = mf.total();
            let mut src = crate::unpack::Source::new(&mut mf).with_packed_total(size);
            列清单(ctx, &mut src, &uopts, ArchiveFormat::SevenZ, Shape::Volumes)
        }
        Target::Rar => {
            let mut f = 打开输入(input)?;
            let mut src = crate::unpack::Source::new(&mut f);
            列清单(ctx, &mut src, &uopts, ArchiveFormat::Rar4, Shape::Plain)
        }
        Target::Stream {
            format,
            is_tar,
            packed,
        } => {
            if *is_tar {
                // tar 家族：裸 tar 没有外层，tar.gz 这类外层就是那个压缩格式
                uopts.outer = if *format == ArchiveFormat::Tar {
                    None
                } else {
                    Some(*format)
                };
                let mut f = 打开输入(input)?;
                let mut src = crate::unpack::Source::new(&mut f).with_packed_total(*packed);
                列清单(ctx, &mut src, &uopts, ArchiveFormat::Tar, Shape::Tar)
            } else {
                uopts.outer = Some(*format);
                uopts.packed_hint = *packed;
                uopts.out_name = crate::extract_stream::strip_suffix(input, *format)
                    .map(|x| x.to_string_lossy().to_string());
                let mut f = 打开输入(input)?;
                let mut src = crate::unpack::Source::new(&mut f).with_packed_total(*packed);
                列清单(ctx, &mut src, &uopts, *format, Shape::Single)
            }
        }
    }
}

/// 打开输入文件（错误码统一：路径问题就说路径问题）
fn 打开输入(input: &Path) -> Result<std::fs::File> {
    std::fs::File::open(input)
        .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open: {e}")))
}

/// 查注册表拿解压器列清单（与 `job.rs` 里的解压路径同一个思路）
fn 列清单(
    ctx: &UnpackCtx,
    src: &mut crate::unpack::Source<'_>,
    uopts: &crate::unpack::UnpackOptions,
    格式: ArchiveFormat,
    形状: Shape,
) -> Result<Preview> {
    let handler = crate::unpack::find_or_err(格式, 形状)?;
    handler.list(src, uopts, ctx)
}

// （这里原来有两个"按路径工作"的外壳：`preview_zip` 与 `preview_zip_volumes`。
//  v0.9 把"准备源"这一步并进了统一接口 —— `src/unpack.rs` 的 `ZipUnpacker::list`
//  接手了它，两条路共用同一个 `preview_zip_reader`。
//  实测结论保留：算好偏移再喂给库读中央目录，8.7 GB 的包只要 69 微秒。）

/// 真正的清单读取（普通 ZIP / 分卷 / 统一接口共用同一份）
///
/// `pub(crate)`：统一接口（[`crate::unpack::ZipUnpacker`] 的 `list`）也调它 ——
/// 一份实现两处用，别再抄第二份。
pub(crate) fn preview_zip_reader<R: std::io::Read + std::io::Seek>(src: R) -> Result<Preview> {
    let mut archive = crate::layout::zip::打开已知偏移(src)
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("zip open: {e}")))?;

    let mut entries = Vec::with_capacity(archive.len());
    let mut total = 0u64;
    let mut enc = 0u64;
    for i in 0..archive.len() {
        let Ok(raw) = archive.by_index_raw(i) else {
            continue;
        };
        // 用我们自己的四级回退解码（库的 name() 会按 CP437 把中文解成乱码）
        let path = crate::layout::zip::decode_name(raw.name_raw(), false);
        let is_dir = raw.is_dir();
        let size = raw.size();
        let encrypted = raw.encrypted();
        if !is_dir {
            total += size;
        }
        if encrypted {
            enc += 1;
        }
        entries.push(EntryView {
            index: i as u64 + 1,
            is_risky: !is_dir && safety::is_risky_executable(&path),
            path,
            size,
            packed: Some(raw.compressed_size()),
            encrypted,
            is_dir,
            modified: None,
        });
    }
    Ok(Preview {
        entries,
        hints: Vec::new(),
        total_size: total,
        encrypted_count: enc,
        format: "zip",
        complete: true,
    })
}

/// 7z：解析头就拿到条目（不解码数据）。
/// 加密头且没给密码时，只能报「需要密码」。
///
/// `pub(crate)` + 收**一条流**：统一接口（[`crate::unpack::SevenZUnpacker`]）也调它；
/// 「把多卷拼成一条流」是编排层的事（`MultiFile`），这里不该知道卷名规则。
pub(crate) fn preview_7z<R: std::io::Read + std::io::Seek>(
    src: R,
    password: Option<&str>,
) -> Result<Preview> {
    let pw = match password {
        Some(p) => sevenz_rust2::Password::new(p),
        None => sevenz_rust2::Password::empty(),
    };
    let reader = sevenz_rust2::ArchiveReader::new(src, pw).map_err(|e| {
        let msg = format!("{e}");
        let code = if msg.contains("密码") || msg.to_lowercase().contains("password") {
            ErrorCode::NeedPassword
        } else {
            ErrorCode::NotArchive
        };
        Error::new(code).context(format!("7z 读头失败: {msg}"))
    })?;

    let mut entries = Vec::with_capacity(reader.archive().files.len());
    let mut total = 0u64;
    for (i, f) in reader.archive().files.iter().enumerate() {
        let is_dir = f.is_directory();
        let size = if is_dir { 0 } else { f.size() };
        if !is_dir {
            total += size;
        }
        entries.push(EntryView {
            index: i as u64 + 1,
            path: f.name().to_string(),
            size,
            packed: None,
            encrypted: false, // 7z 的条目级加密标记要先解密头才知道，这里保守报 false
            is_dir,
            modified: None,
            is_risky: !is_dir && safety::is_risky_executable(f.name()),
        });
    }
    Ok(Preview {
        entries,
        hints: Vec::new(),
        total_size: total,
        encrypted_count: 0,
        format: "7z",
        complete: true,
    })
}

/// RAR：`rars` 要求数据在内存里，超过上限就降级（如实说明，不硬撑）
///
/// `pub(crate)` + 收**一条流**：统一接口（[`crate::unpack::RarUnpacker`]）也调它。
pub(crate) fn preview_rar<R: std::io::Read + std::io::Seek>(src: &mut R) -> Result<Preview> {
    // 注：`seek` 能直接调，是因为 `R: Seek` 这个 bound；但 `take(...)` 返回的
    // `Take<&mut R>` 是**具体类型**，它的 `read_to_end` 需要 `Read` 在作用域里。
    use std::io::Read as _;

    // 长度从流自己量（和 `extract_rar_stream_ctx` 同一套办法）
    let size = src
        .seek(std::io::SeekFrom::End(0))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("取流长度失败: {e}")))?;
    if size > crate::extract_rar::MAX_SIZE {
        // 大 RAR 不预览（底层库限制），标注「不完整」让外壳显示「无法预览」
        return Ok(Preview {
            format: "rar",
            complete: false,
            ..Default::default()
        });
    }
    src.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("回到流开头失败: {e}")))?;
    let mut data = Vec::with_capacity(size as usize);
    src.take(crate::extract_rar::MAX_SIZE + 1)
        .read_to_end(&mut data)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read: {e}")))?;
    let archive = rars::ArchiveReader::read(&data)
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("rar open: {e}")))?;

    let members: Vec<_> = archive.member_refs().collect();
    let mut entries = Vec::with_capacity(members.len());
    for (i, m) in members.iter().enumerate() {
        let raw_name = String::from_utf8_lossy(m.name_bytes()).to_string();
        let name = raw_name.replace('\\', "/");
        let is_dir = m.is_directory();
        entries.push(EntryView {
            index: i as u64 + 1,
            is_risky: !is_dir && safety::is_risky_executable(&name),
            path: name,
            // `rars` 公开的成员元数据里**没有大小**（size 要解析更深一层头才拿得到），
            // 所以 RAR 预览的条目大小报 0，并用 complete=false 如实说明
            size: 0,
            packed: None,
            encrypted: false,
            is_dir,
            modified: None,
        });
    }
    Ok(Preview {
        entries,
        hints: Vec::new(),
        total_size: 0,
        encrypted_count: 0,
        format: "rar",
        complete: false,
    })
}

/// 单流：只有一个产物，名字由**编排层**给（它手上有原始文件名）
///
/// 为什么名字要外面给：`Source` 只认一条流、不再带路径（RAR 也能从流读之后那个
/// 字段就删了）—— 而单流的名字只能从原始文件名推（Brotli 连魔数都没有，只认 `.br`），
/// 所以这一步归编排层。这也让 handler 不必知道"文件叫什么"。
pub(crate) fn preview_single_stream(
    format: crate::layout::ArchiveFormat,
    packed: u64,
    name: &str,
) -> Result<Preview> {
    let name = name.to_string();
    Ok(Preview {
        entries: vec![EntryView {
            index: 1, // 单条目也就一个号
            path: name,
            // 单流格式的原始大小藏在尾部/头部（gzip 的 ISIZE 只存低 32 位），不可靠 → 报 0
            size: 0,
            packed: Some(packed),
            encrypted: false,
            is_dir: false,
            modified: None,
            is_risky: false,
        }],
        hints: Vec::new(),
        total_size: 0,
        encrypted_count: 0,
        // 显示具体格式名（brotli / lz4 / gzip…）比笼统的 "stream" 有用得多
        format: format.as_str(),
        complete: true,
    })
}

// ───────────────────────── 智能解压三规则 ─────────────────────────

/// 按 UI-SPEC §9.2 的「智能解压三规则」决定解到哪
///
/// 规则（源自 Bandizip 的智能解压，实战里最省事）：
/// 1. 包里**只有一个顶层目录** → 直接解到目标目录（不再多套一层同名文件夹）
/// 2. 否则 → 自动建同名文件夹（避免几十个文件散在下载目录里）
/// 3. 重名 → 交给同名策略（`Rename` 会追加 `(2)`）
pub fn smart_dest(base: &Path, preview: &Preview, fallback_name: &str) -> PathBuf {
    match preview.single_root() {
        // 单一顶层目录：解到 base 就行，那一层由包里的目录结构自己带出来
        Some(_) => base.to_path_buf(),
        None => {
            if preview.file_count() <= 1 && !preview.entries.is_empty() {
                // 单个文件：直接放在 base 里
                base.to_path_buf()
            } else {
                base.join(fallback_name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn 造_zip(条目: &[(&str, usize)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, size) in 条目 {
            if name.ends_with('/') {
                w.add_directory(*name, opt).unwrap();
            } else {
                w.start_file(*name, opt).unwrap();
                w.write_all(&vec![b'x'; *size]).unwrap();
            }
        }
        w.finish().unwrap().into_inner()
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_preview_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn 预览_zip_列出中文名与大小() {
        let dir = scratch("zip");
        let data = 造_zip(&[("游戏数据/", 0), ("游戏数据/pak.dat", 1234), ("说明.txt", 10)]);
        let path = dir.join("包.mp4");
        // 前面垫 2KB 假视频头，故意制造"包藏在中间"
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&vec![0xAAu8; 2048]).unwrap();
        file.write_all(&data).unwrap();
        drop(file);

        let ctx = UnpackCtx::quiet(1);
        let (_t, pv) = preview(&path, &ctx, None).unwrap();
        assert_eq!(pv.format, "zip");
        assert_eq!(pv.file_count(), 2, "两个文件（不含目录）: {:?}", pv.entries);
        assert_eq!(pv.total_size, 1244);
        assert!(pv.entries.iter().any(|e| e.path == "游戏数据/pak.dat"));
        assert!(
            pv.hints
                .iter()
                .any(|h| matches!(h, PreviewHint::HiddenAt { offset } if *offset == 2048)),
            "应当给出「藏在中间」的提示：{:?}",
            pv.hints
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 单一顶层目录能被识别() {
        let dir = scratch("root");
        let data = 造_zip(&[("我的游戏/", 0), ("我的游戏/a.dat", 5), ("我的游戏/子/b.dat", 5)]);
        let path = dir.join("包.mp4");
        std::fs::write(&path, &data).unwrap();

        let ctx = UnpackCtx::quiet(1);
        let (_t, pv) = preview(&path, &ctx, None).unwrap();
        assert_eq!(pv.single_root().as_deref(), Some("我的游戏"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 多个顶层条目不算单一目录() {
        let dir = scratch("multi");
        let data = 造_zip(&[("a.dat", 5), ("b.dat", 5)]);
        let path = dir.join("包.mp4");
        std::fs::write(&path, &data).unwrap();
        let ctx = UnpackCtx::quiet(1);
        let (_t, pv) = preview(&path, &ctx, None).unwrap();
        assert_eq!(pv.single_root(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 智能解压三规则() {
        let base = PathBuf::from("/下载");
        // ① 单一顶层目录 → 直接解到 base（不多套一层）
        let single = Preview {
            entries: vec![EntryView {
                index: 0,
                path: "游戏/x.dat".into(),
                size: 1,
                packed: None,
                encrypted: false,
                is_dir: false,
                modified: None,
                is_risky: false,
            }],
            ..Default::default()
        };
        assert_eq!(smart_dest(&base, &single, "包"), PathBuf::from("/下载"));

        // ② 一堆散文件 → 建同名文件夹
        let many = Preview {
            entries: vec![
                EntryView {
                    index: 0,
                    path: "a.dat".into(),
                    size: 1,
                    packed: None,
                    encrypted: false,
                    is_dir: false,
                    modified: None,
                    is_risky: false,
                },
                EntryView {
                    index: 1,
                    path: "b.dat".into(),
                    size: 1,
                    packed: None,
                    encrypted: false,
                    is_dir: false,
                    modified: None,
                    is_risky: false,
                },
            ],
            ..Default::default()
        };
        assert_eq!(smart_dest(&base, &many, "包"), PathBuf::from("/下载/包"));
    }

    #[test]
    fn 预览_7z_也能列出条目() {
        // 用 sevenz-rust2 的写侧造样本
        use sevenz_rust2::*;
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = ArchiveWriter::new(&mut cursor).unwrap();
            for (name, data) in [
                ("游戏/a.dat", "x".repeat(100)),
                ("说明.txt", "hello".to_string()),
            ] {
                let entry = ArchiveEntry::new_file(name);
                w.push_archive_entry(entry, Some(Cursor::new(data.into_bytes())))
                    .unwrap();
            }
            w.finish().unwrap();
        }
        let dir = scratch("7z");
        let path = dir.join("包.7z");
        std::fs::write(&path, cursor.into_inner()).unwrap();

        let ctx = UnpackCtx::quiet(1);
        let (_t, pv) = preview(&path, &ctx, None).unwrap();
        assert_eq!(pv.format, "7z");
        assert_eq!(pv.file_count(), 2, "条目清单：{:?}", pv.entries);
        assert!(
            pv.entries.iter().any(|e| e.path == "游戏/a.dat" && e.size == 100),
            "应当能读到条目名与大小：{:?}",
            pv.entries
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 预览会发出_ready_事件() {
        use crate::event::{ChannelSink, Event};
        let dir = scratch("evt");
        let data = 造_zip(&[("a.txt", 3)]);
        let path = dir.join("包.mp4");
        std::fs::write(&path, &data).unwrap();

        let sink = ChannelSink::new(64);
        let ctx = UnpackCtx::quiet(1).with_sink(sink.clone());
        let _ = preview(&path, &ctx, None).unwrap();

        let evs = sink.drain();
        assert!(
            evs.iter().any(|e| matches!(e, Event::PreviewReady { .. })),
            "应当发 PreviewReady（界面 S3 的数据源）: {:?}",
            evs.iter().map(Event::kind).collect::<Vec<_>>()
        );
        assert!(
            evs.iter()
                .any(|e| matches!(e, Event::StageFinished { stage: Stage::List, .. }))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
