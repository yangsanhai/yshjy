//! 挑选：预览之后只解用户勾的那部分
//!
//! # 为什么需要它（见 `docs/V0.5-ENGINE.md` §5）
//!
//! 界面 S3 是「列出内容 → 勾选 → 开始解压」。伪装包动辄 16 GB，
//! 用户可能只想要里面的主程序，不想等整个包解完 —— 所以引擎必须支持**选择性解压**。
//!
//! 条目序号（`index`）从预览到解压**全程不变**，这就是 GUI 勾选的锚点。
//!
//! 通配表达式自己实现（不引 `glob` crate）：支持 `*`（不跨 `/`）、`**`（跨）、`?`，
//! 大小写敏感（Linux 语义）。

use crate::error::{Error, ErrorCode, Result};

/// 挑选方式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectMode {
    /// 全都要（默认）
    All,
    /// 只要指定序号的条目
    Only(Vec<u64>),
    /// 只要路径匹配通配表达式的
    Matching(String),
    /// 只要「必需的」—— 跳过目录与平台垃圾文件（`.DS_Store` / `__MACOSX/` 等）
    Required,
}

/// 挑选条件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    mode: SelectMode,
    skip_dirs: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self::all()
    }
}

impl Selection {
    /// 全都要
    pub fn all() -> Self {
        Self {
            mode: SelectMode::All,
            skip_dirs: false,
        }
    }

    /// 只要这些序号
    pub fn only(indices: Vec<u64>) -> Self {
        Self {
            mode: SelectMode::Only(indices),
            skip_dirs: false,
        }
    }

    /// 只要路径匹配这个通配表达式的
    pub fn matching(pattern: impl Into<String>) -> Self {
        Self {
            mode: SelectMode::Matching(pattern.into()),
            skip_dirs: false,
        }
    }

    /// 只要「必需的」（跳过目录与已知平台垃圾）
    pub fn required() -> Self {
        Self {
            mode: SelectMode::Required,
            skip_dirs: true,
        }
    }

    /// 是否跳过目录条目
    pub fn with_skip_dirs(mut self, skip: bool) -> Self {
        self.skip_dirs = skip;
        self
    }

    /// 挑选方式
    pub fn mode(&self) -> &SelectMode {
        &self.mode
    }

    /// 是不是「全都要」（外壳可据此跳过逐条判断，省时间）
    pub fn is_all(&self) -> bool {
        self.mode == SelectMode::All && !self.skip_dirs
    }

    /// 某个条目是否被选中
    ///
    /// `index` 就是预览时给出的序号；`path` 是归档内路径；`is_dir` 表示目录条目。
    ///
    /// 通配规则（对用户直觉友好）：
    /// - 模式**含 `/`** → 按整个路径匹配（如 `游戏数据/**`）
    /// - 模式**不含 `/`** → 匹配**任意层级**的文件名（如 `*.exe` 等于「所有 exe」）
    pub fn matches(&self, index: u64, path: &str, is_dir: bool) -> bool {
        if self.skip_dirs && is_dir {
            return false;
        }
        match &self.mode {
            SelectMode::All => true,
            SelectMode::Only(list) => list.contains(&index),
            SelectMode::Matching(pat) => {
                if pat.contains('/') {
                    glob_match(pat, path)
                } else {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    glob_match(pat, name)
                }
            }
            SelectMode::Required => !is_dir && !is_junk(path),
        }
    }

    /// 中性描述（日志用，不是用户文案）
    pub fn describe(&self) -> String {
        match &self.mode {
            SelectMode::All => "all".to_string(),
            SelectMode::Only(list) => {
                let head: Vec<String> = list.iter().take(8).map(|i| i.to_string()).collect();
                format!(
                    "only[{}]{}{}",
                    head.join(","),
                    if list.len() > 8 { "..." } else { "" },
                    if self.skip_dirs { " +skip_dirs" } else { "" }
                )
            }
            SelectMode::Matching(p) => format!("matching[{p}]"),
            SelectMode::Required => "required".to_string(),
        }
    }

    /// 从命令行表达式解析（`--select`）
    ///
    /// | 写法 | 含义 |
    /// |------|------|
    /// | `all` / 空 | 全都要 |
    /// | `1,3,5` | 只要第 1、3、5 个 |
    /// | `*.exe` / `游戏数据/**` | 按通配匹配 |
    /// | `required` | 只要必需的 |
    pub fn parse(spec: &str) -> Result<Self> {
        let s = spec.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("all") {
            return Ok(Self::all());
        }
        if s.eq_ignore_ascii_case("required") {
            return Ok(Self::required());
        }
        if s.contains('*') || s.contains('?') {
            return Ok(Self::matching(s));
        }
        // 形如 "1,3,5" 的序号列表
        if s.split(',').all(|p| p.trim().parse::<u64>().is_ok()) {
            let mut ids: Vec<u64> = s
                .split(',')
                .map(|p| p.trim().parse::<u64>().expect("已校验"))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            return Ok(Self::only(ids));
        }
        Err(Error::new(ErrorCode::InvalidArgument).with("select", s))
    }
}

/// 通配匹配（`*` 不跨 `/`，`**` 跨目录，`?` 匹配单个非 `/` 字符）
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = path.chars().collect();
    glob_inner(&p, &s)
}

fn glob_inner(p: &[char], s: &[char]) -> bool {
    let Some(&first) = p.first() else {
        return s.is_empty();
    };
    match first {
        '*' => {
            if p.get(1) == Some(&'*') {
                // ** —— 跨目录匹配任意内容（紧随的 '/' 也吃掉，便于写 `目录/**`）
                let rest = if p.get(2) == Some(&'/') { &p[3..] } else { &p[2..] };
                for i in 0..=s.len() {
                    if glob_inner(rest, &s[i..]) {
                        return true;
                    }
                }
                false
            } else {
                // * —— 只在当前层级内匹配
                let rest = &p[1..];
                for i in 0..=s.len() {
                    if i > 0 && s[i - 1] == '/' {
                        break;
                    }
                    if glob_inner(rest, &s[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        '?' => !s.is_empty() && s[0] != '/' && glob_inner(&p[1..], &s[1..]),
        c => !s.is_empty() && s[0] == c && glob_inner(&p[1..], &s[1..]),
    }
}

/// 是不是「平台垃圾文件」（macOS / Windows 打出来的附赠物）
///
/// 只在 `Required` 模式下用来瘦身；**说明文件、广告推广都不算垃圾**（用户可能要看）。
pub fn is_junk(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or("");
    name == ".ds_store"
        || name == "thumbs.db"
        || name == "desktop.ini"
        || name == ".gitkeep"
        || name.starts_with("._") // macOS 资源分叉
        || lower.starts_with("__macosx/")
        || lower.contains("/__macosx/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 全都要时任何条目都匹配() {
        let s = Selection::all();
        assert!(s.is_all());
        assert!(s.matches(0, "随便/什么.txt", false));
        assert!(s.matches(7, "目录", true));
        assert_eq!(s.describe(), "all");
    }

    #[test]
    fn 只选指定序号() {
        let s = Selection::only(vec![1, 3]);
        assert!(s.matches(1, "a", false));
        assert!(!s.matches(2, "b", false));
        assert!(s.matches(3, "c", true), "目录也可以被显式选中");
        assert!(!s.is_all());
    }

    #[test]
    fn 按通配表达式挑选() {
        let s = Selection::matching("*.exe");
        assert!(s.matches(0, "游戏/Lake.exe", false), "不含 / 的模式匹配任意层级");
        assert!(s.matches(5, "Lake.exe", false));
        assert!(!s.matches(1, "游戏/readme.txt", false));
        assert_eq!(s.describe(), "matching[*.exe]");

        // 含 / 的模式按整个路径匹配
        let deep = Selection::matching("游戏/*.exe");
        assert!(deep.matches(0, "游戏/Lake.exe", false));
        assert!(!deep.matches(1, "游戏/子目录/Lake.exe", false), "* 不跨目录");
        let any = Selection::matching("游戏/**");
        assert!(any.matches(2, "游戏/子目录/Lake.exe", false));
    }

    #[test]
    fn 必需的会跳过目录与平台垃圾() {
        let s = Selection::required();
        assert!(s.matches(0, "游戏数据/pak.dat", false));
        assert!(!s.matches(1, "游戏数据", true), "目录不必单独解");
        assert!(!s.matches(2, "__MACOSX/._x", false));
        assert!(!s.matches(3, ".DS_Store", false));
        // 说明文件不是垃圾：用户可能要看解压码
        assert!(s.matches(4, "说明.txt", false));
        assert_eq!(s.describe(), "required");
    }

    #[test]
    fn 跳过目录开关生效() {
        let s = Selection::all().with_skip_dirs(true);
        assert!(!s.matches(0, "目录", true));
        assert!(s.matches(1, "文件.txt", false));
        assert!(!s.is_all(), "带 skip_dirs 的 all 不算「全都要」");
    }

    #[test]
    fn 通配星号不跨目录() {
        assert!(glob_match("游戏数据/*.pak", "游戏数据/a.pak"));
        assert!(glob_match("*.pak", "a.pak"));
        assert!(
            !glob_match("*.pak", "游戏数据/a.pak"),
            "单个 * 不应当跨过目录分隔符"
        );
    }

    #[test]
    fn 双星号跨目录() {
        assert!(glob_match("游戏数据/**", "游戏数据/a/b/c.pak"));
        assert!(glob_match("**/*.pak", "a/b/c.pak"));
        assert!(!glob_match("游戏数据/**", "别的地方/a.pak"));
        // `目录/**` 不吃掉目录本身
        assert!(!glob_match("游戏数据/**", "游戏数据"));
    }

    #[test]
    fn 问号匹配单个字符且不吃斜杠() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "a/c"), "? 不应当匹配目录分隔符");
        assert!(!glob_match("a?c", "ac"));
    }

    #[test]
    fn 中文路径可匹配() {
        assert!(glob_match("游戏数据/**/*.exe", "游戏数据/01/08/Lake.exe"));
        assert!(glob_match("*魔女*", "2-示例视频2.0"));
        assert!(
            !glob_match("*魔女*", "2-示例视频2.0/Lake.exe"),
            "单个 * 不跨目录 —— 跨层要用 ** 或交给 Selection（不含 / 的模式按文件名匹配）"
        );
        let s = Selection::matching("*魔女*");
        assert!(
            !s.matches(0, "2-示例视频2.0/Lake.exe", false),
            "不含 / 的模式只看文件名，Lake.exe 里没有「魔女」"
        );
        assert!(s.matches(1, "2-示例视频2.0", true), "条目名本身含「魔女」");
    }

    #[test]
    fn 从表达式解析各写法() {
        assert_eq!(Selection::parse("").unwrap(), Selection::all());
        assert_eq!(Selection::parse("all").unwrap(), Selection::all());
        assert_eq!(Selection::parse("required").unwrap(), Selection::required());
        assert_eq!(Selection::parse("*.exe").unwrap(), Selection::matching("*.exe"));
        assert_eq!(
            Selection::parse("3,1,3").unwrap(),
            Selection::only(vec![1, 3]),
            "序号要去重并排序"
        );
        let e = Selection::parse("这是什么鬼").unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument);
        assert_eq!(e.param("select"), Some("这是什么鬼"));
    }

    #[test]
    fn 平台垃圾判定() {
        assert!(is_junk("__MACOSX/._game"));
        assert!(is_junk("子目录/__MACOSX/x"));
        assert!(is_junk("Thumbs.db"));
        assert!(is_junk("a/b/.DS_Store"));
        assert!(!is_junk("说明.txt"));
        assert!(!is_junk("广告/推广.png"));
        assert!(!is_junk("windows/desktop.ini.bak"));
    }
}
