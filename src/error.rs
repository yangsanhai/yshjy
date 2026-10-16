//! 错误码体系
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

use std::fmt;

/// 统一结果类型
pub type Result<T> = std::result::Result<T, Error>;

/// 稳定的错误码（外壳据此渲染文案与界面，见 docs/UI-SPEC.md §10）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    // ── 用户可自行解决（CLI 退出码 1）──
    /// 路径不存在
    PathNotFound,
    /// 参数不合法
    InvalidArgument,
    /// 磁盘空间不足
    NotEnoughSpace,
    /// 没有写入权限
    PermissionDenied,

    // ── 文件本身的问题（CLI 退出码 2）──
    /// 需要密码
    NeedPassword,
    /// 密码错误
    WrongPassword,
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

    // ── 内部问题（CLI 退出码 3）──
    /// 程序内部错误
    Internal,
}

impl ErrorCode {
    /// 对应 CLI 退出码（见 docs/CLI-SPEC.md §7）
    pub fn exit_code(self) -> i32 {
        use ErrorCode::*;
        match self {
            PathNotFound | InvalidArgument | NotEnoughSpace | PermissionDenied => 1,
            NeedPassword | WrongPassword | NotArchive | Truncated | MissingVolume | DataLost
            | EncryptedUnbreakable | VideoUnplayable => 2,
            Internal => 3,
        }
    }

    /// 稳定的机器可读标识（日志与诊断包用，见 BACKEND-SPEC §6.2）
    ///
    /// 例：`WARN [PWD_REQUIRED] zip entries encrypted, flags=0x0001`
    pub fn as_str(self) -> &'static str {
        use ErrorCode::*;
        match self {
            PathNotFound => "PATH_NOT_FOUND",
            InvalidArgument => "INVALID_ARGUMENT",
            NotEnoughSpace => "NOT_ENOUGH_SPACE",
            PermissionDenied => "PERMISSION_DENIED",
            NeedPassword => "NEED_PASSWORD",
            WrongPassword => "WRONG_PASSWORD",
            NotArchive => "NOT_ARCHIVE",
            Truncated => "TRUNCATED",
            MissingVolume => "MISSING_VOLUME",
            DataLost => "DATA_LOST",
            EncryptedUnbreakable => "ENCRYPTED_UNBREAKABLE",
            VideoUnplayable => "VIDEO_UNPLAYABLE",
            Internal => "INTERNAL",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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
    /// 若底层是"用户取消"（`Interrupted`），保留该语义便于上层判断。
    fn from(e: std::io::Error) -> Self {
        let code = ErrorCode::Internal;
        Error::new(code)
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
    fn 错误码对应正确的退出码() {
        assert_eq!(ErrorCode::PathNotFound.exit_code(), 1);
        assert_eq!(ErrorCode::NeedPassword.exit_code(), 2);
        assert_eq!(ErrorCode::Internal.exit_code(), 3);
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
}
