// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 解压引擎 —— 把「定位」与「解码」彻底解耦
//!
//! # 设计（综合参考项目调研的结论）
//! - **我们负责定位**：自研的 `layout` / `scan` / `rescue` 层算出压缩包在文件里的**真实偏移**
//!   （这是市面工具的短板：它们靠中央目录或扩展名猜，遇到伪装包就废）
//! - **库负责解码**：成熟的 `zip` crate 负责 deflate / ZipCrypto / AES
//! - **桥接**：`io::Window` 把「文件 + 偏移」虚拟成一个干净的 ZIP 文件交给库
//!
//! # 为什么不是一条龙交给库
//! 实测：`zip` crate 自己找 EOCD 要扫完 8.7 GB（113 秒）；
//! 我们把偏移算好再喂给它只要 **69 微秒**（快约 1600 倍）。
//! 这条路线也对应 zip2 源码里那个思路：**不必自己写解压器，
//! 把定位修好，让成熟的解码器干活**。
//!
//! # 安全（对齐调研报告「对比-安全与健壮性」的 P0 项）
//! - 路径穿越：走 `safety::resolve_safe`（防 `../`、盘符、保留名、ADS）
//! - 解压炸弹：走 `safety::ExtractBudget`（压缩比 + 总量双闸）
//! - 危险文件：`safety::is_risky_executable` 只警告不阻断（用户自己决定）

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, ItemResult, Result};
use crate::io::Window;
use crate::safety::{self, ExtractBudget, Limits};
use crate::select::Selection;
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

/// 同名文件处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overwrite {
    /// 跳过已存在
    Skip,
    /// 覆盖
    Replace,
    /// 自动改名（`name (1).ext`）—— 默认，最不容易误删用户数据
    #[default]
    Rename,
}

impl Overwrite {
    /// 从配置里的字符串解析（`config.toml` 的 `output.conflict`）
    ///
    /// 认 `rename`（默认）/ `replace` / `skip`，大小写与前后空格不敏感。
    /// **认不出的值一律按 Rename** —— 宁可多出一个 `(1)` 副本，也绝不误删用户已有文件。
    ///
    /// 由来（体检 T9-P2）：这个配置项以前只在设置页里存着，引擎从来没读过，
    /// 三种策略实测输出一模一样（永远是 Rename），等于骗人。
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "replace" | "overwrite" => Overwrite::Replace,
            "skip" => Overwrite::Skip,
            _ => Overwrite::Rename,
        }
    }
}

/// 解压参数
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 密码（`None` = 不试密码；只解未加密的条目）
    pub password: Option<String>,
    /// 同名处理
    pub overwrite: Overwrite,
    /// 挑哪些条目（默认全要）—— 见 [`crate::select::Selection`]
    pub selection: Selection,
    /// 安全限值
    pub limits: Limits,
}

impl ExtractOptions {
    /// 以输出目录构造默认参数（不覆盖、不解密、全选）
    pub fn new<P: Into<PathBuf>>(dest: P) -> Self {
        Self {
            dest: dest.into(),
            password: None,
            overwrite: Overwrite::default(),
            selection: Selection::all(),
            limits: Limits::default(),
        }
    }
}

/// 一个成功写出的文件
#[derive(Debug, Clone)]
pub struct ExtractedFile {
    /// 归档内的原始名字
    pub name: String,
    /// 实际落盘路径
    pub out_path: PathBuf,
    /// 大小
    pub size: u64,
    /// 是否加密
    pub encrypted: bool,
}

/// 解压报告 —— **结构化结果，而不是一个 bool**
///
/// 对齐 7-Zip 的「结果分级」思路（`ZipHandler.cpp:1246-1251` 的多个 *_Error 标志）：
/// 部分成功也是成功，必须把「哪些没成、为什么」如实交给上层。
#[derive(Debug, Default)]
pub struct ExtractReport {
    /// 成功写出的文件
    pub files: Vec<ExtractedFile>,
    /// 被跳过的条目（名字, 原因）
    pub skipped: Vec<(String, String)>,
    /// 非致命的提示（如"解出了可执行文件"）
    pub warnings: Vec<String>,
    /// 累计写出的字节数
    pub bytes_written: u64,
    /// 是否因为限额被中止
    pub budget_hit: bool,
    /// 是否被用户取消（**已解出的文件保留**，见台账 §5.8）
    pub cancelled: bool,
    /// 索引坏了、但**在内存里打补丁救回来**的说明（`None` = 没修过索引）
    ///
    /// 为什么不只发一条事件：批量汇总要据此把任务标成「索引有错但救回来了」
    /// （`ArchiveFlags::HEADERS_ERROR`）。事件是**给人看**的，这个字段是**给机器看**的，
    /// 两者不能混（BACKEND-SPEC §6.2 那条例）。
    pub repaired: Option<String>,
    /// 落盘过程中遇到的**第一个可归类的 IO 错误**（没权限 / 空间不足 / 路径太长 / 目标不是文件夹）
    ///
    /// 为什么不靠 `skipped` 的文本判断：那文本来自系统（`Permission denied (os error 13)`），
    /// 随系统语言变化，不可靠。上层要按「用户能不能自己解决」分流，就得有个稳定的码。
    /// 用途见 `job.rs`：避免「一个都没解出来 + 有跳过」被误判成 [`ErrorCode::DataLost`]
    /// （体检 T9-P3：只读目录被报成「中间有一段数据丢了」）。
    pub io_error: Option<ErrorCode>,
}

impl ExtractReport {
    /// 记下落盘时的 IO 错误分类（只记第一个可归类的；`Internal` 不算，那是真内部问题）
    pub fn note_io_error(&mut self, e: &std::io::Error) {
        if self.io_error.is_some() {
            return;
        }
        let code = ErrorCode::from_io(e);
        if code != ErrorCode::Internal {
            self.io_error = Some(code);
        }
    }

    /// 是否一个文件都没解出来
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
    /// 是否「全部干净成功」（没有跳过、没有警告）
    ///
    /// 注意：**修过索引不算干净** —— 文件都出来了，但那是我们替它擦的屁股，
    /// 值得让上层知道（批量汇总会据此把任务标成「索引有错但救回来了」）。
    pub fn is_clean(&self) -> bool {
        self.skipped.is_empty()
            && self.warnings.is_empty()
            && !self.budget_hit
            && self.repaired.is_none()
    }
    /// 一句话摘要
    pub fn summary(&self) -> String {
        let mut s = format!(
            "解出 {} 个文件 / {:.1} MB",
            self.files.len(),
            self.bytes_written as f64 / 1_048_576.0
        );
        if !self.skipped.is_empty() {
            s += &format!("，跳过 {} 个", self.skipped.len());
        }
        if !self.warnings.is_empty() {
            s += &format!("，{} 条提示", self.warnings.len());
        }
        if self.budget_hit {
            s += "（触发安全限额，已中止）";
        }
        s
    }
}

/// 同一个密码的字节候选，按「现代工具 → 中文 Windows → 7z/老实现」排序
///
/// 由来（DESIGN §17.1 / 台账 `对比-测试构建与编码.md` 2.1.13）：
/// ZIP 的密码是**任意字节**，格式本身没有编码标准 ——
///
/// - Linux / 现代工具（7-Zip CLI、Python zipfile）：UTF-8 字节
/// - 中文 Windows 的 WinRAR / Bandizip / 好压：CP936（GBK）
/// - 7z 规范：UTF-16LE（`refs/sevenz-rust/src/encryption/password.rs:52-61`）；
///   7z 侧由 `sevenz-rust2` 自己转，这里备一份给 ZIP / RAR 的老实现
///
/// 顺序不影响正确性：错密码会被 CRC / 校验块挡住，只是白试一次；
/// 影响的是命中速度 —— 现代包占多数，所以 UTF-8 先试。
/// # 为什么同一段中文密码要试三种字节（账本 B-4，已按实测更正）
///
/// **ZIP / 7z 侧确实需要**：这两个格式里密码就是「一串任意字节」，没有编码规定。
/// 中文 Windows 上的压缩工具写的是 CP936（GBK），7z 规范还允许 UTF-16LE ——
/// 只试 UTF-8 会让这批包一律「密码错」。实测：`tests/data/A3_steg_modeA_cn_pwd.mp4`
/// （密码 `demo-password`，ZIP 内 AES-256）用 UTF-8 候选就能开（写端传的是 UTF-8），
/// 但中文 Windows 工具造的包必须靠 GBK 候选。
///
/// **RAR 侧不需要**：RAR5 的密码是 UTF-8 文本、RAR4 的 `rars` 实现直接拒绝非 UTF-8
/// （见 `extract_rar.rs` 里 `候选` 那段的实测记录）。所以 `extract_rar` **不用这个函数**，
/// 只在解 ZIP / 7z 时用。
///
/// 纯 ASCII 密码只试一次（GBK 字节与 UTF-8 相同、UTF-16LE 实践中不存在）→ 零性能退化。
/// ⚠️ 别把它「优化」成只试 UTF-8 —— 那是把中文 Windows 造的 ZIP 推回密码错。
pub fn 密码候选(pw: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(3);
    out.push(pw.as_bytes().to_vec());

    // 纯 ASCII 密码：GBK 字节与 UTF-8 完全相同，UTF-16LE 变体在实践中不存在
    // （7z 自己会转），所以不必多试 —— 保持改动前的一次尝试，零性能退化。
    if pw.is_ascii() {
        return out;
    }

    // GBK（encoding_rs 的 GBK 编码器；纯中文时字节与 CP936 完全一致）
    let (gbk, _, had_errors) = encoding_rs::GBK.encode(pw);
    if !had_errors && !gbk.is_empty() {
        out.push(gbk.into_owned());
    }

    // UTF-16LE（7z 规范；少数 ZIP 实现也这么存）
    let mut u16le = Vec::with_capacity(pw.len() * 2);
    for u in pw.encode_utf16() {
        u16le.extend_from_slice(&u.to_le_bytes());
    }
    out.push(u16le);
    out
}

/// **快速验证密码：只读第一个加密条目的开头，不解整个包**
///
/// # 为什么必须有它（三层审查 B2 的落地）
/// 试密码时如果靠「完整解压一遍看成功没」，8 GB 的包每个候选要几分钟 ——
/// 用户会以为程序死机（实测：一个 8.7 GB 的包，试一次就把整条 8.6 GB 条目解了一遍）。
///
/// 实际上 ZipCrypto 的校验字节、AES 的校验块都藏在**数据开头**，
/// 读几 KiB 就足以判断密码对不对。
pub fn verify_password_at<R: Read + Seek>(r: R, base: u64, len: u64, pw: &str) -> bool {
    use std::io::Read as _;

    let Ok(window) = Window::new(r, base, len) else {
        return false;
    };
    let Ok(mut archive) = crate::layout::zip::打开已知偏移(window) else {
        return false;
    };

    // 找一个条目试：优先加密的那个（不加密的条目用什么密码都能读，判不出对错）
    let enc_idx = (0..archive.len()).find(|i| {
        archive
            .by_index_raw(*i)
            .map(|f| f.encrypted())
            .unwrap_or(false)
    });
    let Some(idx) = enc_idx else {
        return true; // 压根没有加密条目 → 不需要密码
    };

    // 多编码逐个试（UTF-8 → GBK → UTF-16LE，见 `密码候选`）
    for cand in 密码候选(pw) {
        let Ok(mut entry) = archive.by_index_decrypt(idx, &cand) else {
            continue;
        };
        let mut buf = [0u8; 4096];
        if matches!(entry.read(&mut buf), Ok(n) if n > 0) {
            return true;
        }
    }
    false
}

/// 同名时生成不冲突的名字：`a.txt` → `a (1).txt`
fn unique_path(path: &std::path::Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let name = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let cand = parent.join(name);
        if !cand.exists() {
            return cand;
        }
    }
    path.to_path_buf()
}

/// **解压指定区域里的 ZIP**
///
/// - `r`：源（一般传 `&mut File`）
/// - `base`：ZIP 数据在源里的起始偏移（来自 `layout::zip::locate` 或 `scan`）
/// - `len`：ZIP 数据长度（从 `ZipLayout::data_len()` 拿）
///
/// 这是「不报进度、不能取消」的简单入口；要进度/取消/事件请用 [`extract_zip_at_ctx`]。
pub fn extract_zip_at<R: Read + Seek>(
    r: R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
) -> Result<ExtractReport> {
    extract_zip_at_ctx(r, base, len, opts, &UnpackCtx::quiet(0))
}

/// 把 zip crate 的条目错误翻成人话。
///
/// ★ `UnsupportedArchive` 必须单独标出来：那是「**包是好的，只是这个算法我们没实现**」
///   （zip 里的 LZMA/PPMd/XZ/Implode/Reduce/Shrink/Deflate64-raw）。
///   上层靠「不支持的压缩方法」这几个字归成 `UNSUPPORTED_METHOD`；不标的话会落到
///   `DataLost`（「中间有一段数据丢了」），用户以为包坏了去重新下载 —— 2026-11-02 审计实测。
fn 条目打不开说明(e: &zip::result::ZipError) -> String {
    match e {
        zip::result::ZipError::CompressionMethodNotSupported(id) => format!(
            "不支持的压缩方法（编号 {id}）—— 包本身是好的，用 7-Zip 或 PeaZip 能打开"
        ),
        zip::result::ZipError::UnsupportedArchive(msg) => format!(
            "不支持的压缩方法（{msg}）—— 包本身是好的，用 7-Zip 或 PeaZip 能打开"
        ),
        _ => format!("打开条目失败: {e}"),
    }
}

/// zip crate 打不开、但我们自己的定位器已经解析出中央目录时走的兜底路径（审计 A-4）。
///
/// 只有「按 local header 真能重建出条目」时才走 —— 重建不出来就老实说文件不行，
/// 免得把垃圾数据当成功产物写出来。
fn 救援兜底<R: Read + Seek>(
    r: &mut R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
    ctx: &UnpackCtx,
) -> Result<ExtractReport> {
    let end = base.saturating_add(len);
    let items = crate::rescue::rescue_by_locals(
        r,
        base,
        end,
        &crate::rescue::RescueOptions::default(),
    )?;
    if items.is_empty() {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "zip_crate_refused_and_no_local_headers")
            .context("这个包连文件头都重建不出来 —— 不是 ZIP，或者数据区已经没了"));
    }
    crate::rescue::extract_by_locals_ctx(r, base, end, opts, ctx)
}

/// 「失败就把这次新建的空目录收回来」的守卫。
///
/// # 为什么用 Drop，而不是在函数末尾写一句清理
///
/// 解压中途有很多**提前返回**：包打不开、剥壳失败、建目录失败……每一条都手动清理
/// 既容易漏、又会越加越多（实测就踩过：`open_tar_stream` 的 `?` 一跳，末尾的收尾
/// 根本执行不到，空目录照样留下）。让 `Drop` 兜住所有出口 —— 包括 panic ——
/// 只需要在「真的解出了东西」时调一次 [`keep`](Self::keep)。
///
/// 清理只删**空**目录（见 [`回收本次新建的空目录`]），所以即使判断失误也只是留下残留，
/// 不会删用户的任何数据。
pub(crate) struct 空目录守卫 {
    新建: Vec<PathBuf>,
    生效: bool,
}

impl 空目录守卫 {
    pub(crate) fn new() -> Self {
        Self {
            新建: Vec::new(),
            生效: true,
        }
    }

    /// 把记账表交给 [`建目录并记账`]
    pub(crate) fn 记账(&mut self) -> &mut Vec<PathBuf> {
        &mut self.新建
    }

    /// 已经解出了东西 → 目录结构就是用户要的，别再动它
    pub(crate) fn keep(&mut self) {
        self.生效 = false;
    }
}

impl Drop for 空目录守卫 {
    fn drop(&mut self) {
        if self.生效 && !self.新建.is_empty() {
            回收本次新建的空目录(&self.新建);
        }
    }
}

/// 建目录，顺便记一笔「这是本次新建的」—— 给失败收尾用（见 [`回收本次新建的空目录`]）。
///
/// 只记**最上层那一截**：`a/b/c` 全都不存在时只记 `a`，删的时候从 `a` 往下扫空的。
/// 记三条反而要处理「父目录还没删完」的先后顺序，容易出岔子。
pub(crate) fn 建目录并记账(p: &Path, 新建: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if p.exists() {
        return Ok(());
    }
    let mut 顶 = p;
    while let Some(parent) = 顶.parent() {
        if parent.as_os_str().is_empty() || parent.exists() {
            break;
        }
        顶 = parent;
    }
    新建.push(顶.to_path_buf());
    fs::create_dir_all(p)
}

/// 把这次顺手建的空目录收回来 —— 只删**空**的，里面有东西的一律留着。
///
/// # 为什么需要它（2026-11-04 真机测试撞出来的）
///
/// ZIP 的**文件名不加密**，所以「按条目建目录」这一步发生在密码校验**之前**
/// （目录条目本身不需要密码）。于是缺密码 / 密码错时会出现：解压报「需要解压码」失败，
/// 但用户的目标目录里已经多出一串空文件夹 —— 看着像「没解开却动了我磁盘」，
/// 也与 `CHANGELOG` 里「密码错在建目录之前就失败，不留空文件夹」这句承诺不符。
///
/// 7z / RAR 两条路天然没这问题（它们打开包时就验证密码，那时目录还没建）。
/// 实测对照：A2/A3 伪装包、`aes_secret.zip` 会留空壳；`C10_pwd_cn.7z`、`B4_rar5_cn.pwd.rar` 干净。
///
/// # 安全边界（故意写保守）
///
/// 递归只删空目录：遇到任何文件 / 符号链接就停下并保留。所以即使判断失误
/// （比如解压期间别的程序往里写了东西），最坏结果是留下残留，**绝不误删用户的东西**。
pub(crate) fn 回收本次新建的空目录(新建: &[PathBuf]) {
    fn 删空(p: &Path) {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                // 不跟随符号链接：链接指向的目录可能是用户的真实数据
                if path.is_dir() && !path.is_symlink() {
                    删空(&path);
                }
            }
        }
        // 空的才会删成功；不空就留着
        let _ = fs::remove_dir(p);
    }
    // 倒序：深的先删，浅的才有机会变空
    for d in 新建.iter().rev() {
        删空(d);
    }
}

/// 同 [`extract_zip_at`]，但带上事件 / 进度 / 取消上下文
pub fn extract_zip_at_ctx<R: Read + Seek>(
    r: R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
    ctx: &UnpackCtx,
) -> Result<ExtractReport> {
    let mut r = r;
    // 把「文件 + 偏移」虚拟成一个独立的小 ZIP 文件
    let 开包 = crate::layout::zip::打开已知偏移(
        Window::new(&mut r, base, len)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("window: {e}")))?,
    );
    let mut archive = match 开包 {
        Ok(a) => a,
        // ★ 审计 A-4：有些包的名字字段里就塞着 `PK\x06\x06` / `PK\x06\x07` 这些 ZIP64 魔数
        //   （zip2 的 `zip64_magic_in_filename_*` 就是造来考验解析器的）。`zip` crate 会被骗得
        //   去找并不存在的 ZIP64 记录然后整包拒收 —— 而 7-Zip 照解不误。
        //   这时退回「照 local header 抽数据」的救援路径。
        //   ★ 但**先让修复流程上**：A-3 那种「坏 extra / 偏移被改错」的包是修复能救的，
        //   抢在它前面走救援会少解条目（实测：2 个文件只解出 1 个）。
        Err(e) => {
            let 能修 = matches!(
                crate::repair::analyze(&mut r, base, len),
                Ok(crate::repair::RepairAnalysis::Repairable(_))
            );
            if 能修 {
                return Err(Error::new(ErrorCode::NotArchive)
                    .context(format!("zip open: {e}")));
            }
            return 救援兜底(&mut r, base, len, opts, ctx);
        }
    };

    // 先列出「打包后总大小」，给炸弹防护做基准
    let packed_total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index_raw(i).ok().map(|f| f.compressed_size()))
        .sum::<u64>()
        .max(1);
    let mut budget = ExtractBudget::new(opts.limits.clone(), packed_total);

    // 进度总量：**只统计被挑中的条目**，否则进度条永远到不了 100%
    let (raw_total, file_count) = {
        let mut bytes = 0u64;
        let mut files = 0u64;
        for i in 0..archive.len() {
            if let Ok(f) = archive.by_index_raw(i) {
                let nm = crate::layout::zip::decode_name_forced(
                    f.name_raw(),
                    false,
                    &[],
                    ctx.encoding.as_deref(),
                );
                if opts.selection.matches(i as u64 + 1, &nm, f.is_dir()) {
                    bytes += f.size();
                    files += 1;
                }
            }
        }
        (bytes, files)
    };
    ctx.set_total(raw_total, file_count);

    // 本次新建的目录（解压失败时要把它们收回去 —— 见 `空目录守卫`）
    let mut 守卫 = 空目录守卫::new();

    // T8-P5：目标位置如果已经是个文件，create_dir_all 只会给个含糊的 AlreadyExists，
    // 用户看到的是「内部错误」。这里提前判断，直接说「这是文件不是文件夹」。
    if opts.dest.exists() && !opts.dest.is_dir() {
        return Err(Error::new(ErrorCode::DestIsFile)
            .with("path", opts.dest.display().to_string()));
    }
    // 只测不写：连输出目录都不建（用户不该因为「测一下」而多出一个空文件夹）
    if !ctx.verify_only {
        建目录并记账(&opts.dest, 守卫.记账()).map_err(|e| {
            // T8-P5：目标位置是个文件 / 路径太长 / 没权限，别一律说「内部错误」
            let m = format!("mkdir dest: {e}");
            Error::from(e).context(m)
        })?;
    }

    let mut report = ExtractReport::default();

    for i in 0..archive.len() {
        // 用户取消：已解出的保留，不再往下做（取消不是错误，见台账 §5.8）
        if ctx.is_cancelled() {
            report.cancelled = true;
            break;
        }
        // 取名字（先不解码数据）
        let (name, is_dir, _size, encrypted, out_path_result, is_symlink) = {
            let f = match archive.by_index_raw(i) {
                Ok(f) => f,
                Err(e) => {
                    report
                        .skipped
                        .push((format!("#{i}"), format!("读条目失败: {e}")));
                    continue;
                }
            };
            // 用我们自己的四级回退链解码 —— 不能直接用 zip 的 `name()`：
            // 它在没有 UTF-8 标志位时会按 CP437 解，中文名会变成乱码
            let raw_name = crate::layout::zip::decode_name_forced(
                f.name_raw(),
                false,
                &[],
                ctx.encoding.as_deref(),
            );
            let is_dir = f.is_dir();
            let _size = f.size();
            let encrypted = f.encrypted();
            let safe = safety::resolve_safe(&opts.dest, &raw_name);
            // zip 没有专门的「这是符号链接」类型位，只能看 external attributes 里
            // 的 Unix mode：S_IFLNK = 0o120000。tar 有 header 类型，zip 只能这么认
            let is_symlink = f
                .unix_mode()
                .map(|m| m & 0o170000 == 0o120000)
                .unwrap_or(false);
            (raw_name, is_dir, _size, encrypted, safe, is_symlink)
        };

        // 挑选：没被勾中的条目直接跳过（不写盘、不算错，见 docs/V0.5-ENGINE.md §5）
        if !opts.selection.matches(i as u64 + 1, &name, is_dir) {
            continue;
        }

        // 路径安全检查
        let mut out_path = match out_path_result {
            Ok(p) => p,
            Err(e) => {
                report
                    .skipped
                    .push((name.clone(), format!("路径不安全被拒: {e}")));
                continue;
            }
        };

        if is_dir {
            // 断点续传：上次已经建过这个目录 → 跳
            if ctx.resume_skip(&name, &out_path, crate::resume::条目种类::目录) {
                ctx.entry_done(i as u64 + 1, &name, ItemResult::Ok, 0);
                continue;
            }
            if !ctx.verify_only && let Err(e) = 建目录并记账(&out_path, 守卫.记账()) {
                report.note_io_error(&e);
                report
                    .skipped
                    .push((name.clone(), format!("建目录失败: {e}")));
            }
            ctx.resume_record(&name, &out_path, crate::resume::条目种类::目录);
            // 目录也算一个「做完了」的条目，否则界面上的总数对不上
            ctx.entry_done(i as u64 + 1, &name, ItemResult::Ok, 0);
            continue;
        }

        // 断点续传：上次已经完整解出过这一条（且磁盘上大小对得上）→ 不重解
        let 续传种类 = if is_symlink {
            crate::resume::条目种类::链接
        } else {
            crate::resume::条目种类::文件
        };
        if ctx.resume_skip(&name, &out_path, 续传种类) {
            let 大小 = if is_symlink {
                0
            } else {
                std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0)
            };
            ctx.entry_done(i as u64 + 1, &name, ItemResult::Ok, 0);
            report.files.push(ExtractedFile {
                name,
                out_path,
                size: 大小,
                encrypted,
            });
            continue;
        }

        // 同名处理
        if out_path.exists() {
            match ctx.同名策略(opts.overwrite) {
                Overwrite::Skip => {
                    report
                        .skipped
                        .push((name.clone(), "已存在，按策略跳过".into()));
                    continue;
                }
                Overwrite::Rename => out_path = unique_path(&out_path),
                Overwrite::Replace => {}
            }
        }

        // ── 符号链接（对齐 tar：Linux 软件包的标准结构）──
        // zip 把链接目标**直接写在条目内容里**（不压缩），所以这里读出来就是目标路径。
        // 安全规则跟 tar 共用一套：只接受「相对路径 + 规范化后仍在解压目录内」，
        // 绝对路径 / 用 `..` 爬出目录的一律拒绝并说明 —— 不静默、也不假装解出来了。
        if is_symlink {
            if !ctx.verify_only && let Some(parent) = out_path.parent() {
                let _ = 建目录并记账(parent, 守卫.记账());
            }
            let 读出 = archive.by_index_raw(i).ok().and_then(|mut f| {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut buf)
                    .ok()
                    .map(|_| buf)
            });
            let Some(buf) = 读出 else {
                report
                    .skipped
                    .push((name.clone(), "链接条目读不出目标，已跳过".into()));
                ctx.entry_done(i as u64 + 1, &name, ItemResult::Skipped, 0);
                continue;
            };
            let 目标 = String::from_utf8_lossy(&buf).into_owned();
            if 目标.is_empty() {
                report
                    .skipped
                    .push((name.clone(), "链接条目没写目标，已跳过".into()));
                ctx.entry_done(i as u64 + 1, &name, ItemResult::Skipped, 0);
                continue;
            }
            if !crate::extract_stream::链接目标安全(&opts.dest, &out_path, &目标) {
                report.skipped.push((
                    name.clone(),
                    format!("链接指向「{目标}」，会跑到解压目录外，已拒绝（防逃逸）"),
                ));
                ctx.entry_done(i as u64 + 1, &name, ItemResult::Skipped, 0);
                continue;
            }
            // 同名已存在（Replace 策略或重解一遍）先删掉，否则 symlink 直接 EEXIST
            let _ = fs::remove_file(&out_path);
            // 建链接：Unix 直接建符号链接；**Windows 上普通用户建符号链接要管理员权限**，
            // 所以退回「目标是什么就复制什么」（目录建同名目录、文件复制一份），并记一条
            // SlowPath 让界面看得到 —— 不许静默丢条目。代价说清：脚本链接在 Windows 上
            // 不会保持「跟随」，但内容一定在（这也是 2026-10-29 修 Windows 编译阻塞的落点）。
            #[cfg(unix)]
            let 建链接: std::io::Result<()> = std::os::unix::fs::symlink(&目标, &out_path);
            #[cfg(not(unix))]
            let 建链接: std::io::Result<()> = {
                // 链接目标按 POSIX 语义是**相对链接所在目录**的，不是相对解压根
                //（2026-11-07 实测：`dir/link.txt → file.txt` 被拼成 `out/file.txt`，
                // 找不到目标 → 整条被跳过）。
                let 基准 = out_path.parent().unwrap_or(&opts.dest);
                let 真身 = 基准.join(&目标);
                match std::fs::metadata(&真身) {
                    Ok(m) if m.is_dir() => fs::create_dir_all(&out_path),
                    Ok(_) => fs::copy(&真身, &out_path).map(|_| ()),
                    Err(e) => Err(e),
                }
            };
            match 建链接 {
                Ok(()) => {
                    #[cfg(not(unix))]
                    ctx.diagnostic(
                        crate::event::DiagnosticCode::SlowPath,
                        format!("{name}：这个系统不支持符号链接，已按目标内容复制"),
                    );
                    ctx.resume_record(&name, &out_path, crate::resume::条目种类::链接);
                    ctx.entry_done(i as u64 + 1, &name, ItemResult::Ok, 0);
                    report.files.push(ExtractedFile {
                        name,
                        out_path,
                        size: 0,
                        encrypted,
                    });
                }
                Err(e) => {
                    report
                        .skipped
                        .push((name.clone(), format!("建链接失败: {e}")));
                    ctx.entry_done(i as u64 + 1, &name, ItemResult::Skipped, 0);
                }
            }
            continue;
        }

        if !ctx.verify_only
            && let Some(parent) = out_path.parent()
            && let Err(e) = 建目录并记账(parent, 守卫.记账())
        {
            report.note_io_error(&e);
            report
                .skipped
                .push((name.clone(), format!("建父目录失败: {e}")));
            continue;
        }

        // 危险可执行文件：警告但不阻断（用户自己决定跑不跑）
        if safety::should_warn_executable(&opts.limits, &name) {
            // 走**事件**而不是往内部列表里塞字符串：
            // ① 事件外壳才看得见（否则这条提醒等于没发）
            // ② 文案由外壳查语言包渲染（引擎不产句子）
            ctx.diagnostic(crate::event::DiagnosticCode::RiskyEntry, name.clone());
        }

        // ── 真正解码 ──
        let mut file = match if encrypted {
            match &opts.password {
                Some(pw) => {
                    // 多编码逐个试（UTF-8 → GBK → UTF-16LE）。
                    // 注意：不能把 ZipFile 带出循环（它可变借用 archive，下一次迭代
                    // 再借就会 E0499），所以先用「读一小段」验证候选，再正式打开一次。
                    let mut 命中: Option<Vec<u8>> = None;
                    let mut 最后错误 = String::new();
                    for cand in 密码候选(pw) {
                        match archive.by_index_decrypt(i, &cand) {
                            Ok(mut probe) => {
                                let mut buf = [0u8; 4096];
                                match probe.read(&mut buf) {
                                    Ok(n) if n > 0 || probe.size() == 0 => {
                                        命中 = Some(cand);
                                        break;
                                    }
                                    Ok(_) => 最后错误 = "读不到数据".to_string(),
                                    Err(e) => 最后错误 = e.to_string(),
                                }
                            }
                            Err(e) => 最后错误 = e.to_string(),
                        }
                    }
                    match 命中 {
                        Some(cand) => archive
                            .by_index_decrypt(i, &cand)
                            .map_err(|e| format!("解密失败（密码可能不对）: {e}")),
                        None => Err(format!("解密失败（密码可能不对）: {最后错误}")),
                    }
                }
                None => Err("条目已加密，但没有提供密码".to_string()),
            }
        } else {
            archive
                .by_index(i)
                .map_err(|e| 条目打不开说明(&e))
        } {
            Ok(f) => f,
            Err(msg) => {
                report.skipped.push((name.clone(), msg));
                continue;
            }
        };

        // 流式写盘 + 边写边记限额（防炸弹）
        // T9-P1/T8-P6：先写 .yshjy-part，全部写完再原子改名 ——
        // 中途被杀 / 磁盘满 / 取消时，磁盘上不会出现「顶着正式名字的半截文件」
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

        // 条目在中央目录里声明的大小：拿它给输出**提前**封顶。
        //
        // 这就是 libarchive `entry_bytes_declared` 的同一件事（见
        // `refs/tech/libarchive.md` §4）：格式读取器不一定自己管住"到底吐了多少"，
        // 上层就拿头部声明值当上限 —— 多写的丢弃、少写的按截断报错。
        //
        // 为什么必须在**写之前**封顶，而不是像以前那样写完再比对：声明 1 KB、实际
        // 吐 10 GB 的坏包/恶意包，先写满磁盘再报错等于把用户的磁盘当人质（budget
        // 只管总量，管不到"这一条明显在超发"）。封顶后多出来的字节根本不落盘。
        //
        // 声明是 0 的情况（老式「数据描述符」流式写入的包）不封顶：那时 `size()`
        // 本来就不可信，只能靠 budget 的总量限额兜底。
        let expected = file.size();
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        let mut read_err = None;
        let mut cancelled_now = false;
        loop {
            // 取消检查下沉到数据块级：8 GB 的单文件也能及时停下（三层审查 B1）
            if ctx.is_cancelled() {
                cancelled_now = true;
                break;
            }
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    // 只接受「声明范围内」的字节数
                    let 允许 = if expected > 0 {
                        (expected.saturating_sub(written)).min(n as u64) as usize
                    } else {
                        n
                    };
                    if 允许 > 0 {
                        if let Err(e) = budget.record(允许 as u64) {
                            // 「跳过 / 取消」是控制流，不是坏数据：走取消收尾（保留已解文件）
                            if e.is_cancelled() {
                                cancelled_now = true;
                            } else {
                                // ★ 以前这里只写了一句人话到 read_err，**没有留下码**，
                                //   于是上层兜底看不到真原因，把「安全限额」报成
                                //   `DataLost`（「中间有一段数据丢了」）—— 用户以为包坏了。
                                //   实测：100MB 全零文件的普通 zip 就这么被冤枉的。
                                report.budget_hit = true;
                                read_err = Some(format!("安全限额中止: {e}"));
                            }
                            break;
                        }
                        if let Err(e) =
                            std::io::Write::write_all(part.as_file(), &buf[..允许])
                        {
                            report.note_io_error(&e);
                            read_err = Some(format!("写盘失败: {e}"));
                            break;
                        }
                        written += 允许 as u64;
                        // 进度：节流由 ctx 负责（100ms 合并推送，避免刷爆外壳）
                        ctx.advance(允许 as u64, Some(&name));
                    }
                    if 允许 < n {
                        // 多写：声明之外的数据一律不要，这一条按数据错误处理
                        read_err = Some(format!(
                            "条目声明 {expected} 字节，实际数据更多 —— 已丢弃超出的 {} 字节",
                            n - 允许
                        ));
                        break;
                    }
                }
                Err(e) => {
                    read_err = Some(format!("读取失败: {e}"));
                    break;
                }
            }
        }

        if cancelled_now {
            // 取消：part 由 Drop 删掉（不留半截正式文件）；之前解好的文件全部保留
            drop(part);
            report.cancelled = true;
            break;
        }

        // 读满校验（DESIGN §17.6 / AUDIT 3-12）：声明大小 vs 实际读出。
        // 分卷拼错位时库可能「成功」返回但少读几字节 —— 不校验就是假成功。
        // （多写的分支在上面的循环里已经先一步报错了，这里只管少写。）
        if read_err.is_none() && expected > 0 && written != expected {
            read_err = Some(format!(
                "条目声明 {expected} 字节，实际读出 {written} 字节 —— 分卷可能拼错位或数据被截断"
            ));
        }

        match read_err {
            Some(msg) => {
                report.skipped.push((name.clone(), msg));
                // 清理半成品（对齐 unar「失败即删输出文件」的做法）：Drop 会把 part 删掉
                drop(part);
                // 安全限额拦下的不能标成「数据损坏」——文件是好的，是我们的闸；
                // 否则 CLI 会同时打出「数据损坏」和「文件没坏」两句自相矛盾的话。
                let 条目标记 = if report.budget_hit {
                    ItemResult::SafetyLimited
                } else {
                    ItemResult::DataError
                };
                ctx.entry_done(i as u64 + 1, &name, 条目标记, 0);
            }
            None => {
                // 原子落地：改名成功之前，正式文件名还不存在
                if let Err(e) = part.finish() {
                    report
                        .skipped
                        .push((name.clone(), format!("落盘改名失败: {e}")));
                    ctx.entry_done(i as u64 + 1, &name, ItemResult::DataError, 0);
                    continue;
                }
                report.bytes_written += written;
                // 把归档里记录的时间戳 / 权限还原到刚解出来的文件上
                // （尽力而为：设不上不该把一次成功解压变成失败）
                if !ctx.verify_only {
                    还原条目属性(&out_path, &file, &mut report);
                }
                // 断点续传：到这一步文件才算**真的完整落地**（原子改名 + 属性还原都过了）
                ctx.resume_record(&name, &out_path, crate::resume::条目种类::文件);
                ctx.entry_done(i as u64 + 1, &name, ItemResult::Ok, written);
                report.files.push(ExtractedFile {
                    name,
                    out_path,
                    size: written,
                    encrypted,
                });
            }
        }
    }

    // ── 收尾：解出了东西就解除守卫，否则 Drop 自动把空目录收回去 ──
    //
    // 判据刻意收紧（`files` 空 **且** 写入 0 字节 = 一个都没落地）：只要有任何文件成功写出，
    // 目录结构就是用户要的东西，一个都不动 —— 「半成功」（解了几个、剩下要密码）也安全。
    // 提前返回（包打不开 / 剥壳失败）不需要在这里处理，`Drop` 会兜住。
    if !report.files.is_empty() || report.bytes_written > 0 {
        守卫.keep();
    }

    Ok(report)
}

/// **带自动修复的解压**
///
/// 先按正常路径解；如果索引对不上（很多伪装包会故意改坏中央目录偏移），
/// 就让 [`crate::repair`] 分析出「错在哪几个字节」，用 [`crate::io::patch::PatchWindow`]
/// 在**内存里**改对后重试 —— 磁盘文件一个字节都不动。
///
/// 返回 `(报告, 修复说明)`；修复说明为 `None` 表示一路顺利、不需要修。
///
/// 对齐调研结论：坏包多数是「数据完好、索引写错」，修索引比重新实现解压算法划算得多。
pub fn extract_zip_auto<R: Read + Seek>(
    r: &mut R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
) -> Result<(ExtractReport, Option<String>)> {
    extract_zip_auto_ctx(r, base, len, opts, &UnpackCtx::quiet(0))
}

/// 同 [`extract_zip_auto`]，但带上事件 / 进度 / 取消上下文
pub fn extract_zip_auto_ctx<R: Read + Seek>(
    r: &mut R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
    ctx: &UnpackCtx,
) -> Result<(ExtractReport, Option<String>)> {
    // ① 先走正常路径
    match extract_zip_at_ctx(&mut *r, base, len, opts, ctx) {
        Ok(rep) => Ok((rep, None)),
        Err(first_err) => {
            // 用户取消不是「索引坏了」，别去修
            if first_err.is_cancelled() {
                return Err(first_err);
            }
            // ② 尝试分析索引问题
            let analysis = match crate::repair::analyze(r, base, len) {
                Ok(a) => a,
                Err(_) => return Err(first_err),
            };
            let plan = match analysis {
                crate::repair::RepairAnalysis::Repairable(p) => p,
                _ => return Err(first_err),
            };

            // ③ 在内存里打补丁后重试（PatchWindow 自身已把 [base,base+len) 虚拟化，
            //    所以再喂给 extract 时 base=0、len=窗口长度）
            let patched = crate::io::patch::PatchWindow::new(&mut *r, base, len, plan.patches)
                .map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("建补丁窗口失败: {e}"))
                })?;
            let mut patched = patched;
            let rep = extract_zip_at_ctx(&mut patched, 0, len, opts, ctx)?;
            Ok((rep, Some(plan.reason)))
        }
    }
}

/// 从 ZIP 条目读「时间戳 / 权限」并应用到落盘文件
///
/// 失败只记一条提示（最多 3 条，免得刷屏）—— 属性没还原上**不影响解压成功**这个结论。
fn 还原条目属性<R: std::io::Read>(
    path: &std::path::Path,
    entry: &zip::read::ZipFile<'_, R>,
    report: &mut ExtractReport,
) {
    let mut attrs = crate::restore::EntryAttrs::default();
    if let Some(dt) = entry.last_modified() {
        // ZIP 里存的是「年月日时分秒」的 DOS 格式，按**本地墙钟**看待。
        // 转成 Unix 秒时必须减掉本地偏移；否则解出来的文件时间会整体 +8 小时
        //（2026-11-07 在 Windows 上实测：源 18:50 → 解出 02:50 次日）。
        // 打包端写的是本地时间（`unix_to_civil_本地`），这样一减正好还原。
        attrs.mtime = Some(
            crate::restore::civil_to_unix(
                dt.year() as i32,
                dt.month() as u32,
                dt.day() as u32,
                dt.hour() as u32,
                dt.minute() as u32,
                dt.second() as u32,
            ) - crate::restore::本地偏移秒(),
        );
    }
    attrs.mode = entry.unix_mode();
    if attrs.is_empty() {
        return;
    }
    if let Err(e) = crate::restore::apply(path, &attrs)
        && report.warnings.len() < 3
    {
        report.warnings.push(format!("属性没还原上：{e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 重名策略从配置解析() {
        // T9-P2：设置页存的就是这三个词，引擎必须认
        assert_eq!(Overwrite::from_config("rename"), Overwrite::Rename);
        assert_eq!(Overwrite::from_config("replace"), Overwrite::Replace);
        assert_eq!(Overwrite::from_config("skip"), Overwrite::Skip);
        // 大小写 / 前后空格都算数（用户手改 config.toml 很容易带空格）
        assert_eq!(Overwrite::from_config("  REPLACE "), Overwrite::Replace);
        assert_eq!(Overwrite::from_config("Skip"), Overwrite::Skip);
        // 认不出的值一律按最安全的 Rename：宁可多一个副本，也绝不覆盖用户文件
        assert_eq!(Overwrite::from_config(""), Overwrite::Rename);
        assert_eq!(Overwrite::from_config("乱写的"), Overwrite::Rename);
        assert_eq!(Overwrite::from_config("overwrite"), Overwrite::Replace);
    }

    /// 「密码」这个中文密码的三种字节形态（2026-10-17 实测值）
    #[test]
    fn 密码候选_中文给出_utf8_gbk_utf16le() {
        let c = 密码候选("密码");
        assert_eq!(c.len(), 3, "中文密码应有 3 个候选: {c:?}");
        assert_eq!(c[0], "密码".as_bytes(), "① UTF-8");
        assert_eq!(c[1], vec![0xC3, 0xDC, 0xC2, 0xEB], "② GBK（CP936）");
        assert_eq!(c[2], vec![0xC6, 0x5B, 0x01, 0x78], "③ UTF-16LE");
    }

    /// 纯 ASCII 不许多试：改动前只试一次，现在也必须只试一次（零性能退化）
    #[test]
    fn 密码候选_ascii只有一份() {
        assert_eq!(密码候选("secret").len(), 1);
    }

    use std::io::Cursor;

    #[test]
    fn 解压后保留归档里的时间戳() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("yshjy_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 造一个 ZIP，条目时间写死成 2020-10-17 12:00:00
        let dt = zip::DateTime::from_date_and_time(2020, 10, 17, 12, 0, 0).unwrap();
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(dt);
        w.start_file("老文件.txt", opt).unwrap();
        w.write_all(b"hello").unwrap();
        let data = w.finish().unwrap().into_inner();

        let pack = dir.join("包.zip");
        std::fs::write(&pack, &data).unwrap();

        let out = dir.join("出");
        let mut f = std::fs::File::open(&pack).unwrap();
        let opts = ExtractOptions::new(&out);
        extract_zip_at_ctx(&mut f, 0, data.len() as u64, &opts, &UnpackCtx::quiet(1))
            .expect("解压应当成功");

        let md = std::fs::metadata(out.join("老文件.txt")).unwrap();
        let got = filetime::FileTime::from_last_modification_time(&md);
        assert_eq!(
            got.unix_seconds(),
            crate::restore::civil_to_unix(2020, 10, 17, 12, 0, 0),
            "解出来的文件应当保留归档里记的时间戳（而不是变成\"现在\"）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 解出来的文件内容仍然正确没有因为设属性坏掉() {
        // 属性还原是在写完之后做的 —— 确认它没有破坏文件内容/长度
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("yshjy_mtime2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let dt = zip::DateTime::from_date_and_time(2019, 3, 4, 5, 6, 7).unwrap();
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(dt);
        w.start_file("数据.bin", opt).unwrap();
        w.write_all("内容不能被破坏".as_bytes()).unwrap();
        let data = w.finish().unwrap().into_inner();

        let pack = dir.join("包.zip");
        std::fs::write(&pack, &data).unwrap();
        let out = dir.join("出");
        let mut f = std::fs::File::open(&pack).unwrap();
        extract_zip_at_ctx(
            &mut f,
            0,
            data.len() as u64,
            &ExtractOptions::new(&out),
            &UnpackCtx::quiet(1),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(out.join("数据.bin")).unwrap(),
            "内容不能被破坏".as_bytes()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 造一个内存 ZIP（含中文名 + 目录）
    fn make_zip() -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.add_directory("sub/", opt).unwrap();
        w.start_file("sub/中文.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, "hello 中文 content".as_bytes()).unwrap();
        w.start_file("plain.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, b"plain").unwrap();
        w.finish().unwrap().into_inner()
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// 造一个「带加密条目」的内存 ZIP（目录条目也加密），用于密码相关的回归测试
    fn make_加密zip(密码: &str) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let base = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let 加密 = base.with_aes_encryption(zip::AesMode::Aes256, 密码);
        w.add_directory("secret/", 加密).unwrap();
        w.start_file("secret/机密.txt", 加密).unwrap();
        std::io::Write::write_all(&mut w, b"top secret").unwrap();
        w.finish().unwrap().into_inner()
    }

    /// 造一个「明文 + 加密」混合的 ZIP：验证「半成功」时目录结构不能被误删
    fn make_混合zip(密码: &str) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let base = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let 加密 = base.with_aes_encryption(zip::AesMode::Aes256, 密码);
        w.add_directory("sub/", base).unwrap();
        w.start_file("sub/说明.txt", base).unwrap();
        std::io::Write::write_all(&mut w, b"plain note").unwrap();
        w.start_file("sub/秘密.txt", 加密).unwrap();
        std::io::Write::write_all(&mut w, b"secret").unwrap();
        w.finish().unwrap().into_inner()
    }

    /// 回归（2026-11-04 真机测试撞出来的）：缺密码解压失败，不该在目标目录留下空文件夹。
    ///
    /// 为什么以前会漏：ZIP 的**文件名不加密**，所以「按目录条目建目录」发生在密码校验之前
    /// （目录条目本身不需要密码）。缺密码 / 密码错时，目标目录里就剩下一串空壳，
    /// 看着像「没解开却动了我磁盘」，也违背了 CHANGELOG 里「不留空文件夹」的承诺。
    ///
    /// 反向验证：把 `回收本次新建的空目录` 那次调用注释掉 → 本测试当场红。
    #[test]
    fn 缺密码失败后不留空目录() {
        let data = make_加密zip("p@ss");
        let len = data.len() as u64;
        let dest = scratch_dir("pwd_no_leak");

        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest); // 刻意不给密码
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert!(rep.files.is_empty(), "没密码不该解出文件: {:?}", rep.files);
        assert_eq!(rep.bytes_written, 0);
        let 剩余: Vec<String> = fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            剩余.is_empty(),
            "解压失败后目标目录必须干净，实际残留: {剩余:?}"
        );
        let _ = fs::remove_dir_all(&dest);
    }

    /// 反向边界：**半成功**（明文解出来了、加密的跳过）时，目录结构和已解文件一个都不能动。
    ///
    /// 这条防的是「清理过头」—— 收尾逻辑只看 `files.is_empty() && bytes_written == 0`，
    /// 只要有任意一个文件落地就不清理。改成「只要有 skipped 就清理」会让这条当场红。
    #[test]
    fn 半成功时保留已解出的文件与目录() {
        let data = make_混合zip("p@ss");
        let len = data.len() as u64;
        let dest = scratch_dir("half_ok");

        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest); // 不给密码：明文能解，加密的跳过
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert_eq!(rep.files.len(), 1, "明文那条该解出来: {:?}", rep.files);
        assert_eq!(rep.files[0].name, "sub/说明.txt");
        assert!(!rep.skipped.is_empty(), "加密那条该被跳过并说明原因");
        let 已解 = dest.join("sub/说明.txt");
        assert!(已解.is_file(), "解出来的文件必须还在: {}", 已解.display());
        assert_eq!(fs::read_to_string(&已解).unwrap(), "plain note");
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 解压内存_zip_到目录() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("basic");

        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert_eq!(rep.files.len(), 2, "应解出 2 个文件，实际: {:?}", rep.files);
        assert_eq!(rep.skipped.len(), 0, "不该有跳过: {:?}", rep.skipped);
        let names: Vec<_> = rep.files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"sub/中文.txt"), "中文名要正确: {names:?}");
        assert!(names.contains(&"plain.txt"));
        // 内容对
        let txt = fs::read_to_string(dest.join("sub/中文.txt")).unwrap();
        assert_eq!(txt, "hello 中文 content");
        assert!(rep.bytes_written > 0);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 偏移窗口_能从伪装包中间解出() {
        let data = make_zip();
        // 前面塞 3KB 假视频头，后面塞 2KB 垃圾
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\x00\x00\x00\x18ftypmp42");
        buf.extend(std::iter::repeat_n(0xAAu8, 3000 - 8));
        let base = buf.len() as u64;
        let len = data.len() as u64;
        buf.extend_from_slice(&data);
        buf.extend(std::iter::repeat_n(0u8, 2000));

        let dest = scratch_dir("offset");
        let mut cur = Cursor::new(buf);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, base, len, &opts).unwrap();
        assert_eq!(rep.files.len(), 2);
        assert!(rep.is_clean());
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 路径穿越被拒() {
        // 手工造一个带 ../ 的 ZIP
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        w.start_file("../../etc/evil.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, b"pwned").unwrap();
        let data = w.finish().unwrap().into_inner();
        let len = data.len() as u64;

        let dest = scratch_dir("slip");
        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert!(rep.files.is_empty(), "路径穿越必须被挡住");
        assert_eq!(rep.skipped.len(), 1);
        assert!(
            rep.skipped[0].1.contains("不安全") || rep.skipped[0].1.contains("拒"),
            "跳过原因应说明是安全问题: {:?}",
            rep.skipped
        );
        // 确认宿主机上没多出文件
        assert!(!PathBuf::from("/etc/evil.txt").exists());
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 选择性提取只解指定条目() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("select");
        let mut cur = Cursor::new(data);
        let mut opts = ExtractOptions::new(&dest);
        opts.selection = Selection::matching("plain.txt");
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();
        assert_eq!(rep.files.len(), 1);
        assert_eq!(rep.files[0].name, "plain.txt");
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 同名按策略处理() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("dupe");
        // 先解一次
        let mut c1 = Cursor::new(data.clone());
        extract_zip_at(&mut c1, 0, len, &ExtractOptions::new(&dest)).unwrap();
        // 再解一次：Rename 应产生 "plain (1).txt"
        let mut c2 = Cursor::new(data);
        let rep = extract_zip_at(&mut c2, 0, len, &ExtractOptions::new(&dest)).unwrap();
        let names: Vec<_> = rep
            .files
            .iter()
            .filter_map(|f| f.out_path.file_name())
            .collect();
        let joined = format!("{names:?}");
        assert!(joined.contains("(1)"), "自动改名应产生 (1): {joined}");
        let _ = fs::remove_dir_all(&dest);
    }

    /// 用真实加密样本（zig -P 造的 ZipCrypto / 7z AES256 造的 AES）验证解密通路
    fn 试加密样本(path: &str, pw: &str, tag: &str) {
        if !std::path::Path::new(path).exists() {
            eprintln!("样本 {path} 不存在，跳过");
            return;
        }
        let data = fs::read(path).unwrap();
        let len = data.len() as u64;
        let dest = scratch_dir(tag);

        // ① 不给密码 → 全部跳过，且原因要说清是"缺密码"，不能崩
        let mut c1 = Cursor::new(data.clone());
        let rep = extract_zip_at(&mut c1, 0, len, &ExtractOptions::new(&dest)).unwrap();
        assert!(
            rep.files.is_empty(),
            "{path}: 没密码不该解出东西: {:?}",
            rep.files
        );
        assert_eq!(
            rep.skipped.len(),
            2,
            "{path}: 两个条目都该被跳过: {:?}",
            rep.skipped
        );
        assert!(
            rep.skipped.iter().all(|(_, why)| why.contains("密码")),
            "{path}: 跳过原因必须说清是缺密码: {:?}",
            rep.skipped
        );

        // ② 给对密码 → 全部解出，内容必须对
        let mut c2 = Cursor::new(data);
        let mut opts = ExtractOptions::new(&dest);
        opts.password = Some(pw.into());
        let rep2 = extract_zip_at(&mut c2, 0, len, &opts).unwrap();
        assert_eq!(
            rep2.files.len(),
            2,
            "{path}: 给对密码应解出 2 个: {:?}",
            rep2.skipped
        );
        assert_eq!(
            fs::read_to_string(dest.join("locked.txt")).unwrap(),
            "top secret content"
        );
        assert_eq!(
            fs::read_to_string(dest.join("plain.txt")).unwrap(),
            "not secret"
        );

        // ③ 给错密码 → 不能崩，要如实报告
        let data3 = fs::read(path).unwrap();
        let mut c3 = Cursor::new(data3);
        let mut opts3 = ExtractOptions::new(&dest);
        opts3.password = Some("wrong-password".into());
        let rep3 = extract_zip_at(&mut c3, 0, len, &opts3).unwrap();
        assert!(
            rep3.files.is_empty(),
            "{path}: 错密码不该解出: {:?}",
            rep3.files
        );
        assert!(!rep3.skipped.is_empty(), "{path}: 错密码应如实报告失败");

        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 快速验证密码只读开头不解整包() {
        // 用真实加密样本（zipcrypto）验证：读 4 KiB 就能判对错
        let path = "tests/data/zipcrypto_secret.zip";
        if !std::path::Path::new(path).exists() {
            eprintln!("样本 {path} 不存在，跳过");
            return;
        }
        let data = fs::read(path).unwrap();
        let len = data.len() as u64;
        assert!(
            verify_password_at(Cursor::new(data.clone()), 0, len, "secret"),
            "正确密码应当通过"
        );
        assert!(
            !verify_password_at(Cursor::new(data), 0, len, "不是这个密码"),
            "错密码应当立刻被拒（不该等整包解完）"
        );

        // 没有加密条目的包：不需要密码，任何输入都算通过（调用方据此跳过试密码）
        let plain = make_zip();
        let plen = plain.len() as u64;
        assert!(verify_password_at(Cursor::new(plain), 0, plen, "随便"));
    }

    #[test]
    fn zipcrypto_加密包可解() {
        试加密样本("tests/data/zipcrypto_secret.zip", "secret", "enc_zipcrypto");
    }

    #[test]
    fn aes_加密包可解() {
        试加密样本("tests/data/aes_secret.zip", "secret", "enc_aes");
    }

    #[test]
    fn 坏extra字段的包也能解() {
        // 参考样本（来自 zip2 的 extended_timestamp_bad.zip）：
        // CD 条目 test.txt 的 extra 链尺寸字段写坏 —— 0x5455(UT) 后面 size=0，
        // 再往后的字节全被当成下一个 field 的 tag/size（tag=0x1103 size=13121，越界）。
        //
        // 现状（2026-11-02 审计 A-3）：EOCD 层面自洽 → repair::analyze 判「不用修」→
        // zip crate 打不开 → 报「这不是压缩包」，而 7z / unzip 都能正常解 2 个文件。
        // 期望：坏 extra 不该让整个包判死，内容照解。
        let path = format!(
            "{}/tests/data/extended_timestamp_bad.zip",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut f = std::fs::File::open(&path).unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        let dest = scratch_dir("bad_extra");
        let opts = ExtractOptions::new(&dest);

        let (rep, 修复说明) = extract_zip_auto(&mut f, 0, len, &opts)
            .expect("坏 extra 字段的包应该能解（忽略坏字段，别把好包判成「不是压缩包」）");
        eprintln!("修复说明: {修复说明:?}");

        assert_eq!(
            rep.files.len(),
            2,
            "应解出 2 个文件，实际: {:?}，跳过: {:?}",
            rep.files.iter().map(|f| &f.name).collect::<Vec<_>>(),
            rep.skipped
        );
        let txt = std::fs::read_to_string(dest.join("mimetype")).unwrap();
        assert_eq!(txt.len(), 39, "mimetype 的 39 字节要完整解出");
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn 自动修复_偏移被改错的包也能解() {
        // 造一个"数据好、但 EOCD 里 CD 偏移被改错"的包（伪装包常见手法）
        let data = b"repair me!";
        let crc = crate::scan::crc32(data);
        let mut z = Vec::new();
        z.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&9u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"fixed.txt");
        z.extend_from_slice(data);
        let cd_start = z.len() as u64;
        z.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        // 版本(2)+需求版本(2)+标志(2)+方法(2)+时间(2)+日期(2) = 12 字节
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&9u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(b"fixed.txt");
        let cd_size = z.len() as u64 - cd_start;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&(cd_size as u32).to_le_bytes());
        z.extend_from_slice(&12345u32.to_le_bytes()); // ← 故意写错的偏移
        z.extend_from_slice(&0u16.to_le_bytes());

        let len = z.len() as u64;
        let dest = scratch_dir("repair");
        let mut cur = Cursor::new(z);
        let rep = extract_zip_auto(&mut cur, 0, len, &ExtractOptions::new(&dest)).unwrap();
        assert!(rep.1.is_some(), "应当报告发生了修复");
        assert_eq!(
            rep.0.files.len(),
            1,
            "修复后应解出文件: {:?}",
            rep.0.skipped
        );
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 挑选支持目录前缀与序号() {
        // 旧的「目录前缀」语义现在用 `sub/**` 表达
        let by_dir = Selection::matching("sub/**");
        assert!(by_dir.matches(0, "sub/a.txt", false));
        assert!(by_dir.matches(1, "sub/deep/b.txt", false));
        assert!(!by_dir.matches(2, "other/c.txt", false));

        let by_index = Selection::only(vec![1, 3]);
        assert!(by_index.matches(1, "任意", false), "序号从 1 开始（界面上看到的号）");
        assert!(!by_index.matches(2, "任意", false));
        assert!(by_index.matches(3, "任意", true));

        assert!(Selection::all().matches(9, "任意", true), "默认全都要");
    }

    #[test]
    fn 解压时上报进度与条目事件() {
        use crate::ctx::UnpackCtx;
        use crate::event::{ChannelSink, Event};

        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("progress_ctx");
        let sink = ChannelSink::new(512);
        let ctx = UnpackCtx::quiet(1).with_sink(sink.clone());

        let mut cur = Cursor::new(data);
        let rep = extract_zip_at_ctx(&mut cur, 0, len, &ExtractOptions::new(&dest), &ctx).unwrap();
        assert_eq!(rep.files.len(), 2);
        assert!(!rep.cancelled);

        let evs = sink.drain();
        let kinds: Vec<&str> = evs.iter().map(Event::kind).collect();
        assert!(
            kinds.contains(&"ENTRY_FINISHED"),
            "应当有条目级事件（S7/S8 的明细来源）: {kinds:?}"
        );
        assert!(kinds.contains(&"EXTRACT_PROGRESS"), "应当有进度事件");

        // 目录 + 2 个文件 = 3 个条目，进度总量必须是 3，否则界面上的进度永远到不了头
        assert_eq!(ctx.progress.view().done_files, 3);
        assert_eq!(ctx.progress.view().total_files, 3);
        assert!(ctx.progress.view().done_bytes > 0);
        assert_eq!(
            ctx.progress.view().done_bytes,
            ctx.progress.view().total_bytes,
            "解完了就该是 100%"
        );
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 挑选后进度总量只算被选中的() {
        use crate::ctx::UnpackCtx;

        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("progress_selected");
        let ctx = UnpackCtx::quiet(1);
        let mut opts = ExtractOptions::new(&dest);
        opts.selection = Selection::matching("plain.txt");

        let mut cur = Cursor::new(data);
        let rep = extract_zip_at_ctx(&mut cur, 0, len, &opts, &ctx).unwrap();
        assert_eq!(rep.files.len(), 1);
        let v = ctx.progress.view();
        assert_eq!(v.total_files, 1, "只选了 1 个条目");
        assert_eq!(v.done_files, 1);
        assert_eq!(v.done_bytes, v.total_bytes);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 取消后保留已解出的文件() {
        use crate::ctx::UnpackCtx;
        use crate::event::{Event, EventSink};
        use crate::progress::CancelToken;

        /// 收到第一个**真实文件**完成事件就取消（模拟用户点「取消」）
        ///
        /// 注意要跳过目录条目（`bytes == 0`）—— 否则解压刚开始就被取消掉。
        struct 取消于首个文件(CancelToken);
        impl EventSink for 取消于首个文件 {
            fn emit(&self, ev: &Event) {
                if matches!(ev, Event::EntryFinished { bytes, .. } if *bytes > 0) {
                    self.0.cancel_with("测试：解出第一个文件之后取消");
                }
            }
        }

        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("cancel_ctx");
        let cancel = CancelToken::new();
        let ctx = UnpackCtx::quiet(1)
            .with_sink(std::sync::Arc::new(取消于首个文件(cancel.clone())))
            .with_cancel(cancel);

        let mut cur = Cursor::new(data);
        let rep = extract_zip_at_ctx(&mut cur, 0, len, &ExtractOptions::new(&dest), &ctx).unwrap();

        assert!(rep.cancelled, "应当标记为已取消");
        assert!(
            !rep.files.is_empty() && rep.files.len() < 2,
            "取消不是失败：已解出的要留下，实际 {:?}",
            rep.files
        );
        assert!(rep.skipped.is_empty(), "取消不该产生「跳过」的错误记录");
        // 已解出的文件真的在盘上
        for f in &rep.files {
            assert!(f.out_path.exists(), "{:?} 应当还在", f.out_path);
        }
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 一上来就取消时一个都不解() {
        use crate::ctx::UnpackCtx;
        use crate::progress::CancelToken;

        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("cancel_first");
        let cancel = CancelToken::new();
        cancel.cancel();
        let ctx = UnpackCtx::quiet(1).with_cancel(cancel);

        let mut cur = Cursor::new(data);
        let rep = extract_zip_at_ctx(&mut cur, 0, len, &ExtractOptions::new(&dest), &ctx).unwrap();
        assert!(rep.cancelled);
        assert!(rep.files.is_empty());
        let _ = fs::remove_dir_all(&dest);
    }
}
