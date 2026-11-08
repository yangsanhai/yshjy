// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 解压后「等数据真写进设备」（只在移动设备上做）
//!
//! # 为什么要有这个模块
//!
//! 2026-10-29 真机实测（用户那块 14.6 GB U 盘切 512 MiB exFAT）：
//! 解 300 MB 到 U 盘，屏幕报 `✅ 解开（300.0 MB，用时 0.3 秒）`、退出码 0，
//! 而同一刻内核里 `Dirty: 307576 kB / Writeback: 0 kB` —— 数据全在页缓存里，
//! 盘上一个字节都还没写。此刻拔盘，重插后 `fsck.exfat` 报
//! `clean. directories 1, files 0`：解压目录根本不存在，300 MB 一个文件不剩。
//! 真落盘要约 200 秒（该盘持续写速 ≈1.5 MB/s），与提示差 **660 倍**。
//!
//! # 这不是本工具特有的毛病，也不该默认替用户扛
//!
//! `cp` / `rsync` / `unzip` / GNOME 文件管理器全都不 `fsync`
//! （SuperUser 1718589、Unix.SE 560908 是同一个问题的两次提问），
//! 系统的规范做法是**先「安全弹出」再拔**。所以本模块：
//!
//! - **默认关闭**（配置项 `[safety] flush_movable`，见 [`crate::config::Safety`]）
//! - 只在目标确实在**移动设备**上时才生效（本机硬盘一律不等，一秒都不多花）
//! - 打开后界面必须同时显示「请勿拔出设备」—— 等待期间硬拔照样会丢，
//!   这一点本模块管不了，只能靠界面把话说明白
//!
//! # 边界（做不到的事写清楚，免得以后当成 bug）
//!
//! - 「还剩多少待写入」读的是**全系统**脏页（`/proc/meminfo` 的 Dirty + Writeback），
//!   不是只算我们这一份。解压刚结束时脏页大头就是它，够用作进度读数。
//! - Windows / macOS 走 [`FlushOutcome::Unsupported`]：本批只做 Linux。
//! - 用户点取消就立刻返回，不等 `sync` 收尾 —— 数据可能还没写完，界面要如实说。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use crate::progress::CancelToken;

/// 轮询间隔：200 毫秒
///
/// 太稀了界面像卡死，太密了白烧 CPU（读一次 `/proc/meminfo` 很便宜，但没必要每帧读）。
pub const TICK: Duration = Duration::from_millis(200);

/// 等落盘这件事的结局
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlushOutcome {
    /// 数据已确认写进设备（`sync` 正常返回）
    Done,
    /// 用户取消了等待（**数据可能还没写完**，外壳要如实提示）
    Cancelled,
    /// 这台机器上没法等（找不到 `sync` 命令 / 平台不支持 / 目标目录不存在）
    Unsupported,
}

impl FlushOutcome {
    /// 稳定标识（JSON / 日志用）
    pub fn as_str(self) -> &'static str {
        match self {
            FlushOutcome::Done => "DONE",
            FlushOutcome::Cancelled => "CANCELLED",
            FlushOutcome::Unsupported => "UNSUPPORTED",
        }
    }
}

// ───────────────────────── ① 这台目标在不在移动设备上 ─────────────────────────

/// 目标目录所在的块设备名（`maj:min`，如 `8:17`）
///
/// 做法：拿真实路径去 `/proc/self/mountinfo` 里找**最长匹配**的挂载点。
/// 必须最长匹配 —— `/media/u` 和 `/media/u/disk` 同时挂着时，要认后者。
pub fn mount_majmin_for(mountinfo: &str, dest: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() < 5 {
            continue;
        }
        let majmin = f[2];
        let mount_point = unescape_mount(f[4]);
        if dest.starts_with(&mount_point) {
            let len = mount_point.as_os_str().len();
            if best.as_ref().is_none_or(|(l, _)| len >= *l) {
                best = Some((len, majmin.to_string()));
            }
        }
    }
    best.map(|(_, m)| m)
}

/// 还原 mountinfo 里的八进制转义（空格 `\040` / 制表 `\011` / 换行 `\012` / 反斜杠 `\134`）
fn unescape_mount(s: &str) -> PathBuf {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            let oct = &s[i + 1..i + 4];
            if let Ok(v) = u8::from_str_radix(oct, 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

/// 判断某个 `maj:min` 对应的设备是不是**移动设备**（U 盘 / USB 移动硬盘）
///
/// 两条判据，任一命中即算：
/// 1. sysfs 真实路径里带 `/usb` —— 走 USB 总线的基本就是U盘/移动硬盘
///    （**移动硬盘的 `removable` 通常是 0，只看它会把移动硬盘漏掉**）
/// 2. 整盘的 `removable` 标记是 `1` —— 覆盖读卡器之类不走 USB 总线的
///
/// `sys_root` 是给测试用的（真机传 `/sys`），好造一棵假的 sysfs 树。
pub fn device_is_removable(sys_root: &Path, majmin: &str) -> Option<bool> {
    let link = sys_root.join("dev/block").join(majmin);
    let real = std::fs::canonicalize(&link).ok()?;
    if real.to_string_lossy().contains("/usb") {
        return Some(true);
    }
    // 分区（sdb1）的 removable 在整盘目录（sdb）上，两级都查一遍
    let mut cands = Vec::new();
    if let Some(disk) = real.parent() {
        cands.push(disk.join("removable"));
    }
    cands.push(real.join("removable"));
    for c in cands {
        if let Ok(s) = std::fs::read_to_string(&c) {
            return Some(s.trim() == "1");
        }
    }
    Some(false)
}

/// 解压目标是不是在移动设备上（非 Linux 一律 `false`）
pub fn is_removable(dest: &Path) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Ok(real) = std::fs::canonicalize(dest) else {
        return false;
    };
    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    let Some(majmin) = mount_majmin_for(&mountinfo, &real) else {
        return false;
    };
    device_is_removable(Path::new("/sys"), &majmin).unwrap_or(false)
}

// ───────────────────────── ② 还剩多少没真写进盘 ─────────────────────────

/// 从 `/proc/meminfo` 文本里算出「待写入字节数」（Dirty + Writeback）
///
/// 纯函数，方便单测。两个字段的单位都是 kB。
pub fn parse_meminfo_pending(text: &str) -> u64 {
    let mut dirty = 0u64;
    let mut writeback = 0u64;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let Some(val) = it.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        match key {
            "Dirty:" => dirty = val,
            "Writeback:" => writeback = val,
            _ => {}
        }
    }
    (dirty + writeback).saturating_mul(1024)
}

/// 当前系统里还没真写进设备的字节数（读不到就当 0）
pub fn pending_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .map(|t| parse_meminfo_pending(&t))
        .unwrap_or(0)
}

/// 「顺手把产物落盘」的**统一入口**
///
/// 解压、打包、救援、封面提取 —— 四条写盘路径都该受同一个开关管。
/// 2026-10-30 的教训：一开始只有解压那条接了这个开关，
/// 打包到 U 盘报成功后拔盘照样全灭（而且它的"回读自检"读的是页缓存，
/// **自检通过 ≠ 数据在盘上**，比没自检更会骗人）。
///
/// 返回 `None` 表示**什么都不做**（开关关着 / 目标在本机盘 / 平台不支持）——
/// 一分钟都不多等。返回 `Some((结局, 毫秒))` 表示真的等了。
/// `cap`：本次产物一共多少字节。内核读数（[`pending_bytes`]）是**全系统**脏页，
/// 别的程序也在写盘时会虚高（实测见过"打了个 15 MB 的包，界面显示还剩 472 MB"）。
/// 传了就按它封顶，用户看到的数字才跟自己的文件对得上；传 0 表示不封顶。
pub fn flush_if_needed<F: FnMut(u64)>(
    dest_hint: &Path,
    cap: u64,
    cancel: &CancelToken,
    on_progress: &mut F,
) -> Option<(FlushOutcome, u64)> {
    if !crate::config::全局配置().safety.flush_movable {
        return None;
    }
    if !is_removable(dest_hint) {
        return None;
    }
    let t0 = std::time::Instant::now();
    let r = wait_until_written_at(dest_hint, cancel, TICK, &mut |p| {
        on_progress(if cap > 0 { p.min(cap) } else { p });
    });
    let millis = t0.elapsed().as_millis().min(u64::MAX as u128) as u64;
    Some((r, millis))
}

// ───────────────────────── ③ 等它写完 ─────────────────────────

/// 阻塞等目标目录所在文件系统写完，边等边回调「还剩多少」
///
/// 进度回调里拿到的是**真实读数**（[`pending_bytes`]），会一路往下降；
/// 回调返回后调用方应把它转成事件发给外壳。
pub fn wait_until_written<F: FnMut(u64)>(dest: &Path, cancel: &CancelToken, mut on_progress: F) -> FlushOutcome {
    wait_until_written_at(dest, cancel, TICK, &mut on_progress)
}

/// 同 [`wait_until_written`]，但轮询间隔可调（测试用短间隔）
pub fn wait_until_written_at<F: FnMut(u64)>(
    dest: &Path,
    cancel: &CancelToken,
    tick: Duration,
    on_progress: &mut F,
) -> FlushOutcome {
    if !cfg!(target_os = "linux") || !dest.exists() {
        return FlushOutcome::Unsupported;
    }
    // 先报一次真实读数，界面立刻就有东西可显示（而不是白等 200ms）
    on_progress(pending_bytes());

    let (tx, rx) = mpsc::channel::<bool>();
    let target = dest.to_path_buf();
    std::thread::spawn(move || {
        let ok = sync_now(&target);
        let _ = tx.send(ok);
    });

    loop {
        if cancel.is_cancelled() {
            // sync 线程还在跑，它会自己结束；这里只管不再让用户干等
            return FlushOutcome::Cancelled;
        }
        match rx.try_recv() {
            Ok(true) => {
                on_progress(pending_bytes());
                return FlushOutcome::Done;
            }
            Ok(false) => return FlushOutcome::Unsupported,
            Err(TryRecvError::Empty) => {
                on_progress(pending_bytes());
                std::thread::sleep(tick);
            }
            Err(TryRecvError::Disconnected) => return FlushOutcome::Unsupported,
        }
    }
}

/// 真正干活：同步目标所在的文件系统
///
/// 优先 `sync -f <目标>`（coreutils ≥ 8.24，只刷这一个文件系统，不牵连别的盘）；
/// 老系统上 `-f` 不认就退回全量 `sync` —— 会多刷别的挂载点，慢一点但结果正确。
fn sync_now(dest: &Path) -> bool {
    let only_this_fs = std::process::Command::new("sync")
        .arg("-f")
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if only_this_fs {
        return true;
    }
    std::process::Command::new("sync")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
25 30 0:23 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
36 35 8:17 / /media/u rw,nosuid,nodev,relatime shared:120 - exfat /dev/sdb1 rw
37 35 8:17 / /media/u/disk rw,nosuid,nodev,relatime shared:121 - exfat /dev/sdb1 rw
40 35 8:34 / /media/\\040有空格 rw,relatime shared:130 - exfat /dev/sdc1 rw
41 35 0:26 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
";

    #[test]
    fn 最长匹配挂载点() {
        // 同时挂了 /media/u 和 /media/u/disk，要认更深的那个
        assert_eq!(
            mount_majmin_for(MOUNTINFO, Path::new("/media/u/disk/a/b.zip")),
            Some("8:17".into())
        );
        // 根目录回落到 ext4
        assert_eq!(
            mount_majmin_for(MOUNTINFO, Path::new("/home/x/a.zip")),
            Some("0:26".into())
        );
        // 带空格的挂载点要能还原 \040
        assert_eq!(
            mount_majmin_for(MOUNTINFO, Path::new("/media/ 有空格/a.zip")),
            Some("8:34".into())
        );
        // 不在任何挂载点里（真机不会出现，纯防御）
        assert_eq!(mount_majmin_for("", Path::new("/x")), None);
    }

    #[test]
    fn meminfo_待写入读数() {
        let t = "MemTotal:       16000 kB\nDirty:            100 kB\nWriteback:         20 kB\n";
        assert_eq!(parse_meminfo_pending(t), (100 + 20) * 1024);
        // 字段缺失当 0，不 panic
        assert_eq!(parse_meminfo_pending("Dirty: 1 kB\n"), 1024);
        assert_eq!(parse_meminfo_pending(""), 0);
        // 脏数据不炸
        assert_eq!(parse_meminfo_pending("Dirty: abc kB\n"), 0);
    }

    /// 造一棵假 sysfs，验「是不是移动设备」的两条判据
    ///
    /// 只在 Linux 跑：`device_is_removable` 的判据就是 sysfs，别的平台直接返回 None；
    /// 这里还用了 unix symlink，Windows 上连编译都过不去（2026-11-05 就踩过）。
    #[cfg(target_os = "linux")]
    #[test]
    fn 移动设备判定() {
        let root = std::env::temp_dir().join(format!("yshjy-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        // ① USB 上的分区：路径带 /usb → 移动设备（移动硬盘 removable=0 也算）
        let usb_disk = root.join("devices/pci0000:00/usb1/1-1/1-1:1.0/host6/block/sdb");
        std::fs::create_dir_all(usb_disk.join("sdb1")).unwrap();
        std::fs::write(usb_disk.join("removable"), "0").unwrap();
        std::fs::create_dir_all(root.join("dev/block")).unwrap();
        std::os::unix::fs::symlink(
            usb_disk.join("sdb1"),
            root.join("dev/block/8:17"),
        )
        .unwrap();

        // ② 内置 SATA 盘：路径没 /usb、removable=0 → 不是移动设备
        let sata_disk = root.join("devices/pci0000:00/ata1/host0/block/sda");
        std::fs::create_dir_all(sata_disk.join("sda2")).unwrap();
        std::fs::write(sata_disk.join("removable"), "0").unwrap();
        std::os::unix::fs::symlink(sata_disk.join("sda2"), root.join("dev/block/8:2")).unwrap();

        // ③ 读卡器：不走 USB，但整盘 removable=1 → 移动设备
        let card_disk = root.join("devices/platform/mmc/mmcblk0");
        std::fs::create_dir_all(card_disk.join("mmcblk0p1")).unwrap();
        std::fs::write(card_disk.join("removable"), "1").unwrap();
        std::os::unix::fs::symlink(card_disk.join("mmcblk0p1"), root.join("dev/block/179:1"))
            .unwrap();

        assert_eq!(device_is_removable(&root, "8:17"), Some(true), "USB 移动硬盘");
        assert_eq!(device_is_removable(&root, "8:2"), Some(false), "内置 SATA");
        assert_eq!(device_is_removable(&root, "179:1"), Some(true), "读卡器");
        assert_eq!(device_is_removable(&root, "9:99"), None, "设备不存在");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 本机的工作目录绝不该被判成移动设备（否则内部盘也会白等）
    #[test]
    fn 本机目录不是移动设备() {
        let d = std::env::temp_dir();
        assert!(!is_removable(&d), "临时目录被判成移动设备了：{}", d.display());
    }

    /// 端到端：在真文件系统上等一次，应该 Done 且能收到读数
    ///
    /// 只在 Linux 跑：`wait_until_written_at` 的非 Linux 分支直接返回 Unsupported
    ///（读不到 Dirty/Writeback，干等没有意义）。
    #[cfg(target_os = "linux")]
    #[test]
    fn 等落盘能跑通() {
        let dir = std::env::temp_dir().join(format!("yshjy-flush-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.bin"), vec![7u8; 4096]).unwrap();

        let cancel = CancelToken::new();
        let mut ticks = 0u64;
        let r = wait_until_written_at(&dir, &cancel, Duration::from_millis(20), &mut |_p| {
            ticks += 1;
        });
        assert_eq!(r, FlushOutcome::Done, "本机应该能正常等完");
        assert!(ticks >= 1, "至少要报过一次读数");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 取消要立刻生效，不能把用户吊在那儿
    ///
    /// 只在 Linux 跑：非 Linux 在进循环之前就返回 Unsupported（见上一条）。
    #[cfg(target_os = "linux")]
    #[test]
    fn 取消立刻返回() {
        let dir = std::env::temp_dir().join(format!("yshjy-flush-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = wait_until_written_at(&dir, &cancel, Duration::from_millis(5), &mut |_p| {});
        assert_eq!(r, FlushOutcome::Cancelled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 开关关着 → `flush_if_needed` 必须**什么都不做**（一秒都不多等）
    ///
    /// 这是整个特性的第一条铁律：默认关就是默认关，不能偷偷等。
    #[test]
    fn 开关关着不落盘() {
        let dir = std::env::temp_dir().join(format!("yshjy-fi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cancel = CancelToken::new();
        // 默认配置 = 关（全局配置读不到文件时就是默认值）
        let r = flush_if_needed(&dir, 0, &cancel, &mut |_p| {});
        assert!(r.is_none(), "开关关着却做了落盘检验");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目标不存在 → 不支持（不是报错、不是卡住）
    #[test]
    fn 目标不存在返回不支持() {
        let cancel = CancelToken::new();
        let r = wait_until_written_at(
            Path::new("/nonexistent/yshjy-test"),
            &cancel,
            Duration::from_millis(5),
            &mut |_p| {},
        );
        assert_eq!(r, FlushOutcome::Unsupported);
    }
}
