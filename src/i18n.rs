// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 多语言（i18n）—— 界面文案与翻译
//!
//! # 为什么不引 fluent / gettext，自己写
//!
//! 1. 这个工具要**单文件跨平台分发**，多一个重型依赖就多一份体积和供应链风险；
//! 2. 语言包必须是**普通人能改的文本文件** —— 社区想加自己的母语，翻译一个 toml 就行，
//!    不用会 Rust、不用重新编译（见下「外部语言包」）；
//! 3. 真实需求很窄：查表 + `{占位符}` 插值 + 回退。几百行足够，还好审计。
//!
//! # 分层（这是架构约定，别破坏）
//!
//! ```text
//! 引擎（lib）   →  只产出「错误码 + 结构化参数」，**永远不产出面向用户的句子**
//! 外壳（CLI/GUI）→  拿错误码来这里查文案、插值、显示
//! ```
//!
//! 所以引擎里不该 `println!("解压失败")`，而该 `Err(ErrorCode::PasswordWrong)`，
//! 由外壳 `i18n.t("error.password_wrong", &[])` 渲染成任何语言。
//!
//! # 语言从哪来（优先级从高到低）
//!
//! 1. 命令行 `--lang xx`
//! 2. 环境变量 `YSHTY_LANG`（给脚本和测试用）
//! 3. 系统语言：`LC_ALL` → `LC_MESSAGES` → `LANG`
//! 4. 兜底：[`FALLBACK`]（简体中文）
//!
//! # 语言包
//!
//! - **内置**：`locales/*.toml`，由 `build.rs` 扫描后嵌进二进制（加语言只需加文件）
//! - **外部**：`$YSHTY_LOCALES/` → `~/.config/yshjy/locales/` → 当前目录 `locales/`
//!   外部包**只需写要覆盖的条目**，其余自动回退内置 —— 改一句话不用抄整个文件

use std::collections::HashMap;
use std::path::PathBuf;

/// 编译期嵌入的语言包（由 `build.rs` 扫描 `locales/*.toml` 生成）
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/locales.rs"));
}

pub use embedded::BUILTIN;

/// 兜底语言：没翻译、认不出系统语言时用它
pub const FALLBACK: &str = "zh-CN";

// ───────────────────────────── 语言代码 ─────────────────────────────

/// 语言代码（如 `zh-CN` / `en-US` / `ja`）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Lang(String);

impl Lang {
    /// 规范化各种写法：`zh_cn` / `ZH-cn` / `zh_CN.UTF-8` / `zh-Hans-CN` → `zh-CN`
    pub fn parse(raw: &str) -> Self {
        let s = raw.trim();
        // POSIX 的 LANG 常带编码后缀（`.UTF-8`）和修饰（`@euro`）
        let s = s.split(['.', '@']).next().unwrap_or(s);
        let s = s.replace('_', "-");
        let mut parts = s.split('-');
        let primary = parts.next().unwrap_or("").to_ascii_lowercase();
        let second = parts.next().unwrap_or("");
        if primary.is_empty() {
            return Self(FALLBACK.to_string());
        }
        // 区域码统一成大写；`zh-Hans` 这种脚本子标签保持原样（只首字母大写）
        let second = if second.len() == 2 || second.len() == 3 && second.chars().all(|c| c.is_ascii_digit()) {
            second.to_ascii_uppercase()
        } else if second.is_empty() {
            String::new()
        } else {
            let mut cs = second.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &cs.as_str().to_lowercase(),
                None => String::new(),
            }
        };
        if second.is_empty() {
            Self(primary)
        } else {
            Self(format!("{primary}-{second}"))
        }
    }

    /// 语言代码本身
    pub fn code(&self) -> &str {
        &self.0
    }

    /// 主语言（`zh-CN` → `zh`）
    pub fn primary(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ───────────────────────────── 语言包 ─────────────────────────────

/// 一个语言包：TOML 展平成 `键 → 文案` 的查表
#[derive(Debug, Clone)]
pub struct Catalog {
    lang: Lang,
    name: String,
    table: HashMap<String, String>,
}

impl Catalog {
    /// 从 TOML 文本解析一个语言包
    ///
    /// 只有字符串会被收进表里；数字/布尔之类的键直接忽略（免得污染文案键集）。
    pub fn from_toml(code: &str, text: &str) -> crate::error::Result<Self> {
        // 注意用 `toml::from_str` 解析**整个文档**；
        // `text.parse::<toml::Value>()` 只吃单个值表达式，喂整个 toml 会报
        // "unexpected content, expected nothing"（踩过）
        let value: toml::Value = toml::from_str(text).map_err(|e| {
            crate::error::Error::new(crate::error::ErrorCode::Internal)
                .context(format!("语言包 {code} 解析失败: {e}"))
        })?;
        let mut table = HashMap::new();
        flatten(&value, String::new(), &mut table);
        let name = table
            .get("meta.name")
            .cloned()
            .unwrap_or_else(|| code.to_string());
        Ok(Self {
            lang: Lang::parse(code),
            name,
            table,
        })
    }

    /// 语言代码
    pub fn lang(&self) -> &Lang {
        &self.lang
    }

    /// 这门语言自称的名字（如 `简体中文` / `English`）
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 这个包里有哪些键（测试用来比对「有没有漏翻」）
    pub fn keys(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.table.keys().map(|s| s.as_str()).collect();
        v.sort_unstable();
        v
    }

    /// 查一条原始文案（不插值）
    pub fn get(&self, key: &str) -> Option<&str> {
        self.table.get(key).map(|s| s.as_str())
    }

    /// 查一条文案并插值：`t("result.ok", &[("input", "a.mp4")])`
    pub fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        interpolate(self.get(key).unwrap_or(key), args)
    }
}

/// 把嵌套的 TOML 表展平成 `a.b.c` 形式的键
fn flatten(v: &toml::Value, prefix: String, out: &mut HashMap<String, String>) {
    match v {
        toml::Value::Table(t) => {
            for (k, val) in t {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(val, key, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix, s.trim_end().to_string());
        }
        // 数字 / 布尔 / 数组 / 时间 → 不是文案，忽略
        _ => {}
    }
}

// ───────────────────────────── 文案仓库 ─────────────────────────────

/// 语言代码的**默认包**：用户给的代码没直接对应某个语言包时，按这张表落位
///
/// 为什么写成常量表、而不是"遍历内置包找第一个主语言相同的"：
/// 后者遍历 `HashMap`，**迭代顺序是随机的** —— 同一条命令今天给简体、明天给繁体。
/// 这种不确定性在别的语言上更隐蔽（`en` 少一个包就察觉不到），所以必须写死。
const LANG_DEFAULTS: &[(&str, &str)] = &[
    // 完整的脚本标签要先判（`zh-Hant` 得给繁体，不能只看主语言 `zh` 就给简体）
    ("zh-Hant", "zh-TW"),
    ("zh-HK", "zh-TW"),
    ("zh-MO", "zh-TW"),
    ("zh-Hans", "zh-CN"),
    ("zh-SG", "zh-CN"),
    // 主语言级
    ("zh", "zh-CN"),
    ("en", "en-US"),
    ("ja", "ja-JP"),
    ("ko", "ko-KR"),
    ("ru", "ru-RU"),
    ("es", "es-ES"),
];

/// 文案仓库：管内置语言、外部覆盖、回退链
///
/// 外壳（CLI / GUI）在启动时建一个，之后所有面向用户的文字都从它来。
#[derive(Debug, Clone)]
pub struct I18n {
    /// 当前生效的语言包（可能是外部覆盖版，也可能是内置版）
    active: Option<Catalog>,
    /// 全部内置语言包
    builtin: HashMap<String, Catalog>,
    /// 兜底包（一定存在）
    fallback: Catalog,
    /// 用户要的语言（没找到时用来提示）
    requested: String,
}

impl I18n {
    /// 建仓库。`want` 为 `None` 时自动猜系统语言。
    pub fn new(want: Option<&str>) -> Self {
        let mut builtin = HashMap::new();
        for (code, text) in BUILTIN {
            match Catalog::from_toml(code, text) {
                Ok(c) => {
                    builtin.insert(c.lang().code().to_string(), c);
                }
                // 内置包坏掉属于编译期问题，不该发生；真发生了也不能让程序崩
                Err(e) => eprintln!("[yshjy] 内置语言包 {code} 有问题：{e}"),
            }
        }
        let fallback = builtin
            .get(FALLBACK)
            .cloned()
            .or_else(|| builtin.values().next().cloned())
            .expect("至少要有一种内置语言包（locales/*.toml 不能全空）");

        let requested = want
            .map(|s| Lang::parse(s).code().to_string())
            .unwrap_or_else(detect);

        // ① 外部语言包（用户/社区自己放的，优先）
        let mut active = load_external(&requested);
        // ② 内置精确匹配
        if active.is_none() {
            active = builtin.get(&requested).cloned();
        }
        // ③ 按默认表落位：先匹配完整代码（`zh-Hant`），再退回主语言（`zh`）
        if active.is_none() {
            let want = Lang::parse(&requested);
            let code = LANG_DEFAULTS
                .iter()
                .find(|(k, _)| *k == want.code())
                .or_else(|| LANG_DEFAULTS.iter().find(|(k, _)| *k == want.primary()))
                .map(|(_, v)| *v);
            let primary = want.primary().to_string();
            active = code
                .and_then(|c| builtin.get(c).cloned())
                .or_else(|| {
                    // 表里没记的（社区自己加的语言）：按字母序取第一个 ——
                    // 顺序可能不是最理想，但至少**确定性**，不会来回跳
                    let mut cands: Vec<&Catalog> = builtin
                        .values()
                        .filter(|c| c.lang().primary() == primary)
                        .collect();
                    cands.sort_by(|a, b| a.lang().code().cmp(b.lang().code()));
                    cands.first().map(|c| (*c).clone())
                });
        }

        Self {
            active,
            builtin,
            fallback,
            requested,
        }
    }

    /// 查一条文案并插值（找不到键时返回键名本身 —— 好让漏翻一眼看见）
    pub fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        let raw = self
            .active
            .as_ref()
            .and_then(|c| c.get(key))
            .or_else(|| self.fallback.get(key))
            .unwrap_or(key);
        interpolate(raw, args)
    }

    /// 当前生效的语言代码
    pub fn lang(&self) -> String {
        self.active
            .as_ref()
            .map(|c| c.lang().code().to_string())
            .unwrap_or_else(|| FALLBACK.to_string())
    }

    /// 内置支持哪些语言（给 `--help` 和错误提示用）
    pub fn available(&self) -> Vec<String> {
        let mut v: Vec<String> = self.builtin.keys().cloned().collect();
        v.sort();
        v
    }

    /// 用户要的语言有没有被满足（没有的话外壳可以提示一句）
    pub fn is_requested_ok(&self) -> bool {
        self.lang() == self.requested || Lang::parse(&self.requested).primary() == Lang::parse(&self.lang()).primary()
    }

    /// 用户原本要的语言代码
    pub fn requested(&self) -> &str {
        &self.requested
    }
}

/// 找外部语言包：`$YSHTY_LOCALES/` → `~/.config/yshjy/locales/` → 当前目录 `locales/`
fn load_external(code: &str) -> Option<Catalog> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var("YSHTY_LOCALES")
        .ok()
        .filter(|s| !s.is_empty())
    {
        dirs.push(PathBuf::from(d));
    }
    // 走 paths::home()：Windows 没有 HOME（只有 USERPROFILE），
    // 以前只认 HOME 的话用户放的语言包在 Windows 上永远找不到（Linux 行为不变）。
    if let Some(home) = crate::paths::home() {
        dirs.push(home.join(".config/yshjy/locales"));
    }
    // 便携 / 测试用：程序旁边的 locales/（也就是源码里的那份）
    dirs.push(PathBuf::from("locales"));

    for dir in dirs {
        let p = dir.join(format!("{code}.toml"));
        if !p.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        match Catalog::from_toml(code, &text) {
            Ok(c) => return Some(c),
            Err(e) => eprintln!("[yshjy] 外部语言包 {} 有问题：{e}", p.display()),
        }
    }
    None
}

// ───────────────────────────── 系统语言 ─────────────────────────────

/// 猜系统语言（`YSHTY_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG` → 兜底）
///
/// 注：Windows 上没有 `LANG`，那边以后接系统 API（现在平台重心在 Linux）。
pub fn detect() -> String {
    for k in ["YSHTY_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim();
            // `C` / `POSIX` 是「不本地化」的意思，不是语言
            if !v.is_empty() && v != "C" && v != "POSIX" {
                return Lang::parse(v).code().to_string();
            }
        }
    }
    FALLBACK.to_string()
}

// ───────────────────────────── 插值 ─────────────────────────────

/// 把 `{name}` 换成值；支持 `{name:>4}` / `{name:<6}` / `{name:^5}` 这类对齐；
/// `{{` / `}}` 是转义的字面花括号（语言包里要写 `{a,b}` 这种 shell 语法时用得上）；
/// **没给值的占位符原样保留**（方便发现漏传参数）。
pub fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    loop {
        let open = rest.find('{');
        let escaped_close = rest.find("}}");
        // 谁在前面先处理谁（`}}` 也要能被转义，否则 `{{x}}` 会多出一个 `}`）
        let close_first = match (open, escaped_close) {
            (Some(o), Some(c)) => c < o,
            (None, Some(_)) => true,
            _ => false,
        };
        if close_first {
            let i = escaped_close.expect("前面已判过");
            out.push_str(&rest[..i]);
            out.push('}');
            rest = &rest[i + 2..];
            continue;
        }

        let Some(i) = open else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        // `{{` → 字面 `{`
        if let Some(r) = after.strip_prefix('{') {
            out.push('{');
            rest = r;
            continue;
        }
        let Some(close) = after.find('}') else {
            // 没有配对的 `}`：原样吐出去
            out.push_str(&rest[i..]);
            return out;
        };
        let spec = &after[..close];
        let (key, fmt) = match spec.split_once(':') {
            Some((k, f)) => (k.trim(), Some(f)),
            None => (spec.trim(), None),
        };
        match args.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => out.push_str(&apply_fmt(v, fmt)),
            None => {
                out.push('{');
                out.push_str(spec);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
}

/// 支持 `:>4` / `:<6` / `:^5`（按**字符数**算宽度；CJK 全角会有偏差，数字和 ASCII 没问题）
fn apply_fmt(v: &str, spec: Option<&str>) -> String {
    let Some(spec) = spec else {
        return v.to_string();
    };
    let (align, width) = match spec.chars().next() {
        Some('>') => ('>', spec[1..].trim()),
        Some('<') => ('<', spec[1..].trim()),
        Some('^') => ('^', spec[1..].trim()),
        _ => ('>', spec.trim()),
    };
    let Ok(w) = width.parse::<usize>() else {
        return v.to_string();
    };
    let len = v.chars().count();
    if len >= w {
        return v.to_string();
    }
    let pad = w - len;
    match align {
        '<' => format!("{v}{}", " ".repeat(pad)),
        '^' => {
            let left = pad / 2;
            format!("{}{v}{}", " ".repeat(left), " ".repeat(pad - left))
        }
        _ => format!("{}{v}", " ".repeat(pad)),
    }
}

// ───────────────────────────── 测试 ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 语言代码规范化() {
        assert_eq!(Lang::parse("zh_CN.UTF-8").code(), "zh-CN");
        assert_eq!(Lang::parse("ZH-cn").code(), "zh-CN");
        assert_eq!(Lang::parse("en_US").code(), "en-US");
        assert_eq!(Lang::parse("ja").code(), "ja");
        assert_eq!(Lang::parse("zh-Hans-CN").code(), "zh-Hans");
        assert_eq!(Lang::parse("").code(), FALLBACK);
        assert_eq!(Lang::parse("zh-CN").primary(), "zh");
    }

    #[test]
    fn 内置语言包都能解析() {
        assert!(!BUILTIN.is_empty(), "至少要有一个语言包");
        for (code, text) in BUILTIN {
            let c = Catalog::from_toml(code, text).unwrap_or_else(|e| panic!("{code} 解析失败: {e}"));
            assert!(!c.name().is_empty(), "{code} 缺 meta.name");
            assert!(c.get("help.text").is_some(), "{code} 缺 help.text");
        }
    }

    #[test]
    fn 各语言包键集一致防漏翻() {
        // 这条测试是**翻译质量的看门人**：加了键却漏翻某种语言，这里会立刻红
        let mut 基准: Option<(String, Vec<String>)> = None;
        for (code, text) in BUILTIN {
            let c = Catalog::from_toml(code, text).unwrap();
            let keys: Vec<String> = c.keys().iter().map(|s| s.to_string()).collect();
            match &基准 {
                None => 基准 = Some((code.to_string(), keys)),
                Some((基准语言, 基准键)) => {
                    let 缺: Vec<&String> = 基准键.iter().filter(|k| !keys.contains(k)).collect();
                    let 多: Vec<&String> = keys.iter().filter(|k| !基准键.contains(k)).collect();
                    assert!(
                        缺.is_empty() && 多.is_empty(),
                        "{code} 与 {基准语言} 的键集不一致：\n  缺 {:?}\n  多 {:?}",
                        缺,
                        多
                    );
                }
            }
        }
    }

    #[test]
    fn 插值支持占位符与对齐() {
        assert_eq!(
            interpolate("解开 {input}（{size}）", &[("input", "a.mp4"), ("size", "1 MB")]),
            "解开 a.mp4（1 MB）"
        );
        // 对齐：右对齐补空格
        assert_eq!(interpolate("[{n:>4}]", &[("n", "7")]), "[   7]");
        assert_eq!(interpolate("[{n:<4}]", &[("n", "7")]), "[7   ]");
        assert_eq!(interpolate("[{n:^4}]", &[("n", "7")]), "[ 7  ]");
        // 超宽不截断
        assert_eq!(interpolate("[{n:>2}]", &[("n", "123")]), "[123]");
        // `{{` 转义
        assert_eq!(interpolate("{{字面}}", &[]), "{字面}");
        // 没给值的占位符原样保留（方便发现漏传参数）
        assert_eq!(interpolate("{a} 和 {b}", &[("a", "1")]), "1 和 {b}");
        // 没有配对 `}` 时不吞掉后面的内容
        assert_eq!(interpolate("坏模板 {a", &[]), "坏模板 {a");
    }

    #[test]
    fn 找不到语言时回退且不崩() {
        let i = I18n::new(Some("xx-XX"));
        // 回退到兜底语言
        assert_eq!(i.lang(), FALLBACK);
        assert!(!i.is_requested_ok(), "要的语言没满足，外壳应当提示一句");
        // 键名原样返回（一眼看出漏翻）
        assert_eq!(i.t("不存在的键", &[]), "不存在的键");
    }

    #[test]
    fn 主语言能匹配到内置包() {
        // 这些「没指定地区」或「脚本标签」的写法都要落到**确定性**的包上：
        // 曾经因为遍历 HashMap 而随机落到简体或繁体（真 bug）
        assert_eq!(I18n::new(Some("zh-Hans")).lang(), "zh-CN");
        assert_eq!(I18n::new(Some("zh")).lang(), "zh-CN");
        assert_eq!(I18n::new(Some("zh-Hans-CN")).lang(), "zh-CN");
        assert_eq!(I18n::new(Some("zh-Hant")).lang(), "zh-TW", "繁体标签该给繁体");
        assert_eq!(I18n::new(Some("zh-HK")).lang(), "zh-TW", "香港用繁体");
        assert_eq!(I18n::new(Some("zh-TW")).lang(), "zh-TW", "精确匹配优先");
        assert_eq!(I18n::new(Some("en-GB")).lang(), "en-US");
        assert_eq!(I18n::new(Some("en-US")).lang(), "en-US");
        assert_eq!(I18n::new(Some("ja")).lang(), "ja-JP");
        assert_eq!(I18n::new(Some("ko")).lang(), "ko-KR");
        assert_eq!(I18n::new(Some("ru")).lang(), "ru-RU");
        assert_eq!(I18n::new(Some("es-MX")).lang(), "es-ES");
        assert!(I18n::new(Some("en")).is_requested_ok());

        // 确定性：同一条命令跑两次必须一样（这条防的就是上面那个随机 bug）
        for _ in 0..20 {
            assert_eq!(I18n::new(Some("zh")).lang(), "zh-CN");
        }
    }

    #[test]
    fn 环境变量能指定语言() {
        // 只在测试进程内改环境变量（Rust 2024 里 set_var 是 unsafe 的，
        // 所以这里只验证「读得到」，不真去写）
        let got = detect();
        assert!(!got.is_empty(), "系统语言探测不该返回空");
        assert!(Lang::parse(&got).code() == got, "探测结果应当是规范化的代码");
    }

    #[test]
    fn 两种语言的同一句话确实不一样() {
        let zh = I18n::new(Some("zh-CN"));
        let en = I18n::new(Some("en-US"));
        assert_ne!(
            zh.t("help.text", &[]),
            en.t("help.text", &[]),
            "中英文的帮助文本不该一样（翻译真的生效了）"
        );
        assert!(en.t("result.ok", &[("input", "a"), ("size", "1 B"), ("secs", "0")])
            .contains("extracted"));
    }
}
