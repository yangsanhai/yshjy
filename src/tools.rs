// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 外部工具检测（**可选增强，不是依赖**）
//!
//! # 用户定下的原则（2026-10-16）
//! **核心功能零外部依赖**（一体化，一个文件走天下）。
//! 外部工具只用于"锦上添花"（例如打包后用 ffmpeg 验证视频能不能播）。
//! 检测不到就**自动降级**，绝不因此报错或拒绝服务。
//!
//! # 举例
//! - 有 `ffmpeg` → 打包后跑一次视频自检（更安心）
//! - 没 `ffmpeg` → 跳过自检，核心功能照常

use std::path::{Path, PathBuf};

/// 外部工具可用情况（启动时探测一次，之后只读）
#[derive(Debug, Clone, Default)]
pub struct ExternalTools {
    /// ffmpeg 路径（用于视频自检等可选功能）
    pub ffmpeg: Option<PathBuf>,
    /// ffprobe 路径（读取视频信息）
    pub ffprobe: Option<PathBuf>,
}

impl ExternalTools {
    /// 探测系统里有哪些可用工具
    pub fn detect() -> Self {
        Self {
            ffmpeg: find_executable("ffmpeg"),
            ffprobe: find_executable("ffprobe"),
        }
    }

    /// 给用户/日志看的一行摘要
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.push(if self.ffmpeg.is_some() {
            "ffmpeg ✓"
        } else {
            "ffmpeg ✗"
        });
        parts.push(if self.ffprobe.is_some() {
            "ffprobe ✓"
        } else {
            "ffprobe ✗"
        });
        parts.join(", ")
    }
}

/// 在 `PATH` 中查找可执行文件（跨平台：Windows 会补 `.exe`）
///
/// 用 `std::env::split_paths` 处理分隔符差异（`:` vs `;`），
/// 不硬编码任何平台特定路径。
pub fn find_executable(name: &str) -> Option<PathBuf> {
    // 已经是路径的形式（含分隔符）就直接检查
    if name.contains('/') || name.contains('\\') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // Windows：`ffmpeg` → `ffmpeg.exe`
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{name}.exe"));
            if is_executable(&with_exe) {
                return Some(with_exe);
            }
        }
    }
    None
}

/// 判断是否为可执行文件
#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

/// 判断是否为文件（Windows 上按扩展名判断可执行比较麻烦，这里只要求存在且是文件）
#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 探测不会panic() {
        // 只要求不崩、能给出摘要
        let t = ExternalTools::detect();
        let s = t.summary();
        assert!(s.contains("ffmpeg"));
    }

    #[test]
    fn 查找不存在的工具返回空() {
        assert!(find_executable("这个程序肯定不存在_yshjy_test").is_none());
    }

    #[test]
    fn 能找到常见命令() {
        // 在类 Unix 上 sh 一定存在；Windows 上跳过
        #[cfg(unix)]
        {
            let sh = find_executable("sh");
            assert!(sh.is_some(), "应该能找到 sh");
            assert!(is_executable(sh.as_ref().unwrap()));
        }
        #[cfg(not(unix))]
        {
            assert!(find_executable("cmd").is_some() || find_executable("cmd.exe").is_some());
        }
    }

    #[test]
    fn 带路径的形式也能查() {
        #[cfg(unix)]
        {
            assert!(find_executable("/bin/sh").is_some());
            assert!(find_executable("/bin/绝对不存在的程序").is_none());
        }
    }
}
