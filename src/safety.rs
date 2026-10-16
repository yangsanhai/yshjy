//! 安全护栏（内置，不可绕过）
//!
//! 见 docs/ARCHITECTURE.md §8 与 06-工程与设计审查 A4。
//!
//! # 防的是什么
//! 1. **Zip Slip / 路径穿越** —— 恶意包里的条目名写成 `../../.ssh/authorized_keys`
//!    就能在你不知情时覆盖系统文件。隐写者自身就有这个隐患，我们必须防住。
//! 2. **Windows 特有陷阱** —— 盘符（`C:x`）、保留设备名（`CON`、`NUL`）、
//!    备用数据流（`file.txt:evil`）。我们做全平台，这些都要拦。
//! 3. **解压炸弹** —— 声明 1KB 解出 100GB。
//! 4. **超长路径** —— 某些文件系统会因此失败或异常。

use crate::error::{Error, ErrorCode, Result};
use std::path::{Component, Path, PathBuf};

/// 安全限制（默认值见 docs/BACKEND-SPEC.md §2.2 配置示例）
#[derive(Debug, Clone)]
pub struct Limits {
    /// 单次任务累计写出的字节上限（0 表示不限制）
    pub max_extract_bytes: u64,
    /// 解压后 / 压缩前 的比值上限（防炸弹）
    pub max_ratio: u64,
    /// 归档内条目路径的最大长度（字节数）
    pub max_path_len: usize,
    /// 解出的可执行文件是否要提醒（外壳据此高亮）
    pub warn_executables: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_extract_bytes: 0,
            max_ratio: 100,
            max_path_len: 512,
            warn_executables: true,
        }
    }
}

/// Windows 保留设备名（不区分大小写，带扩展名也算）
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 解出的文件是否属于"可能危险"的可执行类型（外壳据此提醒，见 UI-SPEC §9.3）
pub fn is_risky_executable(entry: &str) -> bool {
    let lower = entry.to_ascii_lowercase();
    const RISKY: &[&str] = &[
        ".exe",
        ".scr",
        ".com",
        ".bat",
        ".cmd",
        ".ps1",
        ".vbs",
        ".js",
        ".jar",
        ".msi",
        ".sh",
        ".run",
        ".apk",
        ".app",
        ".dmg",
        ".pkg",
        ".deb",
        ".appimage",
        ".lnk",
        ".desktop",
    ];
    RISKY.iter().any(|ext| lower.ends_with(ext))
}

/// 把归档内的相对路径**安全地**解析到输出根目录之下。
///
/// 任何可疑输入都会返回错误，而不是"尽力而为"。
///
/// ```
/// use yshjy::safety::resolve_safe;
/// use std::path::Path;
///
/// let root = Path::new("/tmp/out");
/// // 正常条目
/// assert!(resolve_safe(root, "game/data.bin").is_ok());
/// // 穿越尝试一律拒绝
/// assert!(resolve_safe(root, "../evil").is_err());
/// assert!(resolve_safe(root, "/etc/passwd").is_err());
/// assert!(resolve_safe(root, "a/../../evil").is_err());
/// ```
pub fn resolve_safe(root: &Path, entry: &str) -> Result<PathBuf> {
    if entry.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument).with("reason", "empty_entry_path"));
    }

    // ① 长度检查（按字节）
    if entry.len() > 512 {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .with("reason", "entry_path_too_long")
            .with("len", entry.len()));
    }

    // ② 统一分隔符：包里可能用 Windows 反斜杠
    let normalized = entry.replace('\\', "/");

    // ③ 控制字符 / NUL：跨平台都非法，且可能被用于绕过检查
    if normalized.chars().any(|c| c.is_control()) {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .with("reason", "control_char_in_path")
            .context(format!("entry={entry:?}")));
    }

    // ④ Windows 备用数据流（`file.txt:evil`）与盘符（`C:`）
    //    在 Linux 上冒号合法，但我们的包可能来自 Windows —— 从源头拦掉
    if normalized.contains(':') {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .with("reason", "colon_in_path")
            .context(format!("entry={entry:?}")));
    }

    let rel = Path::new(&normalized);

    // ⑤ 绝对路径（含 Unix `/x` 与 Windows `C:\x`）一律拒绝
    if rel.is_absolute() {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .with("reason", "absolute_path")
            .context(format!("entry={entry:?}")));
    }

    // ⑥ 逐段检查：只接受普通路径段，拒绝 `..`、根、前缀
    let mut out = PathBuf::from(root);
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => {
                let seg_str = seg.to_string_lossy();
                // Windows 保留设备名（`CON`、`con.txt` 都算）
                let stem = seg_str.split('.').next().unwrap_or("").to_ascii_uppercase();
                if WINDOWS_RESERVED.contains(&stem.as_str()) {
                    return Err(Error::new(ErrorCode::InvalidArgument)
                        .with("reason", "windows_reserved_name")
                        .with("segment", seg_str.to_string()));
                }
                // 结尾空格或点（Windows 会静默改写，容易造成"解出的文件找不到"）
                if seg_str.ends_with(' ') || seg_str.ends_with('.') {
                    return Err(Error::new(ErrorCode::InvalidArgument)
                        .with("reason", "trailing_space_or_dot")
                        .with("segment", seg_str.to_string()));
                }
                out.push(seg);
            }
            Component::CurDir => {} // `.` 直接跳过
            Component::ParentDir => {
                return Err(Error::new(ErrorCode::InvalidArgument)
                    .with("reason", "parent_dir_in_path")
                    .context(format!("entry={entry:?}")));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::new(ErrorCode::InvalidArgument)
                    .with("reason", "root_or_prefix_in_path")
                    .context(format!("entry={entry:?}")));
            }
        }
    }

    // ⑦ 结果必须落在 root 之下（双保险）
    if !out.starts_with(root) {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .with("reason", "escaped_output_root")
            .context(format!("entry={entry:?}")));
    }

    Ok(out)
}

/// 解压进度累加器：用于累计写出量并检查是否超限（防炸弹）
#[derive(Debug)]
pub struct ExtractBudget {
    limits: Limits,
    written: u64,
    packed_total: u64,
}

impl ExtractBudget {
    /// 新建（`packed_total` 为压缩数据总量，用于比值检查；0 表示不检查）
    pub fn new(limits: Limits, packed_total: u64) -> Self {
        Self {
            limits,
            written: 0,
            packed_total,
        }
    }

    /// 记录写入，超限即报错
    pub fn record(&mut self, bytes: u64) -> Result<()> {
        self.written = self.written.saturating_add(bytes);

        if self.limits.max_extract_bytes > 0 && self.written > self.limits.max_extract_bytes {
            return Err(Error::new(ErrorCode::DataLost)
                .with("reason", "extract_size_limit_exceeded")
                .with("written", self.written)
                .with("limit", self.limits.max_extract_bytes));
        }

        if self.limits.max_ratio > 0 && self.packed_total > 0 {
            let ratio = self.written / self.packed_total.max(1);
            if ratio > self.limits.max_ratio {
                return Err(Error::new(ErrorCode::DataLost)
                    .with("reason", "possible_zip_bomb")
                    .with("ratio", ratio)
                    .with("limit", self.limits.max_ratio));
            }
        }

        Ok(())
    }

    /// 已写出总量
    pub fn written(&self) -> u64 {
        self.written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn root() -> &'static Path {
        Path::new("/tmp/yshjy-out")
    }

    #[test]
    fn 正常路径通过() {
        assert!(resolve_safe(root(), "game/data.bin").is_ok());
        assert!(resolve_safe(root(), "a/b/c.txt").is_ok());
        assert!(resolve_safe(root(), "./a/./b.txt").is_ok());
        assert!(resolve_safe(root(), "中文目录/文件.txt").is_ok());
    }

    #[test]
    fn 拒绝目录穿越() {
        assert!(resolve_safe(root(), "../evil").is_err());
        assert!(resolve_safe(root(), "a/../../evil").is_err());
        assert!(resolve_safe(root(), "..").is_err());
        assert!(resolve_safe(root(), "a/..").is_err());
    }

    #[test]
    fn 拒绝绝对路径() {
        assert!(resolve_safe(root(), "/etc/passwd").is_err());
        assert!(resolve_safe(root(), "//server/share").is_err());
    }

    #[test]
    fn 拒绝windows陷阱() {
        assert!(resolve_safe(root(), "C:/Windows/system32").is_err()); // 盘符
        assert!(resolve_safe(root(), "file.txt:evil").is_err()); // 备用数据流
        assert!(resolve_safe(root(), "CON").is_err()); // 保留设备名
        assert!(resolve_safe(root(), "con.txt").is_err());
        assert!(resolve_safe(root(), "NUL").is_err());
        assert!(resolve_safe(root(), "a/LPT1.log").is_err());
        assert!(resolve_safe(root(), "trailing ").is_err()); // 结尾空格
        assert!(resolve_safe(root(), "trailing.").is_err()); // 结尾点
    }

    #[test]
    fn 拒绝空与控制字符() {
        assert!(resolve_safe(root(), "").is_err());
        assert!(resolve_safe(root(), "a\u{0}b").is_err());
        assert!(resolve_safe(root(), "a\nb").is_err());
    }

    #[test]
    fn 反斜杠被当作分隔符处理() {
        // Windows 造的包常用反斜杠
        let p = resolve_safe(root(), "dir\\file.txt").unwrap();
        assert!(p.ends_with("file.txt"));
        // 用反斜杠做穿越同样要拦
        assert!(resolve_safe(root(), "..\\evil").is_err());
    }

    #[test]
    fn 危险可执行文件被识别() {
        assert!(is_risky_executable("setup.exe"));
        assert!(is_risky_executable("图标.EXE")); // 大小写不敏感
        assert!(is_risky_executable("run.sh"));
        assert!(!is_risky_executable("readme.txt"));
        assert!(!is_risky_executable("data.pak"));
    }

    #[test]
    fn 解压炸弹被拦截() {
        let mut b = ExtractBudget::new(
            Limits {
                max_extract_bytes: 1000,
                max_ratio: 0,
                ..Default::default()
            },
            0,
        );
        assert!(b.record(500).is_ok());
        assert!(b.record(600).is_err()); // 累计 1100 > 1000
        assert_eq!(b.written(), 1100);
    }

    #[test]
    fn 比值超限被拦截() {
        let mut b = ExtractBudget::new(
            Limits {
                max_extract_bytes: 0,
                max_ratio: 10,
                ..Default::default()
            },
            100,
        );
        assert!(b.record(500).is_ok()); // 比值 5
        assert!(b.record(600).is_err()); // 比值 11 > 10
    }
}
