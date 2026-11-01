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
use std::sync::Arc;

use crate::ask::{AskGate, 答案};

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
    /// 超过限额时「问用户」（`None` = 直接失败：测试 / 无人值守）
    ///
    /// 有闸门时：命中限额 → 发 `ASK_USER` 事件并阻塞等答案（超时按跳过）。
    /// 一次任务只问一次，答案「继续」后本次任务不再检查这条限额。
    pub ask: Option<Arc<AskGate>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_extract_bytes: 0,
            max_ratio: 100,
            max_path_len: 512,
            warn_executables: true,
            ask: None,
        }
    }
}

/// 压缩比检查的起检门槛：本次任务写出 ≥ 64 MiB 之后才判比例。
///
/// 小包的比例没有统计意义（见 [`ExtractBudget::record`] 里的实测例子：
/// `a.txt`(11 B) + `b.bin`(5000 相同字节) 整包 40 B，比例 125:1 纯属误报）。
///
/// ★ 2026-11-02：默认倍数改成 0（不检查，和 7-Zip / PeaZip 一致）之后，
///   这个门槛只在**用户主动设了档位**时才起作用 —— 那时他明确要这道闸，
///   64 MiB 起判是他能接受的粒度。别把门槛抬到 1 GiB：那会让「设了 100 倍」
///   在几百 MB 的文件上完全失效，等于一个假开关（实测踩过）。
pub const RATIO_CHECK_MIN_WRITTEN: u64 = 64 * 1024 * 1024;

/// 磁盘空间的检查结论
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceCheck {
    /// 够用
    Enough {
        /// 目标盘可用字节
        available: u64,
        /// 预估需要字节
        need: u64,
    },
    /// 不够
    NotEnough {
        /// 目标盘可用字节
        available: u64,
        /// 预估需要字节
        need: u64,
    },
    /// 预估不出来 → **不拦**
    Unknown,
}

/// 解压前看看目标盘够不够（体感最直接的护栏：14 GB 的包解到一半磁盘满，前面的时间全白费）
///
/// # 为什么 `need` 为 0 时放行
///
/// `need` 来自预览的总大小。有些格式给不出来（RAR 预览没有条目大小、单流格式不带原始尺寸）。
/// 这种时候**宁可放行**让用户自己承担风险，也不要因为"我猜不准"就拒绝一次正常解压 ——
/// 假拒绝比漏拒更让人恼火，而且用户没法绕过。
///
/// # 为什么留 5% 余量
///
/// 文件系统元数据、已有的临时文件、以及预估与实际的偏差都要地方落。预估得刚刚好 =
/// 大概率还是差一点。
pub fn check_space(dest: &Path, need: u64) -> SpaceCheck {
    if need == 0 {
        return SpaceCheck::Unknown;
    }
    // 目标目录可能还不存在 → 往上找第一个存在的祖先（磁盘是同一个）
    let mut probe = dest.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(p) if !p.as_os_str().is_empty() => probe = p.to_path_buf(),
            _ => break,
        }
    }
    // 查不到可用空间（不支持的挂载 / 权限）→ 别拦人
    let Ok(available) = fs4::available_space(&probe) else {
        return SpaceCheck::Unknown;
    };
    let need_with_margin = need.saturating_add(need / 20);
    if available >= need_with_margin {
        SpaceCheck::Enough { available, need }
    } else {
        SpaceCheck::NotEnough { available, need }
    }
}

/// Windows 保留设备名（不区分大小写，带扩展名也算）
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 要不要提醒用户"解出了可执行文件"
///
/// 提醒开关来自配置（`[safety] warn_executables`，默认开）。
/// 之所以单独一层：**"是不是可执行文件"和"要不要提醒"是两件事** ——
/// 列表里照样会给可执行条目打标记（那是信息），但解压时可以按配置闭嘴。
pub fn should_warn_executable(limits: &Limits, name: &str) -> bool {
    limits.warn_executables && is_risky_executable(name)
}

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
    /// 本次任务是否已经问过用户（问过就不再问，避免每 1 MB 弹一次）
    已问: bool,
}

impl ExtractBudget {
    /// 新建（`packed_total` 为压缩数据总量，用于比值检查；0 表示不检查）
    pub fn new(limits: Limits, packed_total: u64) -> Self {
        Self {
            limits,
            written: 0,
            packed_total,
            已问: false,
        }
    }

    /// 记录写入，超限即报错（有询问闸门时先问用户）
    pub fn record(&mut self, bytes: u64) -> Result<()> {
        self.written = self.written.saturating_add(bytes);

        if self.limits.max_extract_bytes > 0 && self.written > self.limits.max_extract_bytes {
            let 上限 = self.limits.max_extract_bytes;
            self.问用户("extract_size_limit_exceeded", 上限)?;
            // 用户说继续 → 本次任务解除这条限制（问过一次就不再打扰）
            self.limits.max_extract_bytes = 0;
        }

        // 压缩比只对**够大的样本**有意义。
        //
        // 分母是整包压缩总量，小包里一个高压缩比条目会让比例瞬间爆表：
        // 实测 `a.txt`(11 B) + `b.bin`(5000 个相同字节)，整包压缩后约 40 B，
        // 解 b.bin 时 5000/40 = 125 > 100 → 被当成炸弹、报 DATA_ERROR，
        // 而 `unzip -t` 完全正常 —— 这是纯误报（用户会以为包坏了）。
        //
        // 真正的压缩炸弹解出的是 GB 级数据，等到 64 MiB 再判照样拦得住；
        // 「拿不准就放行」的项目原则也指向这个阈值。
        if self.limits.max_ratio > 0
            && self.packed_total > 0
            && self.written >= RATIO_CHECK_MIN_WRITTEN
        {
            let ratio = self.written / self.packed_total.max(1);
            if ratio > self.limits.max_ratio {
                let 上限 = self.limits.max_ratio;
                self.问用户("possible_zip_bomb", 上限)?;
                self.limits.max_ratio = 0;
            }
        }

        Ok(())
    }

    /// 超过限额：有闸门就问，没有就按原来的硬失败
    ///
    /// - 继续 → `Ok(())`，调用方放宽对应限额
    /// - 跳过 → 取消语义（批量里继续下一个）
    /// - 取消 → 同上，但 reason 不同（外壳可区分文案）
    /// - 超时 / 前端断开 → 闸门返回「跳过」（`AskGate` 内部保证）
    fn 问用户(&mut self, 原因: &str, 上限: u64) -> Result<()> {
        // ★ 这里以前两条分支都返回 `DataLost`（「中间有一段数据丢了」）——
        //   用户以为包坏了，跑去重新下载。实际上包完好，只是**我们自己的安全限额**拦了。
        //   实测证据：100 MB 全零文件的普通 zip（`unzip` 解得干干净净）压成 95 KB，
        //   压缩比 1028:1 > 默认上限 100:1 → 被拦 → 报「数据损坏」。把上限调到 2000
        //   同一个包立刻解开、文件 100,000,000 字节完整。
        //   现在改成专门的 `SafetyLimit`：外壳能说清「这是限额，不是损坏」，并且
        //   能把 ratio / limit 两个数字摆出来给用户看（参考 `NOT_ENOUGH_SPACE` 的处理方式）。
        let 硬失败 = |b: &Self| {
            if 原因 == "possible_zip_bomb" {
                Error::new(ErrorCode::SafetyLimit)
                    .with("reason", 原因)
                    .with("ratio", (b.written / b.packed_total.max(1)).to_string())
                    .with("written", b.written)
                    .with("limit", 上限)
            } else {
                Error::new(ErrorCode::SafetyLimit)
                    .with("reason", 原因)
                    .with("written", b.written)
                    .with("limit", 上限)
            }
        };
        let Some(门) = self.limits.ask.clone() else {
            return Err(硬失败(self));
        };
        if self.已问 {
            return Err(硬失败(self));
        }
        self.已问 = true;
        match 门.问(原因, self.written, 上限) {
            答案::继续 => Ok(()),
            答案::跳过 => Err(Error::new(ErrorCode::Cancelled)
                .with("reason", "skipped_by_user")
                .with("written", self.written)),
            答案::取消 => Err(Error::new(ErrorCode::Cancelled)
                .with("reason", "cancelled_by_user")
                .with("written", self.written)),
        }
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
        // 起检门槛之前不判比例：小样本的比值没有统计意义
        assert!(b.record(500).is_ok());
        assert!(b.record(600).is_ok());
        // 过了起检门槛（RATIO_CHECK_MIN_WRITTEN），累计/压缩总量远超 10 倍 → 必须拦
        assert!(b.record(RATIO_CHECK_MIN_WRITTEN).is_err());
    }

    #[test]
    fn 小包的高压缩比不算炸弹() {
        // 回归：a.txt(11 B) + b.bin(5000 个相同字节)，整包压缩总量约 40 B。
        // 旧逻辑在 5000/40=125 > 100 时误报 DATA_ERROR（unzip -t 全好）。
        let mut b = ExtractBudget::new(Limits::default(), 40);
        assert!(b.record(11).is_ok(), "小文件正常");
        assert!(b.record(5000).is_ok(), "小包不该被判成压缩炸弹");
    }

    #[test]
    fn 空间预检的三种结论() {
        // ① need = 0（预估不出来）→ 不拦 —— 宁可放行也不假拒绝
        assert_eq!(check_space(Path::new("."), 0), SpaceCheck::Unknown);

        // ② 只要 1 KB → 肯定够
        assert!(
            matches!(check_space(Path::new("."), 1024), SpaceCheck::Enough { .. }),
            "1 KB 都不够的话这机器没法用了"
        );

        // ③ 要 1 EiB → 谁都不够
        match check_space(Path::new("."), 1 << 60) {
            SpaceCheck::NotEnough { available, need } => {
                assert_eq!(need, 1 << 60);
                assert!(available < need, "可用空间不该有 1 EiB 那么大");
            }
            other => panic!("应当判定为不够，实际 {other:?}"),
        }
    }

    #[test]
    fn 空间预检能处理还不存在的目标目录() {
        // 目标目录还没建 → 往上找存在的祖先，仍然要能查到磁盘可用空间
        let p = std::env::temp_dir().join("yshjy_还没建的目录_xyz/更深的");
        match check_space(&p, 1024) {
            SpaceCheck::Enough { .. } => {}
            other => panic!("应当能顺着父目录找到磁盘，实际 {other:?}"),
        }
    }

    #[test]
    fn 空间预检留了余量() {
        // 刚好等于可用空间 → 因为留了 5% 余量，应当判不够
        let probe = std::env::temp_dir();
        let Ok(avail) = fs4::available_space(&probe) else {
            return; // 这环境查不到就算了
        };
        assert!(
            matches!(
                check_space(&probe, avail),
                SpaceCheck::NotEnough { .. }
            ),
            "预估刚好等于可用空间时必须留余量，否则解到最后一刻才失败"
        );
        assert!(
            matches!(
                check_space(&probe, avail / 2),
                SpaceCheck::Enough { .. }
            ),
            "只要一半可用空间就该放行"
        );
    }

    // ── 回归：安全限额 ≠ 数据损坏（2026-11-01 实测出来的真 bug）──
    //
    // 起因：一个普通 zip 里装 100MB 全零文件（压缩后 95KB，`unzip` 解得很干净），
    // 被压缩比闸拦下后**报成「中间有一段数据丢了」**，用户会以为包坏了去重新下载。
    // 实测边界精确在 67,108,864 字节（= RATIO_CHECK_MIN_WRITTEN），
    // 把上限调到 2000:1 同一个包立刻解开、100,000,000 字节完整。
    //
    // 这条测试钉两件事：① 码必须是 SafetyLimit（不是 DataLost）
    //                 ② 起检门槛之下不许拦（小包的比例没统计意义）

    fn 预算(max_ratio: u64) -> ExtractBudget {
        ExtractBudget::new(
            Limits {
                max_ratio,
                max_extract_bytes: 0,
                max_path_len: 512,
                warn_executables: true,
                ask: None,
            },
            95_000, // 压缩后 95KB
        )
    }

    #[test]
    fn 压缩比超限报安全限额_不报数据损坏() {
        let mut b = 预算(100);
        // 起判门槛 64 MiB，70 MiB 已经过线；70MiB / 95KB ≈ 772:1 > 100
        let e = b
            .record(70 * 1024 * 1024)
            .expect_err("70MiB 配 95KB 的包必然超 100:1");
        assert_eq!(
            e.code,
            ErrorCode::SafetyLimit,
            "报成 {:?} 就是又回到「数据损坏」那条错路上了",
            e.code
        );
        // 外壳要能直接摆出这两个数字给用户看
        let d: std::collections::HashMap<_, _> = e.detail.iter().cloned().collect();
        assert_eq!(d.get("limit").map(String::as_str), Some("100"));
        assert!(d.contains_key("ratio"), "必须带比例，否则用户不知道差多少");
    }

    #[test]
    fn 门槛以下的写入不查比例_小包不该误报() {
        // 只写了 1MiB（远低于 64MiB 起检线）—— 比例再高也不该拦
        let mut b = 预算(100);
        assert!(b.record(1024 * 1024).is_ok(), "小包的比例没有统计意义，不许拦");
    }

    #[test]
    fn 上限设为零等于关掉这道闸() {
        // 0 = 不检查（配置里能关），大包也不该拦
        let mut b = 预算(0);
        assert!(b.record(200 * 1024 * 1024).is_ok());
    }
}
