// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 硬件与资源档位
//!
//! # 为什么需要它
//! 实测（8.7 GB 真实文件，机械硬盘）：
//! `扫描耗时 160 秒`，其中 `CPU 时间只有 59 秒` —— 也就是说**瓶颈是硬盘读取**，
//! 不是算法、更不是 CPU。机械盘的物理极限（实测 83.6 MB/s）决定了
//! "全文件扫描"必然要几分钟。
//!
//! 所以正确的做法不是"让用户换个快电脑"，而是：
//! 1. **检测硬件**（磁盘类型/核心数/内存），据此选默认策略
//! 2. **把资源占用做成档位**让用户自己选（对齐 7-Zip `-mmt`、WinRAR 多线程开关的做法）
//!
//! # 对齐主流工具
//! - 7-Zip：`-mmt=[off|on|N]`，默认取 CPU 核心数（`HandlerOut.h:29`），并有硬上限
//! - WinRAR：设置→系统→多线程，多核机器默认开
//! - Linux 通用：`nice`（CPU 优先级）、`ionice`（磁盘优先级）
//!
//! 我们的差异点：因为瓶颈常在磁盘，我们额外提供**扫描深度**档位。

use std::path::Path;

/// 资源档位 —— 用户可调，决定"花多少资源换多少彻底性"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceProfile {
    /// 省资源：只做最划算的一步（毫秒级），适合低配机/后台/省电
    Low,
    /// 默认：快速定位不行再升级到取样扫描（秒级）
    #[default]
    Balanced,
    /// 全力：允许全文件扫描（HDD 上可能几分钟，最彻底）
    High,
}

impl ResourceProfile {
    /// 中文标签（给界面用；引擎侧只提供语义）
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "省资源",
            Self::Balanced => "平衡",
            Self::High => "最彻底",
        }
    }

    /// 是否允许全文件扫描
    pub fn allows_full_scan(self) -> bool {
        matches!(self, Self::High)
    }

    /// 扫描深度：越大越彻底
    pub fn depth(self) -> ScanDepth {
        match self {
            Self::Low => ScanDepth::TailWindowOnly,
            Self::Balanced => ScanDepth::Sampled,
            Self::High => ScanDepth::Full,
        }
    }
}

/// 扫描深度（策略层的三档，与 [`ResourceProfile`] 一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDepth {
    /// 只看尾部窗口（最快，覆盖绝大多数"视频尾巴接压缩包"的情况）
    TailWindowOnly,
    /// 头 + 尾 + 中间若干采样点（覆盖"包插在中间"）
    Sampled,
    /// 全文件（最彻底，也最慢 —— HDD 上是分钟级）
    Full,
}

/// 磁盘类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKind {
    /// 固态硬盘（随机/顺序都快，可以放心全扫）
    SolidState,
    /// 机械硬盘（顺序读受物理限制，全扫要等很久）
    Rotational,
    /// 未知（保守处理，按机械盘对待）
    Unknown,
}

impl DiskKind {
    /// 机器标识（协议里用；展示名归前端语言包，别在这里塞中文）
    ///
    /// 用字符串而不是布尔：将来要区分 U 盘 / 网络盘（`removable` / `network`）时，
    /// 前端不用改代码就知道多了一档。
    pub fn as_str(self) -> &'static str {
        match self {
            DiskKind::SolidState => "solid",
            DiskKind::Rotational => "rotational",
            DiskKind::Unknown => "unknown",
        }
    }
}

/// 采集到的硬件环境
#[derive(Debug, Clone)]
pub struct Hardware {
    /// 逻辑 CPU 核心数
    pub cpu_cores: usize,
    /// 可用内存（MiB；读不到时为 `None`）
    pub available_mem_mib: Option<u64>,
    /// 承载目标文件的磁盘类型
    pub disk_kind: DiskKind,
}

impl Hardware {
    /// 探测当前机器
    ///
    /// 全部采用"探测不到就降级"策略 —— 绝不因为读不到某个文件而报错。
    pub fn detect() -> Self {
        Self {
            cpu_cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            available_mem_mib: read_available_mem_mib(),
            disk_kind: DiskKind::Unknown,
        }
    }

    /// 针对某个具体文件补全磁盘类型（探测失败则保持 Unknown）
    pub fn with_target<P: AsRef<Path>>(mut self, _path: P) -> Self {
        // 磁盘类型通过 /sys/block/<dev>/queue/rotational 判断；
        // 需要把文件路径映射到块设备，Linux 上用 /proc/mounts 找最长匹配前缀。
        self.disk_kind = detect_disk_kind(_path.as_ref()).unwrap_or(DiskKind::Unknown);
        self
    }

    /// **根据硬件给出推荐档位**（这就是"检测硬件用不同方式"的落地）
    ///
    /// 规则：
    /// - 固态盘 + 内存充足 → 默认就给"最彻底"（反正是秒级，别让用户多点一次）
    /// - 机械盘 → 默认"平衡"（全扫要几分钟，得用户主动要求）
    /// - 核心数/内存很少 → 降一档，别把老机器拖死
    pub fn recommend_profile(&self, file_size: u64) -> ResourceProfile {
        let small_file = file_size < 512 * 1024 * 1024; // < 512 MiB
        let low_end = self.cpu_cores <= 2 || self.available_mem_mib.is_some_and(|m| m < 2048);

        match self.disk_kind {
            // 固态盘：全扫也很快，小文件直接给最彻底
            DiskKind::SolidState => {
                if low_end {
                    ResourceProfile::Balanced
                } else {
                    ResourceProfile::High
                }
            }
            // 机械盘：小文件可以放手全扫，大文件默认平衡
            DiskKind::Rotational | DiskKind::Unknown => {
                if low_end {
                    ResourceProfile::Low
                } else if small_file {
                    ResourceProfile::High
                } else {
                    ResourceProfile::Balanced
                }
            }
        }
    }

    /// 估算"全文件扫描"的耗时下限（秒）—— 用于提前告诉用户"要花几分钟"
    ///
    /// 依据实测：机械盘约 84 MB/s，固态盘约 500 MB/s（保守取值）。
    pub fn estimate_full_scan_secs(&self, file_size: u64) -> u64 {
        let mb_per_s: u64 = match self.disk_kind {
            DiskKind::SolidState => 500,
            DiskKind::Rotational => 84, // 实测值
            DiskKind::Unknown => 150,
        };
        let mib = file_size / (1024 * 1024);
        (mib / mb_per_s).max(1)
    }

    /// 人类可读的一句话摘要（给界面显示用）
    pub fn summary(&self) -> String {
        let disk = match self.disk_kind {
            DiskKind::SolidState => "固态硬盘",
            DiskKind::Rotational => "机械硬盘",
            DiskKind::Unknown => "未知磁盘",
        };
        let mem = match self.available_mem_mib {
            Some(m) => format!("{} MiB 可用内存", m),
            None => "内存未知".to_string(),
        };
        format!("{} 核 / {} / {}", self.cpu_cores, mem, disk)
    }
}

// 「这个位置能不能跳着读」的探测放在 `seek` 模块（那里还有复制兜底），
// 这里转出来是因为方案 §C 与 `docs/ARCHITECTURE.md` F1 都写作 `hardware::seek_ability`。
pub use crate::seek::{seek_ability, SeekAbility};

/// 可用内存（MiB）—— 解压前预检用
pub fn available_mem_mib() -> Option<u64> {
    read_available_mem_mib()
}

/// 外壳注册的平台内存探针
///
/// 为什么绕这一道：Windows 读可用内存要 `GlobalMemoryStatusEx`（unsafe），
/// 而本 crate 是 `#![forbid(unsafe_code)]` —— 和时区偏移一样，「碰系统 API」的活
/// 只能归 bin（见 `main::windows可用内存MiB`），启动时注册、之后每次**实时**来取
/// （不存快照：预检比对的是「此刻」还剩多少内存）。
static 平台探针: std::sync::RwLock<Option<fn() -> Option<u64>>> = std::sync::RwLock::new(None);

/// 注册平台内存探针（外壳启动早期调用；重复注册以后一次为准）
///
/// Linux 不注册 —— `/proc/meminfo` 那条路本来就是实时的。
pub fn 注册内存探针(f: fn() -> Option<u64>) {
    if let Ok(mut g) = 平台探针.write() {
        *g = Some(f);
    }
}

/// 读可用内存（Linux/Android: /proc/meminfo 的 MemAvailable；Windows: 外壳注册的探针）
fn read_available_mem_mib() -> Option<u64> {
    if let Ok(g) = 平台探针.read()
        && let Some(f) = *g
    {
        return f();
    }
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kib: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kib / 1024);
        }
    }
    None
}

/// 外壳注册的磁盘探针
///
/// 为什么绕这一道：Windows 问「这块盘要不要寻道」要 `DeviceIoControl`
/// （unsafe），而本 crate 是 `#![forbid(unsafe_code)]` —— 和内存探针同一套做法：
/// 碰系统 API 的活归 bin，启动时注册，`detect_disk_kind` 先问探针。
/// Linux 不注册，继续走 `/proc/mounts` + `/sys`。
/// 磁盘探针的函数签名（单独起名，避免 clippy 的 type_complexity）
type 磁盘探针类型 = fn(&Path) -> Option<DiskKind>;

static 磁盘探针: std::sync::RwLock<Option<磁盘探针类型>> = std::sync::RwLock::new(None);

/// 注册平台磁盘探针（外壳启动早期调用；重复注册以后一次为准）
pub fn 注册磁盘探针(f: 磁盘探针类型) {
    if let Ok(mut g) = 磁盘探针.write() {
        *g = Some(f);
    }
}

/// 判断某路径所在磁盘是不是机械盘
///
/// 做法：在 `/proc/mounts` 里找**最长的**匹配挂载点 → 得到设备名 →
/// 查 `/sys/block/<dev>/queue/rotational`（1 = 机械，0 = 固态）。
/// Windows 走外壳注册的探针（见 [`注册磁盘探针`]）。
/// 任何一步失败都返回 `None`（调用方降级为 Unknown）。
fn detect_disk_kind(path: &Path) -> Option<DiskKind> {
    if let Ok(g) = 磁盘探针.read()
        && let Some(f) = *g
    {
        return f(path);
    }
    let abs = std::fs::canonicalize(path).ok()?;
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;

    let mut best: Option<(usize, String)> = None; // (挂载点长度, 设备)
    for line in mounts.lines() {
        let mut it = line.split_whitespace();
        let dev = it.next()?;
        let mnt = match it.next() {
            Some(m) => m,
            None => continue,
        };
        let mnt_path = Path::new(mnt);
        if abs.starts_with(mnt_path) {
            let len = mnt_path.as_os_str().len();
            if best.as_ref().is_none_or(|(l, _)| len > *l) {
                best = Some((len, dev.to_string()));
            }
        }
    }

    let dev = best?.1;
    // 只处理真实块设备（/dev/sdX、/dev/nvme0n1p2 等）
    let name = dev.strip_prefix("/dev/")?;
    // 分区名 → 父设备名（sda2 → sda，nvme0n1p2 → nvme0n1）
    let base = base_block_name(name);
    for cand in [name, base.as_str()] {
        let p = format!("/sys/block/{cand}/queue/rotational");
        if let Ok(v) = std::fs::read_to_string(&p) {
            return Some(match v.trim() {
                "0" => DiskKind::SolidState,
                "1" => DiskKind::Rotational,
                _ => DiskKind::Unknown,
            });
        }
    }
    None
}

/// 从分区名推出父块设备名：`sda2`→`sda`，`nvme0n1p2`→`nvme0n1`，`mmcblk0p1`→`mmcblk0`
fn base_block_name(name: &str) -> String {
    // NVMe / mmcblk 这类名字带 'p' 分隔
    if let Some(idx) = name.rfind('p')
        && name[idx + 1..].parse::<u32>().is_ok()
        && name[..idx].ends_with(|c: char| c.is_ascii_digit())
    {
        return name[..idx].to_string();
    }
    // 普通 sdX / vdX / hdX：去掉尾部连续数字
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        name.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认档位是平衡() {
        assert_eq!(ResourceProfile::default(), ResourceProfile::Balanced);
    }

    #[test]
    fn 档位与扫描深度一一对应() {
        assert_eq!(ResourceProfile::Low.depth(), ScanDepth::TailWindowOnly);
        assert_eq!(ResourceProfile::Balanced.depth(), ScanDepth::Sampled);
        assert_eq!(ResourceProfile::High.depth(), ScanDepth::Full);
    }

    #[test]
    fn 低档位不允许全扫() {
        assert!(!ResourceProfile::Low.allows_full_scan());
        assert!(!ResourceProfile::Balanced.allows_full_scan());
        assert!(ResourceProfile::High.allows_full_scan());
    }

    #[test]
    fn 分区名能推出父设备() {
        assert_eq!(base_block_name("sda2"), "sda");
        assert_eq!(base_block_name("sda"), "sda");
        assert_eq!(base_block_name("nvme0n1p2"), "nvme0n1");
        assert_eq!(base_block_name("mmcblk0p1"), "mmcblk0");
    }

    #[test]
    fn 机械盘上小文件推荐最彻底_大文件推荐平衡() {
        let hw = Hardware {
            cpu_cores: 8,
            available_mem_mib: Some(16000),
            disk_kind: DiskKind::Rotational,
        };
        assert_eq!(
            hw.recommend_profile(100 * 1024 * 1024),
            ResourceProfile::High
        );
        assert_eq!(
            hw.recommend_profile(8 * 1024 * 1024 * 1024),
            ResourceProfile::Balanced
        );
    }

    #[test]
    fn 固态盘默认最彻底() {
        let hw = Hardware {
            cpu_cores: 8,
            available_mem_mib: Some(16000),
            disk_kind: DiskKind::SolidState,
        };
        assert_eq!(
            hw.recommend_profile(8 * 1024 * 1024 * 1024),
            ResourceProfile::High
        );
    }

    #[test]
    fn 低配机器降档() {
        let hw = Hardware {
            cpu_cores: 2,
            available_mem_mib: Some(1500),
            disk_kind: DiskKind::Rotational,
        };
        assert_eq!(
            hw.recommend_profile(8 * 1024 * 1024 * 1024),
            ResourceProfile::Low
        );
    }

    #[test]
    fn 耗时估算在合理范围() {
        let hw = Hardware {
            cpu_cores: 8,
            available_mem_mib: Some(16000),
            disk_kind: DiskKind::Rotational,
        };
        // 8.7 GB 机械盘 ≈ 100 秒上下（实测物理极限 104 秒）
        let est = hw.estimate_full_scan_secs(8_711_817_691);
        assert!(
            (80..=120).contains(&est),
            "估算值 {est} 秒应接近实测 104 秒"
        );
    }

    #[test]
    fn 探测本机不会崩() {
        let hw = Hardware::detect();
        assert!(hw.cpu_cores >= 1);
        assert!(!hw.summary().is_empty());
        println!("本机: {}", hw.summary());
    }

    #[test]
    fn 探测目标文件的磁盘类型() {
        let hw = Hardware::detect().with_target(".");
        println!("当前目录所在磁盘: {:?}", hw.disk_kind);
        // 不强断言具体类型（CI 环境各异），只要求不 panic 且有结果
        assert!(matches!(
            hw.disk_kind,
            DiskKind::SolidState | DiskKind::Rotational | DiskKind::Unknown
        ));
    }
}
