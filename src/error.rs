// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 错误码体系（三层结果模型）
//!
//! # 设计原则（ARCHITECTURE.md §7 + 三层审查修正 1）
//!
//! **引擎只产出「错误码 + 结构化参数」，绝不产出面向用户的中文句子。**
//!
//! 渲染由外壳负责：
//! - CLI → 终端文案（中文白话）
//! - GUI → 界面组件（图标 + 标题 + 正文 + 按钮）
//!
//! 这样做的好处：引擎保持纯净可测；以后加英文只改外壳；手机端不必通过 FFI 传大段中文。
//!
//! # 三层结果模型（v0.5 补齐，见 `docs/V0.5-ENGINE.md` §4）
//!
//! 以前只有「一次操作一个错误码」，回答不了「这个包有毛病但能解出 340/351 个文件」这类问题。
//! 仿 7-Zip 的三层（台账 `7zip-工程层.md` §5.2），但去掉它的字符串通道：
//!
//! | 层 | 类型 | 回答的问题 |
//! |----|------|-----------|
//! | 层1 调用级 | [`ErrorCode`] | 这次调用成了吗？ |
//! | 层2 归档级 | [`ArchiveFlags`] | 这个包有什么毛病？（位图，可同时多个） |
//! | 层3 条目级 | [`ItemResult`] | 第 47 个文件对不对？ |
//!
//! [`Outcome`] 把三层收敛成「这次任务算成功还是失败」的结论，并且
//! **退出码只在引擎里算一次**（台账 §7.6：7-Zip 让两个壳各算一份，结果行为不一致）。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 统一结果类型
pub type Result<T> = std::result::Result<T, Error>;

/// 对外退出码（唯一真源）
///
/// CLI 与 GUI 都从这里取，**不要各自硬编码**（台账 §7.6 的反面案例：7-Zip 的两个壳
/// 各算一份退出码，行为不一致）。与 v0.4 的对外契约保持兼容。
pub mod exit_code {
    /// 成功
    pub const OK: i32 = 0;
    /// 一般错误（路径不对、没空间、没权限、内部错误）
    pub const ERROR: i32 = 1;
    /// 用法错误（参数不对）
    pub const USAGE: i32 = 2;
    /// 不是压缩包
    pub const NOT_ARCHIVE: i32 = 3;
    /// 密码问题（需要密码 / 密码不对）
    pub const PASSWORD: i32 = 4;
    /// 部分成功（如救出 340/351 个文件）
    pub const PARTIAL: i32 = 5;
    /// 用户取消（SIGINT 惯例）
    pub const CANCELLED: i32 = 130;
}

/// 稳定的错误码（外壳据此渲染文案与界面，见 docs/UI-SPEC.md §10）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    // ── 用户可自行解决 ──
    /// 路径不存在
    PathNotFound,
    /// 参数不合法
    InvalidArgument,
    /// 磁盘空间不足
    NotEnoughSpace,
    /// 没有写入权限
    PermissionDenied,
    /// 路径太长（超过文件系统上限，常见于超长文件名或深层目录）
    PathTooLong,
    /// 目标位置是个文件，不是文件夹
    DestIsFile,
    /// **安全限额**：解出来的体积超过压缩包的 N 倍（默认 100 倍），或总量超过单次上限
    ///
    /// ★ 这**不是**「包坏了」。以前这条和真正的数据损坏共用 `DataLost`，
    /// 于是用户看到「中间有一段数据丢了」，跑去重新下载、换软件 —— 而包其实完好。
    /// 实测：一个 100 MB 全零文件的普通 zip（`unzip` 解得很干净）压成 95 KB，
    /// 压缩比约 1028:1，被这道闸拦下并报成数据损坏。
    ///
    /// 真实压缩炸弹是 **GB 级**；但大日志 / CSV / 稀疏磁盘镜像 / 数据库导出的
    /// 压缩比轻松过百，也走这条，所以文案必须说清「这是限额，不是损坏」。
    /// 参考：7-Zip / PeaZip / Ark / libarchive 都**没有**这道闸（只有磁盘空间预检），
    /// 我们多这一层是为了防炸弹，代价就是必须解释清楚。
    SafetyLimit,

    // ── 文件本身的问题 ──
    /// 需要密码
    NeedPassword,
    /// 密码错误
    WrongPassword,
    /// **头加密**：连文件清单都加密了（7z `-mhe=on` 这类）——
    /// 没密码时连「里面有哪几个文件」都读不出来，与「内容加密、清单可读」是两种情况，
    /// 文案必须分开，否则用户会以为「我给了密码怎么还不行」。
    HeadEncrypted,
    /// **不支持的压缩方法**：包是好的，但用了我们没实现的算法
    /// （zip 里的 LZMA/PPMd/XZ/Implode/Reduce/Shrink/Deflate64-raw，
    /// 7z 里的 zstdmt-brotli / zstdmt-lz4 等）。
    ///
    /// ★ 必须与 `DataLost` / `NotArchive` 分开：报「中间有一段数据丢了」会让人
    /// 以为包坏了去重新下载、换软件 —— 而用 7-Zip 打开就正常。
    /// 2026-11-02 审计实测：zip2 的 8 个样本 + sevenz-rust 的 2 个样本全被谎报。
    UnsupportedMethod,
    /// **包里有不安全的名字**：条目名含控制字符、绝对路径、`..` 穿越等。
    /// 我们**故意不写出来**（写出去就是安全漏洞），但这不等于包坏了 ——
    /// 以前落到 `DataLost`（「中间有一段数据丢了」），用户会以为数据丢了去重下。
    /// 2026-11-02 审计 A-4：zip2 的 `zip64_magic_in_filename_*` 正是这种包。
    UnsafePath,
    /// 既不是压缩包，也没藏着压缩包
    NotArchive,
    /// 文件不完整（被截断）
    Truncated,
    /// 缺少分卷
    MissingVolume,
    /// 中间有数据丢失（只能救援）
    DataLost,
    /// 加密了但没有密码
    EncryptedUnbreakable,
    /// 封面视频本身损坏（不影响解压）
    VideoUnplayable,

    // ── 用户取消（不是错误，见台账 §5.8）──
    /// 用户主动取消
    Cancelled,

    // ── 内部问题 ──
    /// 程序内部错误
    Internal,
}

impl ErrorCode {
    /// 把 IO 错误归类成用户看得懂的码
    ///
    /// 由来（体检 T9-P3 / T8-P5）：以前所有 IO 错误都被 `From<io::Error>` 折成
    /// [`ErrorCode::Internal`]，于是「目录不让写」「路径太长」「目标是个文件」全都显示
    /// 成「内部错误」；更糟的是「一个都没解出来 + 有跳过记录」会被上层判成
    /// [`ErrorCode::DataLost`]（「中间有一段数据丢了」），吓得用户以为包坏了。
    ///
    /// 这里只做**分类**，不做文案 —— 文案由外壳按码查语言包（D21/N1 架构）。
    /// 用 `ErrorKind`（而非 errno）判断，Windows / Android 同样成立。
    pub fn from_io(e: &std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        match e.kind() {
            // 用户自己能解决的四类
            K::PermissionDenied => ErrorCode::PermissionDenied,
            K::StorageFull => ErrorCode::NotEnoughSpace,
            K::InvalidFilename => ErrorCode::PathTooLong,
            K::NotADirectory => ErrorCode::DestIsFile,
            K::NotFound => ErrorCode::PathNotFound,
            K::IsADirectory => ErrorCode::InvalidArgument,
            // 其他（含 EIO / 意外的 OS 错误）仍算内部问题，但日志里保留原始 io_kind
            _ => ErrorCode::Internal,
        }
    }

    /// 粗档退出码（1 用户问题 / 2 文件问题 / 130 取消）
    ///
    /// 给「按大类分流」的场景用（如 GUI 决定弹哪种框）。
    /// **对外的最终退出码请用 [`Outcome::exit_code`]** —— 它更精确（含 0/3/4/5/130）。
    pub fn exit_code(self) -> i32 {
        use ErrorCode::*;
        match self {
            PathNotFound | InvalidArgument | NotEnoughSpace | PermissionDenied | PathTooLong
            | DestIsFile | SafetyLimit | Internal => 1,
            NeedPassword | WrongPassword | HeadEncrypted | NotArchive | Truncated | MissingVolume
            | DataLost | EncryptedUnbreakable | VideoUnplayable | UnsupportedMethod | UnsafePath => 2,
            Cancelled => exit_code::CANCELLED,
        }
    }

    /// 对外精确退出码（与 v0.4 契约兼容：3 = 不是压缩包，4 = 密码问题）
    pub fn cli_exit_code(self) -> i32 {
        use ErrorCode::*;
        match self {
            PathNotFound | InvalidArgument | NotEnoughSpace | PermissionDenied | PathTooLong
            | DestIsFile | SafetyLimit | Internal => exit_code::ERROR,
            NotArchive => exit_code::NOT_ARCHIVE,
            NeedPassword | WrongPassword | HeadEncrypted | EncryptedUnbreakable => exit_code::PASSWORD,
            Truncated | MissingVolume | DataLost | VideoUnplayable | UnsupportedMethod | UnsafePath => {
                exit_code::ERROR
            }
            Cancelled => exit_code::CANCELLED,
        }
    }

    /// 稳定的机器可读标识（日志与诊断包用，见 BACKEND-SPEC §6.2）
    ///
    /// 例：`WARN [NEED_PASSWORD] zip entries encrypted, flags=0x0001`
    pub fn as_str(self) -> &'static str {
        use ErrorCode::*;
        match self {
            PathNotFound => "PATH_NOT_FOUND",
            InvalidArgument => "INVALID_ARGUMENT",
            NotEnoughSpace => "NOT_ENOUGH_SPACE",
            PermissionDenied => "PERMISSION_DENIED",
            PathTooLong => "PATH_TOO_LONG",
            DestIsFile => "DEST_IS_FILE",
            SafetyLimit => "SAFETY_LIMIT",
            NeedPassword => "NEED_PASSWORD",
            WrongPassword => "WRONG_PASSWORD",
            HeadEncrypted => "HEAD_ENCRYPTED",
            UnsupportedMethod => "UNSUPPORTED_METHOD",
            UnsafePath => "UNSAFE_PATH",
            NotArchive => "NOT_ARCHIVE",
            Truncated => "TRUNCATED",
            MissingVolume => "MISSING_VOLUME",
            DataLost => "DATA_LOST",
            EncryptedUnbreakable => "ENCRYPTED_UNBREAKABLE",
            VideoUnplayable => "VIDEO_UNPLAYABLE",
            Cancelled => "CANCELLED",
            Internal => "INTERNAL",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ───────────────────────── 层2：归档级诊断位图 ─────────────────────────

/// 归档级诊断位（层2）
///
/// 一个包可以「打开成功」但「尾部被截断」且「其中 3 个条目 CRC 错」——
/// 位图能同时表达这些，不丢信息（台账 §5.3）。
///
/// 位定义抄 7-Zip `PropID.h:123-134` 的 11 位；**去掉它的 `kpidError` 字符串通道**，
/// 因为字符串文案归外壳，引擎只发码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArchiveFlags(u16);

impl ArchiveFlags {
    /// 里面的东西根本不是压缩包
    pub const IS_NOT_ARC: u16 = 1 << 0;
    /// 目录/头部有不一致（多数是偏移写错，能修）
    pub const HEADERS_ERROR: u16 = 1 << 1;
    /// 头部被加密（`7z -mhe=on`）
    pub const ENCRYPTED_HEADERS: u16 = 1 << 2;
    /// 起点不完整（包从中间开始，前面缺数据）
    pub const UNAVAILABLE_START: u16 = 1 << 3;
    /// 起点不确定（嗅探出的候选，没把握）
    pub const UNCONFIRMED_START: u16 = 1 << 4;
    /// 意外结束（文件被截断）
    pub const UNEXPECTED_END: u16 = 1 << 5;
    /// 归档数据之后还有别的数据（伪装包的常态）
    pub const DATA_AFTER_END: u16 = 1 << 6;
    /// 用了我们不支持的压缩方法
    pub const UNSUPPORTED_METHOD: u16 = 1 << 7;
    /// 用了我们不支持的特性（如特殊加密）
    pub const UNSUPPORTED_FEATURE: u16 = 1 << 8;
    /// 数据本身坏了
    pub const DATA_ERROR: u16 = 1 << 9;
    /// 校验值不对
    pub const CRC_ERROR: u16 = 1 << 10;

    /// 值 → 名字（日志用，顺序与位定义一致）
    const NAMES: &'static [(u16, &'static str)] = &[
        (Self::IS_NOT_ARC, "IS_NOT_ARC"),
        (Self::HEADERS_ERROR, "HEADERS_ERROR"),
        (Self::ENCRYPTED_HEADERS, "ENCRYPTED_HEADERS"),
        (Self::UNAVAILABLE_START, "UNAVAILABLE_START"),
        (Self::UNCONFIRMED_START, "UNCONFIRMED_START"),
        (Self::UNEXPECTED_END, "UNEXPECTED_END"),
        (Self::DATA_AFTER_END, "DATA_AFTER_END"),
        (Self::UNSUPPORTED_METHOD, "UNSUPPORTED_METHOD"),
        (Self::UNSUPPORTED_FEATURE, "UNSUPPORTED_FEATURE"),
        (Self::DATA_ERROR, "DATA_ERROR"),
        (Self::CRC_ERROR, "CRC_ERROR"),
    ];

    /// 空的（没有任何毛病）
    pub fn empty() -> Self {
        Self(0)
    }

    /// 置位
    pub fn insert(&mut self, bit: u16) {
        self.0 |= bit;
    }

    /// 清位
    pub fn remove(&mut self, bit: u16) {
        self.0 &= !bit;
    }

    /// 是否置位
    pub fn contains(&self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    /// 是否干净（没有任何毛病）
    pub fn is_clean(&self) -> bool {
        self.0 == 0
    }

    /// 名字列表（日志/诊断包用）
    pub fn names(&self) -> Vec<&'static str> {
        Self::NAMES
            .iter()
            .filter(|(bit, _)| self.contains(*bit))
            .map(|(_, name)| *name)
            .collect()
    }
}

impl fmt::Display for ArchiveFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_clean() {
            return f.write_str("CLEAN");
        }
        f.write_str(&self.names().join(","))
    }
}

// ───────────────────────── 层3：条目级结果 ─────────────────────────

/// 单个条目的处理结果（层3）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ItemResult {
    /// 正常写出并校验通过
    #[default]
    Ok,
    /// 用户挑选时被跳过（不是错误）
    Skipped,
    /// 校验值不对（内容可能仍是好的，但不敢保证）
    CrcError,
    /// 数据坏了
    DataError,
    /// **安全限额中止**（压缩比 / 单次总量）—— 文件本身没坏，是我们自己的闸拦的
    ///
    /// 为什么必须和 `DataError` 分开：CLI 每个失败条目都打一行
    /// 「⚠ xxx 没解开（数据损坏）」，若限额也报这个，用户看到的就是
    /// 「数据损坏」+「文件没坏」两句自相矛盾的话。
    SafetyLimited,
    /// 这个条目加密了，没有（正确的）密码
    Encrypted,
    /// 因为前面的错误没轮到它
    NotExtracted,
}

impl ItemResult {
    /// 是否算成功
    pub fn is_ok(self) -> bool {
        matches!(self, ItemResult::Ok | ItemResult::Skipped)
    }

    /// 稳定标识（日志 / 协议用；**界面不要直接显示这个**）
    pub fn as_str(self) -> &'static str {
        match self {
            ItemResult::Ok => "OK",
            ItemResult::Skipped => "SKIPPED",
            ItemResult::CrcError => "CRC_ERROR",
            ItemResult::DataError => "DATA_ERROR",
            ItemResult::SafetyLimited => "SAFETY_LIMITED",
            ItemResult::Encrypted => "ENCRYPTED",
            ItemResult::NotExtracted => "NOT_EXTRACTED",
        }
    }

    /// 中国人话（CLI / 界面直接显示这个，D21：不许把原始码摆给用户）
    pub fn 人话(self) -> &'static str {
        match self {
            ItemResult::Ok => "成功",
            ItemResult::Skipped => "跳过",
            ItemResult::CrcError => "校验不过（内容坏了）",
            ItemResult::DataError => "数据损坏",
            ItemResult::SafetyLimited => "超过安全限额",
            ItemResult::Encrypted => "加密，没解开",
            ItemResult::NotExtracted => "没解出来",
        }
    }
}

impl fmt::Display for ItemResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ───────────────────────── 任务结论 + 退出码 ─────────────────────────

/// 一个任务的最终结论
///
/// 关键点：**「取消」和「部分成功」都不是「失败」**（台账 §5.1 / §5.8）。
/// 外壳一处 match 就知道要不要弹错误框、退出码给多少。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// 全部成功
    Success,
    /// 部分成功（救出 done/total 个条目）
    Partial {
        /// 成功条目数
        done: u64,
        /// 总条目数
        total: u64,
    },
    /// 用户取消（已完成的保留）
    Cancelled {
        /// 已成功条目数
        done: u64,
        /// 总条目数
        total: u64,
    },
    /// 失败
    Failed(ErrorCode),
}

impl Outcome {
    /// 是否完全成功
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success)
    }

    /// 是否真的是失败（部分成功与取消都不算）
    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::Failed(_))
    }

    /// 是否带警告（部分成功 / 取消）
    pub fn has_warnings(&self) -> bool {
        matches!(self, Outcome::Partial { .. } | Outcome::Cancelled { .. })
    }

    /// 对外退出码（**唯一真源**）
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Success => exit_code::OK,
            Outcome::Partial { .. } => exit_code::PARTIAL,
            Outcome::Cancelled { .. } => exit_code::CANCELLED,
            Outcome::Failed(code) => code.cli_exit_code(),
        }
    }

    /// 稳定标识（日志用）
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Success => "SUCCESS",
            Outcome::Partial { .. } => "PARTIAL",
            Outcome::Cancelled { .. } => "CANCELLED",
            Outcome::Failed(_) => "FAILED",
        }
    }

    /// 由一个错误构造（`Cancelled` 会被识别成取消语义，不是失败）
    pub fn from_error(err: &Error) -> Self {
        if err.is_cancelled() {
            Outcome::Cancelled { done: 0, total: 0 }
        } else {
            Outcome::Failed(err.code)
        }
    }

    /// 按「成功/总数」推断结论（全成功 → `Success`；有成功有失败 → `Partial`）
    pub fn from_counts(done: u64, total: u64) -> Self {
        if total > 0 && done == total {
            Outcome::Success
        } else if done > 0 {
            Outcome::Partial { done, total }
        } else {
            Outcome::Failed(ErrorCode::DataLost)
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Success => f.write_str("SUCCESS"),
            Outcome::Partial { done, total } => write!(f, "PARTIAL {done}/{total}"),
            Outcome::Cancelled { done, total } => write!(f, "CANCELLED {done}/{total}"),
            Outcome::Failed(code) => write!(f, "FAILED {}", code.as_str()),
        }
    }
}

/// 结构化错误
///
/// - `code`    —— 稳定错误码，外壳据此选文案与界面
/// - `detail`  —— 结构化参数，供外壳做文案插值（如 `("entries", "351")`）
/// - `context` —— 给开发者的上下文（只进日志，不进用户文案）
/// - `source`  —— 底层错误链
#[derive(Debug)]
pub struct Error {
    /// 稳定错误码
    pub code: ErrorCode,
    /// 结构化参数（键值对，供外壳插值）
    pub detail: Vec<(String, String)>,
    /// 开发者上下文（日志用）
    pub context: Option<String>,
    /// 底层错误
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    /// 用错误码新建
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            detail: Vec::new(),
            context: None,
            source: None,
        }
    }

    /// 用户取消的快捷构造
    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled)
    }

    /// 附加结构化参数（键值对）
    pub fn with(mut self, key: &str, value: impl fmt::Display) -> Self {
        self.detail.push((key.to_string(), value.to_string()));
        self
    }

    /// 附加开发者上下文（进日志，不进用户文案）
    pub fn context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    /// 附加底层错误
    pub fn source(mut self, err: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(err));
        self
    }

    /// 读取某个结构化参数
    pub fn param(&self, key: &str) -> Option<&str> {
        self.detail
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 是不是「用户取消」（不是错误）
    pub fn is_cancelled(&self) -> bool {
        self.code == ErrorCode::Cancelled
    }
}

impl fmt::Display for Error {
    /// 面向日志/开发者的中性输出（**不是**用户文案）
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]", self.code.as_str())?;
        if !self.detail.is_empty() {
            let kv: Vec<String> = self
                .detail
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            write!(f, " {}", kv.join(" "))?;
        }
        if let Some(ctx) = &self.context {
            write!(f, " ({ctx})")?;
        }
        Ok(())
    }
}

impl From<std::io::Error> for Error {
    /// 让 `?` 能直接作用在 IO 操作上
    ///
    /// 若底层是"用户取消"（`Interrupted`），保留该语义便于上层判断（台账 §5.8）：
    /// 取消不该显示成「操作失败」。
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::Interrupted {
            return Error::cancelled()
                .with("io_kind", "Interrupted")
                .context(format!("io error: {e}"))
                .source(e);
        }
        // T9-P3 / T8-P5：分类而不是一律 Internal —— 没权限 / 空间不足 / 路径太长 /
        // 目标不是文件夹，这几种用户自己能解决，得报对应的码
        Error::new(ErrorCode::from_io(&e))
            .with("io_kind", format!("{:?}", e.kind()))
            .context(format!("io error: {e}"))
            .source(e)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io错误按类型归类而不是一律内部错误() {
        use std::io::{Error as Io, ErrorKind as K};
        // 体检 T9-P3 / T8-P5 的四个场景：这几种用户自己能解决，必须报对应的码
        assert_eq!(
            ErrorCode::from_io(&Io::from(K::PermissionDenied)),
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            ErrorCode::from_io(&Io::from(K::StorageFull)),
            ErrorCode::NotEnoughSpace
        );
        assert_eq!(
            ErrorCode::from_io(&Io::from(K::InvalidFilename)),
            ErrorCode::PathTooLong
        );
        assert_eq!(
            ErrorCode::from_io(&Io::from(K::NotADirectory)),
            ErrorCode::DestIsFile
        );
        // 真内部问题还是要报内部错误 —— 别把什么都赖到用户头上
        assert_eq!(ErrorCode::from_io(&Io::from(K::Other)), ErrorCode::Internal);
        assert_eq!(
            ErrorCode::from_io(&Io::from(K::UnexpectedEof)),
            ErrorCode::Internal
        );
    }

    #[test]
    fn 报告只记第一个可归类的io错误() {
        use std::io::{Error as Io, ErrorKind as K};
        let mut rep = crate::extract::ExtractReport::default();
        rep.note_io_error(&Io::from(K::PermissionDenied));
        rep.note_io_error(&Io::from(K::StorageFull));
        // 只留第一个：一次解压里用户最该看到的是最初那一下拦路的原因
        assert_eq!(rep.io_error, Some(ErrorCode::PermissionDenied));

        // 真内部问题不占坑，也不该让上层误以为是用户能解决的
        let mut rep2 = crate::extract::ExtractReport::default();
        rep2.note_io_error(&Io::from(K::Other));
        assert_eq!(rep2.io_error, None);
    }

    #[test]
    fn 错误码对应正确的退出码() {
        assert_eq!(ErrorCode::PathNotFound.exit_code(), 1);
        assert_eq!(ErrorCode::NeedPassword.exit_code(), 2);
        assert_eq!(ErrorCode::Internal.exit_code(), 1);
        assert_eq!(ErrorCode::Cancelled.exit_code(), 130);
    }

    #[test]
    fn 对外退出码与_v04_契约兼容() {
        assert_eq!(ErrorCode::NotArchive.cli_exit_code(), exit_code::NOT_ARCHIVE);
        assert_eq!(ErrorCode::NeedPassword.cli_exit_code(), exit_code::PASSWORD);
        assert_eq!(ErrorCode::WrongPassword.cli_exit_code(), exit_code::PASSWORD);
        assert_eq!(ErrorCode::Truncated.cli_exit_code(), exit_code::ERROR);
        assert_eq!(ErrorCode::PathNotFound.cli_exit_code(), exit_code::ERROR);
    }

    #[test]
    fn 结构化参数可读取() {
        let e = Error::new(ErrorCode::NeedPassword)
            .with("entries", 351)
            .with("encrypted_names", true);
        assert_eq!(e.param("entries"), Some("351"));
        assert_eq!(e.param("encrypted_names"), Some("true"));
        assert_eq!(e.param("不存在"), None);
    }

    #[test]
    fn 日志输出是中性的不含中文文案() {
        let e = Error::new(ErrorCode::WrongPassword)
            .with("tried", 3)
            .context("zip entry crc mismatch");
        let s = e.to_string();
        assert!(s.starts_with("[WRONG_PASSWORD]"));
        assert!(s.contains("tried=3"));
        // 引擎不产出用户文案：这里不该出现中文句子
        assert!(!s.contains("密码不对"));
    }

    #[test]
    fn 中断的io错误被识别成取消() {
        let io = std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled by user");
        let e = Error::from(io);
        assert!(e.is_cancelled(), "取消不该被当成内部错误");
        assert_eq!(e.code, ErrorCode::Cancelled);
    }

    #[test]
    fn 归档诊断位图可多位并存() {
        let mut f = ArchiveFlags::empty();
        assert!(f.is_clean());
        f.insert(ArchiveFlags::UNEXPECTED_END);
        f.insert(ArchiveFlags::DATA_AFTER_END);
        assert!(f.contains(ArchiveFlags::UNEXPECTED_END));
        assert!(!f.contains(ArchiveFlags::CRC_ERROR));
        assert!(!f.is_clean());
        assert_eq!(f.names(), vec!["UNEXPECTED_END", "DATA_AFTER_END"]);
        assert_eq!(f.to_string(), "UNEXPECTED_END,DATA_AFTER_END");

        f.remove(ArchiveFlags::UNEXPECTED_END);
        assert_eq!(f.names(), vec!["DATA_AFTER_END"]);
    }

    #[test]
    fn 条目结果区分成功与失败() {
        assert!(ItemResult::Ok.is_ok());
        assert!(ItemResult::Skipped.is_ok(), "用户自己跳过的不是错误");
        assert!(!ItemResult::CrcError.is_ok());
        assert!(!ItemResult::Encrypted.is_ok());
        assert_eq!(ItemResult::NotExtracted.as_str(), "NOT_EXTRACTED");
    }

    #[test]
    fn 部分成功不是失败() {
        let o = Outcome::Partial {
            done: 340,
            total: 351,
        };
        assert!(!o.is_failure());
        assert!(o.has_warnings());
        assert_eq!(o.exit_code(), exit_code::PARTIAL);
        assert_eq!(o.to_string(), "PARTIAL 340/351");
    }

    #[test]
    fn 取消不是失败且退出码130() {
        let o = Outcome::Cancelled {
            done: 12,
            total: 351,
        };
        assert!(!o.is_failure());
        assert!(o.has_warnings());
        assert_eq!(o.exit_code(), exit_code::CANCELLED);
    }

    #[test]
    fn 失败按错误码映射退出码() {
        assert_eq!(
            Outcome::Failed(ErrorCode::NotArchive).exit_code(),
            exit_code::NOT_ARCHIVE
        );
        assert_eq!(
            Outcome::Failed(ErrorCode::NeedPassword).exit_code(),
            exit_code::PASSWORD
        );
        assert_eq!(
            Outcome::Failed(ErrorCode::PathNotFound).exit_code(),
            exit_code::ERROR
        );
        assert!(Outcome::Failed(ErrorCode::Truncated).is_failure());
    }

    #[test]
    fn 由错误构造结论时取消单独成档() {
        assert_eq!(
            Outcome::from_error(&Error::cancelled()),
            Outcome::Cancelled { done: 0, total: 0 }
        );
        assert_eq!(
            Outcome::from_error(&Error::new(ErrorCode::NotArchive)),
            Outcome::Failed(ErrorCode::NotArchive)
        );
    }

    #[test]
    fn 按计数推断结论() {
        assert_eq!(Outcome::from_counts(351, 351), Outcome::Success);
        assert_eq!(
            Outcome::from_counts(340, 351),
            Outcome::Partial {
                done: 340,
                total: 351
            }
        );
        assert_eq!(
            Outcome::from_counts(0, 351),
            Outcome::Failed(ErrorCode::DataLost)
        );
    }

    #[test]
    fn 三层结果可序列化给外壳() {
        let o = Outcome::Partial { done: 1, total: 2 };
        let j = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<Outcome>(&j).unwrap(), o);

        let mut f = ArchiveFlags::empty();
        f.insert(ArchiveFlags::CRC_ERROR);
        let j = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<ArchiveFlags>(&j).unwrap(), f);
    }
}
