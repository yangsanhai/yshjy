//! 跨平台路径：状态目录 / 主目录
//!
//! 为什么单独一个模块：崩溃日志（`crash.rs`）和诊断包（`report.rs`）各写过一份
//! 「XDG 状态目录」，都是 Unix 假设（`HOME` + `~/.local/state`）。Windows / macOS /
//! 手机上没有这回事，于是同一件事要改三处、还会改漏。这里只留一份。
//!
//! 规则（与 `config.rs` 的配置目录保持一致的分工）：
//! - **配置** = 用户的选择 → `config.rs::config_dir()`（Windows `APPDATA` / macOS
//!   `Application Support` / Unix `XDG_CONFIG_HOME`）
//! - **状态** = 运行产物（崩溃日志、历史、引导标记）→ 本模块 `state_dir()`
//!
//! 允许环境变量覆盖，测试/便携模式用它（`YSHJY_STATE_DIR`）。

use std::path::PathBuf;

/// 用户主目录（Windows 用 `USERPROFILE`）
pub fn home() -> Option<PathBuf> {
    let 变量 = if cfg!(target_os = "windows") {
        ["USERPROFILE", "HOME"]
    } else {
        ["HOME", "USERPROFILE"]
    };
    for k in 变量 {
        if let Some(v) = std::env::var_os(k)
            && !v.is_empty()
        {
            return Some(PathBuf::from(v));
        }
    }
    None
}

/// 状态目录（崩溃日志 / 历史 / 引导标记）
///
/// 优先级：
/// 1. `YSHJY_STATE_DIR`（测试、便携版）
/// 2. 平台惯例：
///    - Windows `%LOCALAPPDATA%\yshjy`
///    - macOS `~/Library/Application Support/yshjy`
///    - 其它 Unix `$XDG_STATE_HOME/yshjy`，没有就 `~/.local/state/yshjy`
///
/// 返回 `None` 只在「连 HOME 都没有」的极端情况下发生 —— 调用方按「没有状态目录
/// 也能继续跑」处理，不要为了写日志而让主流程失败。
pub fn state_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("YSHJY_STATE_DIR")
        && !d.is_empty()
    {
        return Some(PathBuf::from(d));
    }
    if cfg!(target_os = "windows")
        && let Some(a) = std::env::var_os("LOCALAPPDATA")
        && !a.is_empty()
    {
        return Some(PathBuf::from(a).join("yshjy"));
    }
    if cfg!(target_os = "macos")
        && let Some(h) = home()
    {
        return Some(h.join("Library/Application Support").join("yshjy"));
    }
    unix_state_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        home(),
    )
}

/// Unix 状态目录的纯计算部分（可单测，不碰进程环境）
///
/// XDG 规范：`XDG_STATE_HOME` 必须是绝对路径，相对路径一律忽略。
fn unix_state_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = xdg
        && p.is_absolute()
    {
        return Some(p.join("yshjy"));
    }
    home.map(|h| h.join(".local/state/yshjy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 只在 Unix 上测：`/tmp/state` 在 Windows 不算绝对路径（没有盘符），
    // is_absolute 的语义是平台相关的（W8）
    #[cfg(unix)]
    #[test]
    fn xdg_绝对路径优先() {
        assert_eq!(
            unix_state_from(Some(PathBuf::from("/tmp/state")), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/tmp/state/yshjy"))
        );
    }

    #[test]
    fn xdg_相对路径按规范忽略() {
        assert_eq!(
            unix_state_from(Some(PathBuf::from("state")), Some(PathBuf::from("/home/u"))),
            Some(PathBuf::from("/home/u/.local/state/yshjy"))
        );
    }

    #[test]
    fn 没有主目录就返回_none_不猜() {
        assert_eq!(unix_state_from(None, None), None);
    }
}
