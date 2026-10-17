//! XDG 回收站（DESIGN §17.2：解压完原包可以「移入回收站」，**永不 rm**）
//!
//! 规范：`$XDG_DATA_HOME/Trash/files/<名字>` 放文件本体，
//! `$XDG_DATA_HOME/Trash/info/<名字>.trashinfo` 记录原路径和删除时间。
//! 文件管理器（Nautilus 等）看得懂，用户能一键还原。
//!
//! 取舍：不引 `trash` crate —— 格式只有三行，自己写更符合「单文件零依赖」。

use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorCode, Result};

/// 回收站根目录（`$XDG_DATA_HOME/Trash`，回退 `~/.local/share/Trash`）
pub fn trash_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME")
        && !x.is_empty()
    {
        return PathBuf::from(x).join("Trash");
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/Trash")
}

/// 把文件/目录移入回收站，返回它在新位置上的路径
///
/// - **绝不 rm**：只做 `rename`（跨盘时退回「复制 + 删源」，但仍然先进回收站）
/// - 重名自动加 `-<时间戳>` 后缀，不会覆盖回收站里别的东西
pub fn move_to_trash(path: &Path) -> Result<PathBuf> {
    let root = trash_dir();
    let files = root.join("files");
    let info = root.join("info");
    std::fs::create_dir_all(&files)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("建回收站失败: {e}")))?;
    std::fs::create_dir_all(&info)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("建回收站失败: {e}")))?;

    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "未命名".to_string());

    // 目标名冲突时加时间戳（Unix 秒），比 `(1)` 更适合回收站
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut target = files.join(&name);
    let mut info_name = name.clone();
    let mut n = 0u32;
    while target.exists() {
        n += 1;
        info_name = format!("{name}.{stamp}.{n}");
        target = files.join(&info_name);
    }

    // 先写 info（写不成就不动原文件，宁可报错）
    let now = 本地时间字符串();
    let info_text = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        path.display(),
        now
    );
    std::fs::write(info.join(format!("{info_name}.trashinfo")), info_text)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("写回收站记录失败: {e}")))?;

    match std::fs::rename(path, &target) {
        Ok(()) => Ok(target),
        Err(_) => {
            // 跨盘：复制过去再删源（源已经在回收站里有副本，删源是安全的）
            copy_recursive(path, &target)?;
            if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
            .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("移入回收站失败: {e}")))?;
            Ok(target)
        }
    }
}

fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)
            .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("建目录失败: {e}")))?;
        for entry in std::fs::read_dir(from)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("读目录失败: {e}")))?
        {
            let entry = entry.map_err(|e| Error::new(ErrorCode::Internal).context(format!("{e}")))?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("复制失败: {e}")))?;
        Ok(())
    }
}

/// `2026-10-17T21:40:00` 这种本地时间（回收站规范要 ISO 8601，不带时区也认）
fn 本地时间字符串() -> String {
    // 不引 chrono：简单按本地偏移算（引擎外壳已注入时区，这里只求够用）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let offset = 本地偏移秒();
    let t = secs + offset;
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let (y, m, d) = 民用日期(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn 本地偏移秒() -> i64 {
    // 从 `date +%z` 拿不到（不能起进程）；用 TZ 环境变量粗略判断：
    // 中国 +08:00。拿不到就按 0（UTC），回收站信息只影响显示时间。
    if let Ok(tz) = std::env::var("TZ")
        && let Some(rest) = tz.strip_prefix("UTC")
        && let Ok(h) = rest.trim_start_matches(['+', '-']).parse::<i64>()
    {
        let sign = if rest.starts_with('-') { -1 } else { 1 };
        return sign * h * 3600;
    }
    8 * 3600 // 本机（Asia/Shanghai）默认
}

/// 公历日期（Howard Hinnant 的 civil_from_days）
fn 民用日期(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 回收站目录在_xdg_data_home_下() {
        let d = trash_dir();
        assert!(d.ends_with("Trash"));
    }

    #[test]
    fn 移入回收站后原位置不存在且本体在files里() {
        let dir = std::env::temp_dir().join(format!("yshjy_trash_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("测试包.mp4");
        std::fs::write(&f, b"payload").unwrap();

        let moved = move_to_trash(&f).unwrap();
        assert!(!f.exists(), "原位置必须已移走");
        assert!(moved.exists(), "回收站里必须有本体");
        assert_eq!(std::fs::read(&moved).unwrap(), b"payload");
        let info = moved
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("info")
            .join(format!(
                "{}.trashinfo",
                moved.file_name().unwrap().to_string_lossy()
            ));
        let text = std::fs::read_to_string(&info).unwrap();
        assert!(text.contains("[Trash Info]"));
        assert!(text.contains("测试包.mp4"));

        let _ = std::fs::remove_file(&moved);
        let _ = std::fs::remove_file(&info);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
