//! 把归档里记录的「时间戳 / 权限」还原到解出来的文件上
//!
//! # 为什么单开一个模块
//!
//! 每个格式的元数据 API 都不一样（ZIP 给的是年月日时分秒、TAR 给的是 Unix 秒、
//! 7z 给的是 Windows FILETIME…），但「把属性应用到落盘文件」这一步完全相同。
//! 所以这里定义**格式无关的中间形态** [`EntryAttrs`]：各解压器只负责把自己的
//! 元数据翻译进来，落盘逻辑只写一份。
//!
//! # 尽力而为 —— 绝不让解压失败
//!
//! 某些文件系统或挂载点就是不给设（FAT32 没有 Unix 权限、只读挂载、部分 tmpfs 的
//! 时间戳策略）。那种情况下**解压本身是成功的**，「时间戳没还原上」不该把它变成
//! 一次失败解压。所以 [`apply`] 返回 `Result`，调用方可以记一条提示，也可以无视。
//!
//! # 为什么自己算公历转换
//!
//! 就差一个「公历年月日 → 天数」的转换（20 行，含闰年规则），不值得为它多引
//! `chrono` / `time` 这种重依赖 —— 这个工具要单文件跨平台分发。

use std::path::Path;

use crate::error::{Error, ErrorCode, Result};

/// 归档条目的属性（与格式无关的中间形态）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryAttrs {
    /// 修改时间（Unix 秒；`None` = 归档里没记）
    pub mtime: Option<i64>,
    /// Unix 权限位（如 `0o644`；`None` = 归档里没记或平台不适用）
    pub mode: Option<u32>,
}

impl EntryAttrs {
    /// 有没有东西可还原（省得白跑一趟系统调用）
    pub fn is_empty(&self) -> bool {
        self.mtime.is_none() && self.mode.is_none()
    }
}

/// 把属性应用到磁盘上的文件 / 目录
///
/// 时间戳先设、权限后设：设权限可能把文件变成只读，之后再改时间就改不动了。
pub fn apply(path: &Path, attrs: &EntryAttrs) -> Result<()> {
    if attrs.is_empty() {
        return Ok(());
    }
    if let Some(secs) = attrs.mtime {
        let t = filetime::FileTime::from_unix_time(secs, 0);
        filetime::set_file_mtime(path, t).map_err(|e| 属性出错(path, "时间", e))?;
    }
    #[cfg(unix)]
    if let Some(mode) = attrs.mode {
        use std::os::unix::fs::PermissionsExt;
        // 只取权限位（低 12 位）：归档里可能带 setuid/setgid/sticky，
        // 从网上下载的包**不该**凭它给文件挂上 setuid —— 那是提权隐患。
        let perm = std::fs::Permissions::from_mode(mode & 0o777);
        std::fs::set_permissions(path, perm).map_err(|e| 属性出错(path, "权限", e))?;
    }
    Ok(())
}

fn 属性出错(path: &Path, what: &str, e: std::io::Error) -> Error {
    Error::new(ErrorCode::Internal).context(format!("还原{what}失败 {}: {e}", path.display()))
}

/// Unix 秒 → 公历年月日时分秒（UTC）
///
/// 与 [`civil_to_unix`] 互逆（有测试盯着这条互逆性）。
pub fn unix_to_civil(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // 反过来用 Hinnant 的 civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = y + if m <= 2 { 1 } else { 0 };
    (
        year as i32,
        m as u32,
        d as u32,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// 公历年月日时分秒 → Unix 秒（UTC）
///
/// 用 Howard Hinnant 的 `days_from_civil` 算法 —— 被广泛使用、验证充分，
/// 闰年规则（4 年一闰、100 年不闰、400 年再闰）已经包含在里面。
pub fn civil_to_unix(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let d = day as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719468;
    days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 公历转时间戳对得上已知值() {
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(civil_to_unix(2000, 1, 1, 0, 0, 0), 946_684_800);
        assert_eq!(civil_to_unix(2020, 10, 17, 12, 0, 0), 1_602_936_000);
        // 闰日：2024 年 2 月 29 日确实存在
        assert_eq!(civil_to_unix(2024, 2, 29, 0, 0, 0), 1_709_164_800);
        // 世纪闰年规则：2000-02-29 存在（能被 400 整除），1900-02-29 不存在
        assert_eq!(civil_to_unix(2000, 2, 29, 0, 0, 0), 951_782_400);
        // 时间部分
        assert_eq!(civil_to_unix(1970, 1, 1, 1, 2, 3), 3723);
        // 1970 之前（负数）
        assert_eq!(civil_to_unix(1969, 12, 31, 23, 59, 59), -1);
    }

    #[test]
    fn 公历转换两个方向互逆() {
        // 这条最值钱：正反两个方向必须严格互逆，否则时间会漂
        for secs in [
            0i64,
            1_602_936_000,
            951_782_400,
            1_709_164_800,
            -1,
            -86_400,
            2_000_000_000,
        ] {
            let (y, m, d, hh, mm, ss) = unix_to_civil(secs);
            assert_eq!(
                civil_to_unix(y, m, d, hh, mm, ss),
                secs,
                "时间戳 {secs} 换算再换回来变了（{y}-{m}-{d} {hh}:{mm}:{ss}）"
            );
        }
        // 抽一个具体值核对（免得两个方向一起错、还互相"对上"）
        assert_eq!(unix_to_civil(1_602_936_000), (2020, 10, 17, 12, 0, 0));
        assert_eq!(unix_to_civil(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn 能把时间戳和权限设到文件上() {
        let dir = std::env::temp_dir().join(format!("yshjy_restore_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, b"hi").unwrap();

        let attrs = EntryAttrs {
            mtime: Some(1_602_936_000),
            mode: Some(0o640),
        };
        apply(&f, &attrs).expect("应当能把属性设上去");

        let md = std::fs::metadata(&f).unwrap();
        let got = filetime::FileTime::from_last_modification_time(&md);
        assert_eq!(got.unix_seconds(), 1_602_936_000, "时间戳没设对");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(md.permissions().mode() & 0o777, 0o640, "权限没设对");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 空属性不做任何事() {
        let attrs = EntryAttrs::default();
        assert!(attrs.is_empty());
        // 路径根本不存在也不该报错（因为我们压根不会去碰它）
        assert!(apply(Path::new("/不存在的路径/xyz"), &attrs).is_ok());
    }

    #[test]
    fn 危险权限位会被剥掉() {
        // setuid(0o4000) / setgid(0o2000) / sticky(0o1000) 都该被剥掉 ——
        // 从网上下载的包不该凭归档里写的权限就给文件挂 setuid（提权隐患）
        let dir = std::env::temp_dir().join(format!("yshjy_restore_suid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("b.txt");
        std::fs::write(&f, b"x").unwrap();

        apply(
            &f,
            &EntryAttrs {
                mtime: None,
                mode: Some(0o4755), // setuid + rwxr-xr-x
            },
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).unwrap().permissions().mode();
            assert_eq!(mode & 0o4000, 0, "setuid 必须被剥掉");
            assert_eq!(mode & 0o777, 0o755, "普通权限位要保留");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
