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
use crate::safety::Limits;

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
/// 优先级：任务自带的目录 > 批量选项里的 `-o` > 「去掉外壳扩展名的同名文件夹」
///
/// > 注：UI-SPEC §9.2 的「智能解压三规则」（单条目直接解到当前目录 / 重名加 `(2)`）
/// > 需要先预览才知道条目数，v0.5 暂不启用，留 v0.6 与 GUI 一起做。
pub fn resolve_dest(job: &Job, opts: &BatchOptions) -> PathBuf {
    if let Some(d) = &job.dest {
        return d.clone();
    }
    match &opts.dest {
        Some(d) => d.clone(),
        None => PathBuf::from(output_dir_name(&job.input)),
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
    let dest = resolve_dest(job, opts);

    let mut stats = JobStats {
        dest: Some(dest.clone()),
        ..Default::default()
    };

    let outcome = match identify(&job.input, &ctx) {
        Err(e) => {
            ctx.diagnostic(DiagnosticCode::PartialData, format!("identify: {e}"));
            Outcome::from_error(&e)
        }
        Ok(target) => {
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
            let (rep, repair_note) =
                crate::extract::extract_zip_auto_ctx(&mut f, *base, *len, &eopts, ctx)?;
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
            crate::extract7z::extract_7z_volumes_ctx(volumes, &eopts, ctx)
        }
        Target::Rar => {
            let mut eopts = ExtractRarOptions::new(dest);
            eopts.password = password;
            eopts.selection = opts.selection.clone();
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
                    &Limits::default(),
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
    let hints = crate::password_hint::find_hints_near(&job.input);
    if hints.is_empty() {
        return None;
    }

    // 只有 ZIP 能低成本地先试一下；其它格式（7z/rar 每次候选 121ms 起）先取第一个候选
    let Target::Zip { base, len } = target else {
        let first = hints.first()?;
        ctx.diagnostic(
            DiagnosticCode::PasswordFromNotes,
            format!("candidate from {}", first.source.display()),
        );
        return Some(first.password.clone());
    };

    // 逐个候选在临时目录里静默试（7z 每候选就要 121ms，所以只试前 3 个）
    for the_hint in hints.iter().take(3) {
        if ctx.is_cancelled() {
            break;
        }
        let tmp = std::env::temp_dir().join(format!("yshjy-probe-{}-{}", std::process::id(), job.id));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut eopts = ExtractOptions::new(&tmp);
        eopts.password = Some(the_hint.password.clone());
        let ok = match std::fs::File::open(&job.input) {
            Ok(mut f) => matches!(
                crate::extract::extract_zip_at(&mut f, *base, *len, &eopts),
                Ok(rep) if !rep.files.is_empty()
            ),
            Err(_) => false,
        };
        let _ = std::fs::remove_dir_all(&tmp);
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
    fn 挑选只解勾中的条目() {
        let dir = scratch("select");
        let p = fake_mp4_zip(&dir);
        let out = scratch("select_out");

        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let opts = BatchOptions {
            dest: Some(out.clone()),
            selection: crate::select::Selection::matching("*.txt"),
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
