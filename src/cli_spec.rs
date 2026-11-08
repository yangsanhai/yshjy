// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! CLI 选项表 —— **唯一真源**（2026-10-28 新增，验收见 `docs/CLI-GUI-PLAN.md` §1）
//!
//! ## 为什么要有这个文件
//!
//! 同一份选项清单以前存在**三处手抄副本**：`main.rs` 的两份常量表、
//! `scripts/completions/` 的三份补全脚本、`docs/yshjy.1` man 页。
//! 手抄必然漂移，而且已经漂了 —— 实测（2026-10-28）：
//!
//! - `--help` 里写了 `--import-passwords`，但它**不在** `带值的选项` 里（真隐患：
//!   这个选项会取值，漏登记就意味着它的值可能被当成要解压的文件名 ——
//!   与当年 `--lang en 包.mp4` 把 `en` 当文件是同一个坑）；
//! - 手写补全只登记 18 个长选项，`--help` 实际有 35 个，**缺 17 个**。
//!
//! 所以把清单一律升成这张带字段的表，`main.rs` 的两份常量、补全脚本、man 页
//! **全部从这里推导**，并由 `tests/cli_spec_match.sh` 对账（三方不一致就红）。
//!
//! ## 加新选项的三步（别再手抄到别处）
//!
//! 1. 在这张表里加一行（长名 / 短名 / 值名 / 是否内部）；
//! 2. `cargo build` 会编译 `main.rs` 里那两个从表推导的函数，行为自动跟上；
//! 3. 跑 `bash tests/cli_spec_match.sh` 确认补全与 man 页已同步（它们也从这张表生成）。
//!
//! ## 本文件的两个硬约束
//!
//! - **只放纯数据**，不 `use` 任何 crate 内模块 —— 这样 `build.rs` 也能
//!   `include!` 它来生成补全与 man 页（照 `refs/upstream/ouch/build.rs:20-39` 的做法）；
//! - 长名/短名都**带前导横线**（`"--dest"` / `"-o"`），这样它们是 `&'static str`，
//!   能零成本喂给 `main.rs` 里原本期待 `&[&str]` 的代码。

/// 一个命令行选项的完整描述
pub struct Opt {
    /// 长名，带前导 `--`（如 `"--dest"`）
    pub long: &'static str,
    /// 短名，带前导 `-`（如 `Some("-o")`；`None` = 没有短形式）
    pub short: Option<&'static str>,
    /// `Some(值名)` = 这个选项**要跟一个值**（如 `Some("目录")`）；`None` = 开关型选项
    pub value: Option<&'static str>,
    /// 内部选项：不进 `--help`、不进 shell 补全（协议/调试专用）
    pub internal: bool,
}

impl Opt {
    /// 是否要跟一个值
    pub const fn takes_value(&self) -> bool {
        self.value.is_some()
    }
}

/// 表里的人手写法（一选项一行，方便 review —— 加选项只加一行）
const fn opt(
    long: &'static str,
    short: Option<&'static str>,
    value: Option<&'static str>,
    internal: bool,
) -> Opt {
    Opt { long, short, value, internal }
}

/// **全部命令行选项**（顺序 = `--help` 里的大致出现顺序，不参与语义）
pub const OPTS: &[Opt] = &[
    // ── 解压主路径 ──────────────────────────────────────────────
    opt("--list", Some("-l"), None, false),               // 只看里面有什么
    opt("--rescue", None, None, false),                   // 索引坏了按文件头救
    opt("--extract-cover", None, None, false),            // 只取封面
    opt("--dest", Some("-o"), Some("目录"), false),        // 解压到哪
    opt("--password", Some("-p"), Some("密码"), false),
    opt("--password-file", None, Some("文件"), false),
    opt("--no-auto-password", None, None, false),
    opt("--no-auto-password-notes", None, None, false),
    opt("--no-auto-password-book", None, None, false),
    opt("--encoding", None, Some("编码"), false),          // big5 / sjis / gbk / utf-8
    opt("--select", None, Some("条件"), false),            // 只解一部分
    opt("--candidate", None, Some("偏移"), false),          // 解「第 N 处」（一个文件里藏着多处归档时）
    opt("--pick", None, None, false),                     // 全屏勾选（dialoguer）
    opt("--jobs", Some("-j"), Some("N"), false),
    opt("--force", Some("-f"), None, false),              // 跳过磁盘空间预检
    opt("--test", None, None, false),                    // 只测不写：完整解码 + 校验，一个字节都不落盘
    opt("--no-smart-dir", None, None, false),
    opt("--trash-source", None, None, false),             // 解完把原包移进回收站（可还原）
    opt("--delete-source", None, None, false),            // 解完永久删除原包（要二次确认）
    // ── 打包路径 ────────────────────────────────────────────────
    opt("--split", None, Some("大小"), false),
    // 归档格式（T17-4）：一给就走「普通压缩」（不加外壳）；不给＝原来的伪装包
    opt("--format", None, Some("zip|tar.gz|tar.zst"), false),
    // 显示名（T18-2）：自定义产物文件名（伪装成 电影.2026.1080p.mp4 这种）。
    // **只改名字，不改位置** —— 目录仍由 -o/--dest 决定，避免两个出口互相打架。
    opt("--name", None, Some("名称"), false),
    // 防炸链（T18-3）：抖条目时间戳，让网盘当成新文件；默认关
    opt("--tweak-time", None, None, false),
    opt("--level", None, Some("0-9"), false),
    opt("--layout", None, Some("L1|L2|L3|L4|L7|L8"), false),
    opt("--second", None, Some("文件"), false),             // L7 的第二个包（现成 zip）
    opt("--decoy", None, Some("N"), false),                // 撒几段干扰数据（0 = 不打）
    opt("--decoy-size", None, Some("大小"), false),         // 每段多大（4K–1M）
    opt("--decoy-signatures", None, None, false),           // 每段夹一枚假特征码
    opt("--cover", None, Some("文件"), false),
    opt("--tail", None, None, false),
    opt("--no-checksum", None, None, false),
    opt("--no-readme", None, None, false),
    opt("--sfx", None, None, false),                      // 自解压 .exe
    opt("--sfx-linux", None, None, false),                // 自解压 .sh
    // ── 输出与配置 ──────────────────────────────────────────────
    opt("--json", None, None, false),
    opt("--quiet", Some("-q"), None, false),
    opt("--no-color", None, None, false),
    opt("--verbose", None, None, false),                  // 与 -v / -vv 同义的长写法
    opt("--log", None, Some("文件"), false),
    opt("--lang", None, Some("语言"), false),
    opt("--config", None, Some("文件"), false),
    opt("--show-config", None, None, false),
    opt("--caps", None, None, false),                     // 各格式能力表
    opt("--import-passwords", None, Some("文件"), false),  // ← 2026-10-28 补登记（此前漏）
    // ── 诊断 ────────────────────────────────────────────────────
    opt("--report", None, None, false),                   // 生成诊断包
    opt("--no-paths", None, None, false),                 // 配合 --report 脱敏
    // ── 帮助与版本 ──────────────────────────────────────────────
    opt("--help", Some("-h"), None, false),
    opt("--version", Some("-V"), None, false),
    // ── 内部选项（不进 help / 补全）──────────────────────────────
    opt("--serve", None, None, true),                     // 协议服务（界面走这条）
    opt("--debug-crash", None, None, true),               // 手动触发崩溃，测兜底
    opt("--dump-cli-spec", None, None, true),             // 把本表输出成 JSON（生成补全/man 用）
];

/// `-v` / `-vv` 是**重复计数型**短选项，不属于「长名 + 可选短名」的形状，
/// 所以单独登记（`--verbose` 的等价长形式已经在上表里）
pub const REPEAT_SHORTS: &[&str] = &["-v", "-vv"];

/// 表里所有长选项（含内部），用于对账与补全生成
///
/// 目前只有测试与生成器用；主二进制不调它，所以允许 dead_code。
#[allow(dead_code)]
pub fn long_flags() -> Vec<&'static str> {
    OPTS.iter().map(|o| o.long).collect()
}

/// 对外可见（非内部）的长选项 —— `--help` 与补全只认这些
///
/// 同上：留给生成器与对账探针用。
#[allow(dead_code)]
pub fn public_long_flags() -> Vec<&'static str> {
    OPTS.iter().filter(|o| !o.internal).map(|o| o.long).collect()
}

/// 会取值的选项（长名 + 短名）—— `main.rs` 收集文件名时必须跳过它们的值
pub fn value_flags() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for o in OPTS.iter().filter(|o| o.takes_value()) {
        v.push(o.long);
        if let Some(s) = o.short {
            v.push(s);
        }
    }
    v
}

/// 开关型选项（长名 + 短名）—— 用来拦住拼错的参数
/// （以前 `-extract-cover` 少打一个横线会被静默忽略 → 默认解压照跑）
pub fn switch_flags() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for o in OPTS.iter().filter(|o| !o.takes_value()) {
        v.push(o.long);
        if let Some(s) = o.short {
            v.push(s);
        }
    }
    for s in REPEAT_SHORTS {
        v.push(s);
    }
    v
}

/// 把表输出成 JSON —— `--dump-cli-spec` 用它，`scripts/gen-cli-assets.py` 读它生成
/// 补全与 man 页。
///
/// **生成器不许去解析 Rust 源码**（正则解析源码是最脆的做法），一律读这个 JSON。
pub fn dump_json() -> String {
    let mut s = String::from("{\n");
    s.push_str("  \"options\": [\n");
    for (i, o) in OPTS.iter().enumerate() {
        s.push_str(&format!(
            "    {{\"long\": {long}, \"short\": {short}, \"value\": {value}, \"internal\": {internal}}}{comma}\n",
            long = json_str(o.long),
            short = match o.short {
                Some(x) => json_str(x),
                None => "null".to_string(),
            },
            value = match o.value {
                Some(v) => json_str(v),
                None => "null".to_string(),
            },
            internal = o.internal,
            comma = if i + 1 == OPTS.len() { "" } else { "," },
        ));
    }
    s.push_str("  ],\n");
    s.push_str(&format!(
        "  \"repeat_shorts\": [{}]\n",
        REPEAT_SHORTS
            .iter()
            .map(|x| json_str(x))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    s.push_str("}\n");
    s
}

/// 最小的 JSON 字符串转义（我们只有选项名与值名，不会出现控制字符，但引号/反斜杠仍要挡）
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 长选项总数钉桩 —— 加选项时**必须**同步改这里（故意的：逼你确认补全/man 也跟着更新）
    ///
    /// 49 = 原有 37 + 2026-10-28 补登记的 `--import-passwords` + 生成器用的 `--dump-cli-spec`
    /// + `--pick`（全屏勾选）+ 2026-10-29 的 `--trash-source` / `--delete-source`
    /// + `--test`（只测不写：完整解码 + 校验，一个字节都不落盘）
    /// + 2026-10-29 第三期 B 的 `--decoy` / `--decoy-size` / `--decoy-signatures`
    /// + 2026-10-31 的 `--candidate <字节偏移>`（S10 多候选的命令行入口，T16）
    /// + 2026-10-31 的 `--format zip|tar.gz|tar.zst`（T17-4 打包格式可选）
    /// + 2026-10-31 的 `--tweak-time`（T18-3 防炸链）
    /// + 2026-10-31 的 `--name <名称>`（T18-2 自定义显示名）
    /// + 2026-11-01 的 `--second <文件>`（L7 的第二个包，一个现成的 zip）
    /// - 2026-11-01 删掉 `--keep`（T18-7：用户口径「打包完就删了，不用留」，整条链删除）
    #[test]
    fn 长选项总数是_52() {
        assert_eq!(
            OPTS.len(),
            52,  // −1：--keep 已删（T18-7）
            "加/删选项后请同步更新这个数字，并跑 tests/cli_spec_match.sh"
        );
    }

    /// `--dump-cli-spec` 输出的 JSON 必须能被解析，且含关键字段（生成器靠它吃饭）
    #[test]
    fn dump_json_形状正确() {
        let j = dump_json();
        assert!(j.contains("\"options\""));
        assert!(j.contains("\"repeat_shorts\""));
        assert!(j.contains("\"--dest\""));
        assert!(j.contains("\"目录\""), "值名要带上（补全/man 要用）");
        assert!(!j.contains("\\u"), "不该出现转义乱码");
    }

    /// 表内不许出现重复长名（复制粘贴最容易出的错）
    #[test]
    fn 长名唯一且带横线() {
        let mut seen = std::collections::HashSet::new();
        for o in OPTS {
            assert!(seen.insert(o.long), "重复的长选项：{}", o.long);
            assert!(o.long.starts_with("--"), "长名要带前导 --：{}", o.long);
            assert!(!o.long.contains(char::is_whitespace), "长名里有空格：{}", o.long);
        }
    }

    /// 短名也不许重复
    #[test]
    fn 短名唯一() {
        let mut seen = std::collections::HashSet::new();
        for o in OPTS.iter().filter_map(|o| o.short) {
            assert!(seen.insert(o), "重复的短选项：{o}");
            assert!(o.starts_with('-') && !o.starts_with("--"), "短名要写成 -x：{o}");
        }
    }

    /// 短名不许和 `-v/-vv` 撞（重复计数型单独管）
    #[test]
    fn 短名不与v冲突() {
        assert!(OPTS.iter().all(|o| o.short != Some("-v")), "-v 归 REPEAT_SHORTS 管");
    }

    /// `--import-passwords` 必须在带值清单里（2026-10-28 修的那个漏登记，别再漏）
    #[test]
    fn 导入密码本_带值() {
        assert!(value_flags().contains(&"--import-passwords"));
    }

    /// 取值清单与开关清单不许重叠
    #[test]
    fn 取值与开关不重叠() {
        let v: std::collections::HashSet<_> = value_flags().into_iter().collect();
        for s in switch_flags() {
            assert!(!v.contains(s), "{s} 同时出现在取值表和开关表里");
        }
    }

    /// 内部选项不进对外清单
    #[test]
    fn 内部选项不出现在对外清单() {
        let public = public_long_flags();
        assert!(!public.contains(&"--serve"));
        assert!(!public.contains(&"--debug-crash"));
    }

    /// 对外长选项必须都能在 `value_flags ∪ switch_flags` 里找到（否则解析器不认它）
    #[test]
    fn 对外选项都被解析器认识() {
        let mut all: std::collections::HashSet<&str> = value_flags().into_iter().collect();
        all.extend(switch_flags());
        for f in public_long_flags() {
            assert!(all.contains(f), "{f} 既不在取值表也不在开关表");
        }
    }
}
