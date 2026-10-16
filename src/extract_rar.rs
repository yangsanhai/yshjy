//! RAR 解压（含加密）
//!
//! # 关于体积的诚实说明
//! 底层库 `rars` 目前只提供**全量内存读**接口（`ArchiveReader::read(&[u8])`），
//! 不像 ZIP / 7z 那样能流式处理。所以本模块设置了一个**大小上限**：
//! 超过 [`MAX_SIZE`] 的 RAR 会直接拒绝并给出提示，避免把内存吃光。
//!
//! 对超大 RAR 的建议：先用官方 `unrar`（非自由许可，不随我们分发）解开。
//! 我们的目标是"**别人的工具打不开的伪装包**"，这类包里 RAR 占比很小。
//!
//! # 为什么 RAR 解出的文件没有原始时间戳
//!
//! 本模块走的是 `rars` 的**高层解压 API**（直接把数据流给你），拿不到 `FileHeader`，
//! 也就读不到里面的 `file_time`。要支持得改成手工遍历 RAR 头 —— 工程不小，
//! 而这类包里 RAR 占比很低（ZIP / TAR 的时间戳已经支持）。
//! 这是**有意的取舍**，不是漏做。
//!
//! # 许可
//! RAR 的压缩算法是专有的，`rars` 是 Apache-2.0 的**纯 Rust 读取实现** ——
//! 我们只做解压、不生成 RAR，符合许可边界。

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, ItemResult, Result};
use crate::safety::{self, Limits};
use crate::select::Selection;
use std::fs;
use std::path::{Path, PathBuf};

/// RAR 全量读入内存的上限（256 MiB）—— 超过就拒绝，避免 OOM
pub const MAX_SIZE: u64 = 256 * 1024 * 1024;

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
    let size = fs::metadata(path)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("读文件信息失败: {e}")))?
        .len();
    if size > MAX_SIZE {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "rar_too_large_for_memory")
            .with("size", size)
            .with("limit", MAX_SIZE)
            .context(format!(
                "这个 RAR 有 {:.0} MB，超过我们目前能处理的上限（{} MB）。\
                 底层库还不支持流式读取 RAR —— 想处理超大 RAR 请用其它工具。",
                size as f64 / 1_048_576.0,
                MAX_SIZE / 1_048_576
            )));
    }
    let data = fs::read(path)
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
pub fn extract_rar_ctx(
    data: &[u8],
    opts: &ExtractRarOptions,
    ctx: &UnpackCtx,
) -> Result<crate::extract::ExtractReport> {
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    let archive = rars::ArchiveReader::read(data).map_err(|e| {
        Error::new(ErrorCode::NotArchive).context(format!("不是有效的 RAR（或版本不支持）: {e}"))
    })?;

    fs::create_dir_all(&opts.dest)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("建输出目录失败: {e}")))?;

    let pw_bytes = opts.password.as_ref().map(|p| p.as_bytes().to_vec());
    let dest = opts.dest.clone();
    let overwrite = opts.overwrite;
    let limits = opts.limits.clone();
    let has_pw = opts.password.is_some();
    let selection = opts.selection.clone();
    // RAR 回调不给条目序号，我们自己数（预览→解压的挑选锚点就靠它）
    let idx_cell = Rc::new(std::cell::Cell::new(0u64));
    let idx_ref = idx_cell.clone();
    // 上下文里全是 Arc / 可克隆量，放进回调没有生命周期顾虑
    // （外层仍用借用版的 ctx，闭包里用这份 owned 的）
    let ctx_owned = ctx.clone();

    // ── 先验证密码 ──
    // 重要：`rars` 在解密失败时**不报错**，只是静默不写数据（会产出空文件）。
    // 所以必须先主动 test 一遍，否则用户会得到一堆 0 字节的"成功"产物。
    if let Err(e) = archive.test(pw_bytes.as_deref()) {
        let msg = e.to_string();
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
    // 解压总量预算（以"内存中的压缩数据大小"为分母）
    let budget = Rc::new(RefCell::new(crate::safety::ExtractBudget::new(
        limits,
        data.len().max(1) as u64,
    )));

    let rep2 = report.clone();
    let bud2 = budget.clone();

    let res = archive.extract_to(pw_bytes.as_deref(), move |meta| {
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
            let _ = fs::create_dir_all(&dir);
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
        if out_path.exists() {
            match overwrite {
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
        if let Some(parent) = out_path.parent()
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

        let f = match fs::File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                rep2.borrow_mut()
                    .skipped
                    .push((name.clone(), format!("创建失败: {e}")));
                return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
            }
        };

        // 包一层：边写边记限额与统计
        Ok(Box::new(CountingWriter {
            inner: f,
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
    });

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
    final_report.files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(final_report)
}

/// 带限额与统计的 Writer：库每写一段，我们就记账
struct CountingWriter {
    inner: fs::File,
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
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        self.ctx.advance(n as u64, Some(&self.name));
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Drop for CountingWriter {
    fn drop(&mut self) {
        // 取消：删掉写了一半的这个文件（之前解好的全部保留）
        if self.cancelled {
            let _ = fs::remove_file(&self.out_path);
            return;
        }
        // 成功写完才登记
        let mut r = self.report.borrow_mut();
        r.bytes_written += self.written;
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

    #[test]
    fn 超大的_rar_会被明确拒绝而不是吃光内存() {
        let dir = scratch("big");
        let p = dir.join("huge.rar");
        let f = fs::File::create(&p).unwrap();
        f.set_len(MAX_SIZE + 1).unwrap();
        drop(f);

        let opts = ExtractRarOptions::new(dir.join("out"));
        let r = extract_rar_file(&p, &opts);
        assert!(r.is_err(), "超大 RAR 应当被拒绝");
        let msg = format!("{}", r.unwrap_err());
        assert!(
            msg.contains("上限") || msg.contains("MB"),
            "错误信息要说清原因: {msg}"
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
}
