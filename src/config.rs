//! 配置与密码本（见 docs/BACKEND-SPEC.md §2）
//!
//! # 为什么自己算路径、不引 `directories`
//!
//! 三个平台的规矩二十行能写清楚；这个工具要单文件跨平台分发，少一个依赖少一份风险。
//!
//! # 放哪儿
//!
//! ```text
//! Linux    $XDG_CONFIG_HOME/yshjy/  （没设就 ~/.config/yshjy/）
//! macOS    ~/Library/Application Support/yshjy/
//! Windows  %APPDATA%\yshjy\
//! ```
//!
//! 环境变量 `YSHJY_CONFIG_DIR` 可以整个换掉（测试和便携场景用）。
//!
//! # 设计原则：**读的时候宽容，写的时候规矩**
//!
//! - 文件不存在 → 全用默认值，不报错（用户没配置是常态）
//! - 某一段写错 → 那一段用默认值，**其余照常生效**（不因为一个笔误整个配置失效）
//! - 未知字段 → 忽略（用户多写了一句注释性字段不该报错）
//! - 路径里的 `~` 会展开（用户手写配置时习惯这么写）

use std::path::{Path, PathBuf};


/// 配置目录（`YSHJY_CONFIG_DIR` 优先，其次按平台惯例）
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var("YSHJY_CONFIG_DIR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(d);
    }
    if cfg!(target_os = "windows")
        && let Some(a) = std::env::var("APPDATA").ok().filter(|s| !s.is_empty())
    {
        return PathBuf::from(a).join("yshjy");
    }
    if cfg!(target_os = "macos")
        && let Some(h) = std::env::var("HOME").ok().filter(|s| !s.is_empty())
    {
        return PathBuf::from(h)
            .join("Library/Application Support")
            .join("yshjy");
    }
    // Linux / 其它 Unix：XDG
    if let Some(x) = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(x).join("yshjy");
    }
    if let Some(h) = std::env::var("HOME").ok().filter(|s| !s.is_empty()) {
        return PathBuf::from(h).join(".config").join("yshjy");
    }
    // 连 HOME 都没有（极端情况）：退到当前目录，至少能用
    PathBuf::from(".yshjy")
}

/// 配置文件路径
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// 把配置里的 `~` 展开成用户主目录
///
/// 走 [`crate::paths::home`] 而不是直接读 `HOME`：Windows 用户的主目录在 `USERPROFILE`，
/// 只认 `HOME` 的话配置里写 `dir = "~/出"` 在 Windows 上永远不展开（审计 W6）
pub fn 展开波浪号(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(h) = crate::paths::home()
    {
        return h.join(rest);
    }
    PathBuf::from(p)
}

// ───────────────────────────── 配置结构 ─────────────────────────────

/// 整份配置
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// 总体
    pub general: General,
    /// 密码相关
    pub password: Password,
    /// 输出相关
    pub output: Output,
    /// 安全限值
    pub safety: Safety,
}

/// `[general]`
#[derive(Debug, Clone)]
pub struct General {
    /// 界面语言（不写 = 跟随系统；写法见 `--help` 的 `--lang`）
    pub language: Option<String>,
    /// 默认模式：`auto` | `extract` | `pack`
    pub default_mode: String,
    /// 并行任务数（默认 1，对机械盘友好）
    pub jobs: usize,
    /// 关窗口时：`tray`（缩到托盘）| `exit`（直接退出）。**界面行为**，引擎自己不消费；
    /// 入库到这里只是让它有一份校验过的家（写非法值会被丢弃，见 load_from）。
    /// 默认值分平台：Windows `exit`、其余 `tray`（用户 2026-10-30 拍板，两边别抄反）。
    pub close_action: Option<String>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            language: None,
            default_mode: "auto".into(),
            jobs: 1,
            close_action: None,
        }
    }
}

/// 试密码候选数的**硬上限**
///
/// # 为什么必须有它（这是安全边界，不是性能考量）
/// 候选来源只有两个，都是「用户已知」或「公开」的信息：用户自己记的密码本、
/// 包作者公开写在旁边的说明文件。这个工具**不做字典、不做爆破**。
/// 但没有上限的配置项就能把它变成爆破器（把 `max_candidates` 配成 100000），
/// 所以这里夹死上限 —— 让「改配置改成破解工具」这件事从根上不可能发生。
pub const 候选数硬上限: usize = 64;

/// `[password]`
#[derive(Debug, Clone)]
pub struct Password {
    /// 密码本文件（相对配置目录；不写就是 `passwords.txt`）
    pub file: String,
    /// 从同目录的「说明.txt」里自动抓解压码
    pub auto_from_notes: bool,
    /// 最多试几个候选（试密码是有成本的，别让用户干等；硬上限见 [`候选数硬上限`]）
    pub max_candidates: usize,
}

// 说明：这里原本还有个 `try_common` 字段（"尝试内置的常见解压码"），**已删除**。
// 它从来没接线过（配置项在、但没有任何代码读它），而且真要实现它，就得内置一张
// 常见密码表 —— 那张表就是一个小字典，跨过了本项目的安全边界。
// 删掉比留一个骗人的开关好：用户以为有、实际没有，比明说"我们不干这个"更糟。
// （配套证据见 README「本工具不做的事」与 CHANGELOG 的安全护栏批次。）

impl Default for Password {
    fn default() -> Self {
        Self {
            file: "passwords.txt".into(),
            auto_from_notes: true,
            max_candidates: 12,
        }
    }
}

/// `[output]`
#[derive(Debug, Clone)]
pub struct Output {
    /// 默认输出目录（不写 = 每个包解到自己旁边）
    pub dir: Option<String>,
    /// 重名怎么办：`rename` | `skip` | `replace`
    pub conflict: String,
    /// 解压完原包怎么办：`keep`（默认）| `trash`（移入回收站，**永不 rm**）
    pub on_done: String,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            dir: None,
            conflict: "rename".into(),
            on_done: "keep".into(),
        }
    }
}

/// `[safety]`
#[derive(Debug, Clone)]
pub struct Safety {
    /// 解压后/前 的比值上限（防炸弹）
    pub zip_bomb_ratio: u64,
    /// 解出可执行文件时提醒
    pub warn_executables: bool,
}

impl Default for Safety {
    fn default() -> Self {
        Self {
            zip_bomb_ratio: 100,
            warn_executables: true,
        }
    }
}

impl Config {
    /// 读配置：文件不在、或整段写坏，都退回默认值（**不报错**）
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    /// 从指定路径读
    ///
    /// **逐字段取值**，而不是整份反序列化 —— 因为要的是「读的时候宽容」：
    /// 用户少个引号、把数字写成字符串、多写一段自己的备注，都只该影响**那一个字段**，
    /// 其余照常生效。一个笔误让整份配置作废，是最让人恼火的失败方式。
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // 整份 TOML 语法就坏（比如中文键名没加引号）→ 无从下手，全用默认
        let Ok(v) = toml::from_str::<toml::Value>(&text) else {
            return Self::default();
        };
        let mut cfg = Self::default();

        if let Some(t) = v.get("general") {
            if let Some(x) = t.get("language").and_then(|x| x.as_str())
                && !x.trim().is_empty()
            {
                cfg.general.language = Some(x.trim().to_string());
            }
            if let Some(x) = t.get("default_mode").and_then(|x| x.as_str()) {
                cfg.general.default_mode = x.to_string();
            }
            if let Some(x) = t.get("jobs").and_then(|x| x.as_integer()) {
                cfg.general.jobs = x.max(1) as usize;
            }
            if let Some(x) = t.get("close_action").and_then(|x| x.as_str()) {
                // 只认两个合法值；写错当没写（界面按平台默认走）
                match x.trim() {
                    "tray" | "exit" => cfg.general.close_action = Some(x.trim().to_string()),
                    _ => {}
                }
            }
        }
        if let Some(t) = v.get("password") {
            if let Some(x) = t.get("file").and_then(|x| x.as_str()) {
                cfg.password.file = x.to_string();
            }
            if let Some(x) = t.get("auto_from_notes").and_then(|x| x.as_bool()) {
                cfg.password.auto_from_notes = x;
            }
            // 注：老配置里可能还写着 `try_common = ...`，这里**故意不认** ——
            // 宽容解析的意义就是"多写一段只影响那一段"，不会因此报错。
            if let Some(x) = t.get("max_candidates").and_then(|x| x.as_integer()) {
                // 上限夹死（安全边界）：配置改不出"爆破模式"
                cfg.password.max_candidates = (x.max(1) as usize).min(候选数硬上限);
            }
        }
        if let Some(t) = v.get("output") {
            if let Some(x) = t.get("dir").and_then(|x| x.as_str())
                && !x.trim().is_empty()
            {
                cfg.output.dir = Some(x.to_string());
            }
            if let Some(x) = t.get("conflict").and_then(|x| x.as_str()) {
                cfg.output.conflict = x.to_string();
            }
            if let Some(x) = t.get("on_done").and_then(|x| x.as_str()) {
                // 只认两个值：别的写法一律按 keep（绝不做永久删除）
                cfg.output.on_done = if x == "trash" { "trash".into() } else { "keep".into() };
            }
        }
        if let Some(t) = v.get("safety") {
            if let Some(x) = t.get("zip_bomb_ratio").and_then(|x| x.as_integer()) {
                cfg.safety.zip_bomb_ratio = x.max(1) as u64;
            }
            if let Some(x) = t.get("warn_executables").and_then(|x| x.as_bool()) {
                cfg.safety.warn_executables = x;
            }
        }
        cfg
    }

    /// 把当前配置写回 `config_path()`（设置页保存用）
    ///
    /// **整份重写**：会丢掉用户手写的注释（设置页保存前应提示一次）。
    pub fn save(&self) -> crate::error::Result<()> {
        use crate::error::{Error, ErrorCode};

        fn esc(s: &str) -> String {
            s.replace('\\', "\\\\").replace('"', "\\\"")
        }

        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                Error::new(ErrorCode::PermissionDenied).context(format!("建配置目录失败: {e}"))
            })?;
        }
        let lang = self.general.language.clone().unwrap_or_default();
        let out_dir = self.output.dir.clone().unwrap_or_default();
        let text = format!(
            "# yshjy 配置（设置页 / --show-config 维护）\n\
             # 注意：在设置页点保存会整份重写，手写注释会丢\n\n\
             [general]\n\
             language = \"{}\"\n\
             default_mode = \"{}\"\n\
             jobs = {}\n\n\
             [password]\n\
             file = \"{}\"\n\
             auto_from_notes = {}\n\
             max_candidates = {}\n\n\
             [output]\n\
             dir = \"{}\"\n\
             conflict = \"{}\"\n\
             on_done = \"{}\"\n\n\
             [safety]\n\
             zip_bomb_ratio = {}\n\
             warn_executables = {}\n",
            esc(&lang),
            esc(&self.general.default_mode),
            self.general.jobs,
            esc(&self.password.file),
            self.password.auto_from_notes,
            self.password.max_candidates,
            esc(&out_dir),
            esc(&self.output.conflict),
            esc(&self.output.on_done),
            self.safety.zip_bomb_ratio,
            self.safety.warn_executables
        );
        std::fs::write(&path, text).map_err(|e| {
            Error::new(ErrorCode::PermissionDenied).context(format!("写配置失败: {e}"))
        })
    }

    /// 默认输出目录（展开 `~`）
    pub fn output_dir(&self) -> Option<PathBuf> {
        self.output.dir.as_deref().map(展开波浪号)
    }

    /// 密码本文件路径
    pub fn passwords_path(&self) -> PathBuf {
        // 绝对路径直接用；相对的相对配置目录
        let p = Path::new(&self.password.file);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            config_dir().join(p)
        }
    }

    /// 读密码本（文件不在 = 空列表）
    pub fn passwords(&self) -> Vec<String> {
        读密码本(&self.passwords_path())
    }
}

// ───────────────────────────── 密码本 ─────────────────────────────

/// 进程级配置（读一次就够）
///
/// 配置文件只有几百字节，但没必要**每个任务**都读一遍 —— 批量解 50 个包就是 50 次
/// 系统调用。CLI 一次进程用一个配置，正是 `OnceLock` 的用武之地。
pub fn 全局配置() -> &'static Config {
    static C: std::sync::OnceLock<Config> = std::sync::OnceLock::new();
    C.get_or_init(|| match 指定配置文件.get() {
        // `--config <路径>`：给了就用它，文件不在就退回默认路径
        Some(p) if p.exists() => Config::load_from(p),
        _ => Config::load(),
    })
}

/// 命令行 `--config` 指定的配置文件（必须在第一次 [`全局配置`] 之前设定）
static 指定配置文件: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// 指定配置文件（`--config`）
pub fn 指定配置文件路径(p: &std::path::Path) {
    let _ = 指定配置文件.set(展开波浪号(&p.to_string_lossy()));
}

/// [config] 命令行指定的配置文件是否真的存在（`--show-config` / 排错提示用）
pub fn 指定配置文件存在() -> bool {
    指定配置文件.get().map(|p| p.exists()).unwrap_or(false)
}

/// `--password-file` 给出的候选密码
///
/// 与配置文件里的密码本分开存：命令行是**这次**的意图，优先级更高，
/// 也不该影响配置文件里用户长期维护的那份。
static 额外密码: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// 设定命令行带来的候选密码（`--password-file`）
pub fn 设定额外密码(v: Vec<String>) {
    let _ = 额外密码.set(v);
}

/// 取命令行带来的候选密码
pub fn 额外密码候选() -> &'static [String] {
    额外密码.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// `--no-auto-password` 的开关
static 自动密码关闭: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 关闭「自动找密码」（说明文件 + 密码本）；用户显式 `-p` 不受影响
pub fn 关闭自动密码() {
    自动密码关闭.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 是否已关闭自动找密码
pub fn 自动密码已关闭() -> bool {
    自动密码关闭.load(std::sync::atomic::Ordering::Relaxed)
}

/// 读密码本
///
/// - 每行一个密码
/// - `#` 开头是注释，跳过
/// - 空行跳过
/// - **兼容隐写者的 `PW.txt`**：那种文件是 `密码<Tab>说明`，所以按 Tab 取第一列
/// - **绝不做 ASCII 过滤** —— 隐写者的 bug 就是过滤掉了中文密码
pub fn 读密码本(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 兼容 `密码<Tab>备注` 的格式
        let pw = line.split('\t').next().unwrap_or(line).trim();
        if pw.is_empty() {
            continue;
        }
        if !out.iter().any(|x: &String| x == pw) {
            out.push(pw.to_string());
        }
    }
    out
}

/// 把外部密码本导入进来（追加、去重）
///
/// 返回新增了几个。源文件的格式规则同 [`读密码本`]。
pub fn 导入密码本(src: &Path, dst: &Path) -> crate::error::Result<usize> {
    let 新 = 读密码本(src);
    if 新.is_empty() {
        return Err(crate::error::Error::new(crate::error::ErrorCode::InvalidArgument)
            .context(format!("{} 里没读到任何密码", src.display())));
    }
    let mut 现有 = 读密码本(dst);
    let mut 加了 = 0;
    for pw in 新 {
        if !现有.iter().any(|x| x == &pw) {
            现有.push(pw);
            加了 += 1;
        }
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::error::Error::new(crate::error::ErrorCode::PermissionDenied)
                .context(format!("建配置目录失败 {}: {e}", parent.display()))
        })?;
    }
    let mut 文本 = String::new();
    if !dst.exists() {
        文本.push_str("# yshjy 密码本 —— 每行一个密码，# 开头是注释\n");
        文本.push_str("# 支持中文密码（不会被过滤）\n");
    }
    for pw in &现有 {
        文本.push_str(pw);
        文本.push('\n');
    }
    std::fs::write(dst, 文本).map_err(|e| {
        crate::error::Error::new(crate::error::ErrorCode::PermissionDenied)
            .context(format!("写密码本失败 {}: {e}", dst.display()))
    })?;
    Ok(加了)
}

/// 配置摘要：**只给键名和值，不给句子**（句子由外壳查语言包渲染）
///
/// 键名对应语言包里的 `config.<键>`。
pub fn 配置摘要(cfg: &Config) -> Vec<(&'static str, String)> {
    vec![
        ("path", config_path().display().to_string()),
        (
            "language",
            cfg.general
                .language
                .clone()
                .unwrap_or_default(), // 空 = 外壳渲染成"（跟随系统）"
        ),
        ("jobs", cfg.general.jobs.to_string()),
        (
            "out_dir",
            cfg.output_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ),
        ("conflict", cfg.output.conflict.clone()),
        ("ratio", cfg.safety.zip_bomb_ratio.to_string()),
        (
            "passwords",
            format!("{}|{}", cfg.passwords_path().display(), cfg.passwords().len()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 临时目录(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_cfg_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn 没有配置文件时全用默认值() {
        let cfg = Config::load_from(Path::new("/绝对不存在的路径/config.toml"));
        assert_eq!(cfg.general.jobs, 1);
        assert_eq!(cfg.safety.zip_bomb_ratio, 100);
        assert!(cfg.general.language.is_none());
        assert!(cfg.output_dir().is_none());
    }

    // 关窗口动作（W9）：合法值收编、非法值丢弃（界面按平台默认走）
    #[test]
    fn close_action_只认合法值() {
        let dir = 临时目录("closeaction");
        let f = dir.join("config.toml");
        // TOML 单引号字面串，免引号转义
        std::fs::write(&f, "[general]\nclose_action = 'tray'\n").unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.general.close_action.as_deref(), Some("tray"));

        std::fs::write(&f, "[general]\nclose_action = '最小化'\n").unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(
            cfg.general.close_action, None,
            "写错值当没写（非法值不收）"
        );
    }

    #[test]
    fn 能读出用户写的配置() {
        let dir = 临时目录("read");
        let f = dir.join("config.toml");
        std::fs::write(
            &f,
            r#"
[general]
language = "en-US"
jobs = 3

[output]
dir = "/tmp/出"
conflict = "skip"

[safety]
zip_bomb_ratio = 50
"#,
        )
        .unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.general.language.as_deref(), Some("en-US"));
        assert_eq!(cfg.general.jobs, 3);
        assert_eq!(cfg.output.conflict, "skip");
        assert_eq!(cfg.safety.zip_bomb_ratio, 50);
        // 没写的段仍然是默认值
        assert_eq!(cfg.password.max_candidates, 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 写坏的配置退回默认值而不是崩掉() {
        let dir = 临时目录("bad");
        let f = dir.join("config.toml");
        std::fs::write(&f, "这不是合法的 toml [[[").unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.general.jobs, 1, "写坏了就该退回默认，不该报错给用户");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 未知字段被忽略() {
        let dir = 临时目录("unknown");
        let f = dir.join("config.toml");
        // 注意中文键名**必须加引号** —— TOML 的裸键只允许 ASCII 字母数字下划线连字符
        std::fs::write(
            &f,
            "[general]\njobs = 2\n\"我自己的备注\" = \"随便写\"\n\n[\"未来的段\"]\nx = 1\n",
        )
        .unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.general.jobs, 2, "多写了一句备注、多了一个自造的段，都不该让配置失效");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 配置改不出爆破模式() {
        // ★ 安全边界：max_candidates 只能夹在 [1, 硬上限] 之间。
        //   不然把配置写成 100000，这工具就变成爆破器了。
        let dir = 临时目录("bruteforce");
        let f = dir.join("config.toml");
        std::fs::write(&f, "[password]\nmax_candidates = 100000\n").unwrap();
        assert_eq!(
            Config::load_from(&f).password.max_candidates,
            候选数硬上限,
            "配到天上也得夹回硬上限"
        );

        // 下限也要夹住（写 0 = 一个候选都不试，那等于把功能写没了，不是合法配置）
        std::fs::write(&f, "[password]\nmax_candidates = 0\n").unwrap();
        assert_eq!(Config::load_from(&f).password.max_candidates, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 老配置里的try_common不会让配置失效() {
        // 这个字段已删（它本来就是个没接线的空壳），但老用户的配置文件里可能还写着它。
        // 宽容解析要保证：写着也不报错，且同段其它字段照常生效。
        let dir = 临时目录("oldtrycommon");
        let f = dir.join("config.toml");
        std::fs::write(
            &f,
            "[password]\ntry_common = true\nmax_candidates = 5\nauto_from_notes = false\n",
        )
        .unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.password.max_candidates, 5);
        assert!(!cfg.password.auto_from_notes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 读密码本绝不改动文件本身() {
        // ★ 保险丝：将来谁加了"自动把抓到的密码 / 试成功的密码写进密码本"的逻辑，
        //   这条测试会红。密码本只允许**用户主动**写（自己编辑，或 `--import-passwords`）。
        let dir = 临时目录("pwlock");
        let f = dir.join("passwords.txt");
        std::fs::write(&f, "# 我自己记的\nabc123\n中文密码7\n").unwrap();
        let 读之前 = std::fs::read(&f).unwrap();

        let got = 读密码本(&f);
        assert_eq!(got, vec!["abc123".to_string(), "中文密码7".to_string()]);

        let 读之后 = std::fs::read(&f).unwrap();
        assert_eq!(读之前, 读之后, "读密码本不许写回文件（只试不存）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 某一段写错不影响其它段() {
        let dir = 临时目录("onerow");
        let f = dir.join("config.toml");
        // jobs 本该是数字，给成字符串 → 只有 [general] 退回默认
        std::fs::write(
            &f,
            "[general]\njobs = \"本该是数字\"\n\n[safety]\nzip_bomb_ratio = 42\n",
        )
        .unwrap();
        let cfg = Config::load_from(&f);
        assert_eq!(cfg.general.jobs, 1, "写坏的那段退回默认");
        assert_eq!(
            cfg.safety.zip_bomb_ratio, 42,
            "没写坏的段必须照常生效 —— 一个笔误不该让整份配置作废"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 密码本能读注释和中文() {
        let dir = 临时目录("pw");
        let f = dir.join("passwords.txt");
        std::fs::write(
            &f,
            "# 这是注释\ndemo-password\nwangyihan2024\n\n# 又是注释\n动漫之家\n",
        )
        .unwrap();
        let pws = 读密码本(&f);
        assert_eq!(pws, vec!["demo-password", "wangyihan2024", "动漫之家"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 密码本兼容隐写者的_tab_格式且不过滤中文() {
        // 隐写者的 PW.txt 是「密码<Tab>说明」，而且它自己会 ascii 过滤吞掉中文
        let dir = 临时目录("pw_tab");
        let f = dir.join("PW.txt");
        std::fs::write(&f, "password123\t备注一\ndemo-password\t中文密码\t多一列\n").unwrap();
        let pws = 读密码本(&f);
        assert_eq!(pws, vec!["password123", "demo-password"], "Tab 取第一列 + 中文保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 导入密码本会去重并保留原有() {
        let dir = 临时目录("import");
        let src = dir.join("外部.txt");
        let dst = dir.join("passwords.txt");
        std::fs::write(&src, "新的甲\n新的乙\n重复的\n").unwrap();
        std::fs::write(&dst, "# 已有\n重复的\n老的\n").unwrap();

        let 加 = 导入密码本(&src, &dst).unwrap();
        assert_eq!(加, 2, "只有两个是新的");
        let 全部 = 读密码本(&dst);
        assert!(全部.contains(&"老的".to_string()), "原有的不能丢");
        assert!(全部.contains(&"新的甲".to_string()));
        assert_eq!(全部.iter().filter(|p| *p == "重复的").count(), 1, "不该有重复");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 导入空文件会报错而不是默默成功() {
        let dir = 临时目录("import_empty");
        let src = dir.join("空.txt");
        std::fs::write(&src, "# 只有注释\n\n").unwrap();
        let 结果 = 导入密码本(&src, &dir.join("passwords.txt"));
        assert!(结果.is_err(), "一个密码都没读到就该明确报错");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 波浪号会展开() {
        // 只在有主目录的环境下检查（Windows 看 USERPROFILE，Unix 看 HOME —— 统一走 paths::home，
        // 比较用 PathBuf：Windows 的分隔符是 \，拿字符串比永远不等）
        if let Some(h) = crate::paths::home() {
            assert_eq!(
                展开波浪号("~/下载/出"),
                h.join("下载/出"),
                "主目录存在时 ~ 必须展开"
            );
        }
        // 不带波浪号的路径原样
        assert_eq!(展开波浪号("/tmp/x").display().to_string(), "/tmp/x");
        assert_eq!(展开波浪号("相对路径").display().to_string(), "相对路径");
    }
}
