//! 自动抓解压码 —— 从旁边的「说明.txt」里把密码找出来
//!
//! # 为什么做这个
//! 这类资源的说明文件长得几乎一样：
//! ```text
//! 解压码：demo-password
//! ```
//! 用户每次都要：打开说明 → 选中 → 复制 → 切回来 → 粘贴。
//! 我们直接**在旁边找**，找不到再让用户输入。
//!
//! # 边界（很重要）
//! - 只**读**文本，不改任何文件
//! - 找错了会试错但不会卡住（试不出就回到"让用户输入"）
//! - **不做字典爆破**（那是另一回事，且容易被滥用）
//! - 抓到的候选**只在内存里**用于尝试，日志里永远打码

use std::fs;
use std::path::{Path, PathBuf};

/// 一个解压码候选（带出处，方便用户核验）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordHint {
    /// 候选密码
    pub password: String,
    /// 从哪个文件、哪一行找到的
    pub source: PathBuf,
    /// 那一行的原文（供用户核对）
    pub line: String,
    /// 置信度（高 = 明确的"解压码："标签）
    pub confidence: u8,
}

/// 说明文件可能的名字（不含扩展名的部分）
const DOC_STEMS: &[&str] = &[
    "说明",
    "readme",
    "README",
    "Readme",
    "解压说明",
    "必看",
    "重要",
    "使用说明",
    "解压密码",
    "密码",
    "pass",
    "howto",
    "help",
];

/// 说明文件可能的扩展名
const DOC_EXTS: &[&str] = &["txt", "md", "nfo", "TXT", "TXT ", "log", "ini"];

/// 标签词 → 置信度（越高越可信）
const LABELS: &[(&str, u8)] = &[
    ("解压码", 100),
    ("解压密码", 100),
    ("提取码", 90),
    ("解压口令", 95),
    ("密码", 80),
    ("passwd", 60),
    ("password", 60),
    ("pass", 50),
    ("pwd", 50),
];

/// 在**小写化**的文本里找一个 ASCII 标签，且要求它卡在词边界上
///
/// # 为什么必须有词边界
/// `pass` 是个短标签，几乎藏在任何英文单词里：`passed` / `bypass` / `passwordless`。
/// 不做边界检查的后果**真实发生过**：`test result: ok. 251 passed; 1 failed;` 这行
/// 会被抓出候选解压码 `ed`（`pass` 之后的残留）—— 于是"旁边放着一份日志"的用户
/// 会白试一堆垃圾密码，界面还会说"从说明文件里抓到了密码"。
///
/// 只对 ASCII 标签做这件事：中文标签（`解压码`）的词边界是天然的，
/// 而按字节去切中文会切坏 UTF-8。
fn 找ascii标签(text_lower: &str, label_lower: &str) -> Option<usize> {
    if label_lower.is_empty() {
        return None;
    }
    let mut from = 0usize;
    while from < text_lower.len() {
        let rel = text_lower[from..].find(label_lower)?;
        let pos = from + rel;
        let end = pos + label_lower.len();
        let 词字符 = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !词字符(text_lower[..pos].chars().next_back()) && !词字符(text_lower[end..].chars().next()) {
            return Some(pos);
        }
        // 这次命中不合法（是别的单词的一部分）→ 从标签末尾继续找下一个
        from = end;
    }
    None
}

/// 从一段文本里抽出候选解压码
///
/// 支持的写法（实测资源里常见的）：
/// - `解压码：xxx` / `解压码: xxx` / `解压码 xxx`
/// - `密码   ＝ xxx`
/// - `password: xxx`
/// - 中文全角冒号、空格、Tab 分隔都认
pub fn extract_passwords(text: &str) -> Vec<(String, u8, String)> {
    let mut out: Vec<(String, u8, String)> = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.len() > 300 {
            continue;
        }
        let lower = line.to_lowercase();

        for (label, conf) in LABELS {
            let label_lower = label.to_lowercase();
            let pos = if label.is_ascii() {
                找ascii标签(&lower, &label_lower)
            } else {
                line.find(label)
            };
            let Some(pos) = pos else { continue };

            // 标签之后的部分
            let after = &line[pos + label.len()..];
            // 去掉分隔符（冒号/等号/空格/Tab，含全角）
            let val = after.trim_start_matches(|c: char| {
                c == ':'
                    || c == '：'
                    || c == '='
                    || c == '＝'
                    || c == '-'
                    || c == '—'
                    || c == ' '
                    || c == '\t'
                    || c == '　'
                    || c == '】'
                    || c == ']'
                    || c == '）'
            });
            let val = val.trim();
            if val.is_empty() || val.chars().count() > 64 {
                continue;
            }
            // 排除明显的句子（"请勿传播" 这种）
            if val.contains("请") || val.contains("勿") || val.contains("注意") {
                continue;
            }
            // 取到行尾，或遇到明显的分隔（空格后跟另一个标签）
            let candidate = val
                .split([' ', '\t', '　', ',', '，', ';', '；'])
                .next()
                .unwrap_or(val)
                .trim()
                .to_string();
            // 注意：这里必须按**字符数**算，不能按字节数
            // （中文标点一个就 3 字节，用 .len() 会把"。"当成合法密码）
            let n_chars = candidate.chars().count();
            if !(2..=64).contains(&n_chars) {
                continue;
            }
            // 排除"只有标点/空白"的候选
            if candidate.chars().all(|c| !c.is_alphanumeric()) {
                continue;
            }
            out.push((candidate.clone(), *conf, line.to_string()));
            // 一行里也可能既有"解压码"又有"提取码"，都要
            break;
        }
    }

    // 去重（保留置信度最高的）
    out.sort_by_key(|(_, conf, _)| std::cmp::Reverse(*conf));
    let mut seen = std::collections::HashSet::new();
    out.retain(|(pw, _, _)| seen.insert(pw.clone()));
    out
}

/// 在目标文件**旁边**（同目录、必要时上一级）找说明文件并抽取解压码
///
/// 返回按置信度排序的候选。完全找不到就返回空 vec（调用方照常让用户输入）。
pub fn find_hints_near(target: &Path) -> Vec<PasswordHint> {
    let mut hints = Vec::new();
    // 注意：`Path::new("a.txt").parent()` 返回的是**空路径**（不是 None），
    // 拿空路径去 read_dir 会失败 —— 必须兜成当前目录 "."。
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };

    // 候选目录：同目录 + 上一级（有时说明在外层）
    let mut dirs: Vec<&Path> = vec![dir];
    if let Some(up) = dir.parent()
        && !up.as_os_str().is_empty()
    {
        dirs.push(up);
    }

    for d in dirs {
        let Ok(rd) = fs::read_dir(d) else { continue };
        for ent in rd.flatten() {
            let path = ent.path();
            if !path.is_file() {
                continue;
            }
            // 跳过目标自己
            if path == target {
                continue;
            }
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            let Some(ext) = path
                .extension()
                .map(|s| s.to_string_lossy().trim().to_string())
            else {
                continue;
            };
            let looks_like_doc =
                DOC_STEMS.iter().any(|s| stem.contains(s)) || DOC_EXTS.iter().any(|e| e == &ext);
            if !looks_like_doc {
                continue;
            }
            // 体积限制：说明文件不该很大
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.len() > 2 * 1024 * 1024 {
                continue;
            }
            let Ok(text) = fs::read(&path) else { continue };
            let text = decode_text(&text);
            for (password, confidence, line) in extract_passwords(&text) {
                hints.push(PasswordHint {
                    password,
                    source: path.clone(),
                    line,
                    confidence,
                });
            }
        }
    }

    hints.sort_by_key(|h| std::cmp::Reverse(h.confidence));
    // 去重
    let mut seen = std::collections::HashSet::new();
    hints.retain(|h| seen.insert(h.password.clone()));
    hints.truncate(12); // 候选别太多，免得试密码试到天荒地老
    hints
}

/// 宽松解码文本（GBK / UTF-8 / 带 BOM 都认）
fn decode_text(bytes: &[u8]) -> String {
    // 去 BOM
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (s, _, had_errors) = encoding_rs::GB18030.decode(bytes);
    if !had_errors {
        return s.into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 能抓中文全角冒号() {
        let r = extract_passwords("解压码：demo-password\n其他内容");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "demo-password");
        assert_eq!(r[0].1, 100);
    }

    #[test]
    fn 能抓半角冒号和等号() {
        for line in [
            "解压码: abc123",
            "解压码：abc123",
            "解压码 = abc123",
            "密码\txyz789",
            "password: p@ssw0rd",
        ] {
            let r = extract_passwords(line);
            assert_eq!(r.len(), 1, "应抓到一个: {line} → {r:?}");
        }
    }

    #[test]
    fn 能抓全角空格分隔() {
        let r = extract_passwords("解压密码　我的口令");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "我的口令");
    }

    #[test]
    fn 一行多个标签会优先高置信度() {
        let r = extract_passwords("提取码：aaa\n解压码：bbb");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "bbb", "解压码置信度应更高");
        assert!(r[0].1 > r[1].1);
    }

    #[test]
    fn 排除明显不是密码的句子() {
        let r = extract_passwords("注意：请勿传播此资源");
        assert!(r.is_empty(), "不该把提示语当密码: {r:?}");
    }

    #[test]
    fn 找不到就返回空() {
        assert!(extract_passwords("这是一份普通的说明文件，没有密码。").is_empty());
        assert!(extract_passwords("").is_empty());
    }

    #[test]
    fn 能在目录里找到说明文件并抓码() {
        let dir = std::env::temp_dir().join(format!("yshjy_pwhint_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("说明.txt"),
            "视频压缩包解压说明\n解压码：demo-password\n只能用winrar解压！\n",
        )
        .unwrap();
        let target = dir.join("假视频.mp4.zip");
        fs::write(&target, b"fake").unwrap();

        let hints = find_hints_near(&target);
        assert!(!hints.is_empty(), "应当找到候选");
        assert_eq!(hints[0].password, "demo-password");
        assert!(
            hints[0]
                .source
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("说明")
        );
        assert!(hints[0].line.contains("解压码"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 相对路径也能找到说明文件() {
        // 复现过的 bug：相对文件名（无目录部分）的 parent() 是空路径，
        // 直接 read_dir("") 会失败 → 抓不到说明文件
        let dir = std::env::temp_dir().join(format!("yshjy_rel_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("说明.txt"), "解压码：相对路径测试").unwrap();
        fs::write(dir.join("t.mp4"), b"x").unwrap();

        // 切到那个目录，用**相对文件名**调用
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let hints = find_hints_near(Path::new("t.mp4"));
        std::env::set_current_dir(old).unwrap();

        assert!(!hints.is_empty(), "相对路径下也该能找到说明文件");
        assert_eq!(hints[0].password, "相对路径测试");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 隔壁是二进制大文件不会浪费时间去读() {
        // ⚠️ `find_hints_near` 会连**上一级目录**一起扫（说明文件有时放在外层），
        // 所以目标必须放在自己造的**深层目录**里。直接放在 `temp_dir()` 下面的话，
        // TMPDIR 里随便一份含 "pass" 的日志就能让这个测试挂 —— 真实踩过
        //（当时 TMPDIR 里躺着一份 cargo 输出）。
        let 根 = std::env::temp_dir().join(format!("yshjy_pwhint2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&根);
        let dir = 根.join("包");
        fs::create_dir_all(&dir).unwrap();
        // 一个 3 MB 的 .txt（超过 2 MB 限制）不该被读
        let big = dir.join("说明.txt");
        fs::write(&big, vec![b'x'; 3 * 1024 * 1024]).unwrap();
        let target = dir.join("x.mp4");
        fs::write(&target, b"x").unwrap();
        assert!(
            find_hints_near(&target).is_empty(),
            "超过 2 MB 的说明文件不该被读（上级 {} 里也确实没有说明文件）",
            根.display()
        );
        let _ = fs::remove_dir_all(&根);
    }

    #[test]
    fn 英文单词里的pass不算标签() {
        // 真实踩到：cargo 的 `test result: ok. 251 passed; 1 failed;` 曾被抓出 `ed`
        let 日志 = "test result: ok. 251 passed; 1 failed; 0 ignored; 0 measured";
        assert!(
            extract_passwords(日志).is_empty(),
            "passed 里的 pass 不是标签，实际抓到：{:?}",
            extract_passwords(日志)
        );
        assert!(extract_passwords("bypass the check").is_empty());
        assert!(extract_passwords("passwordless login").is_empty());
    }

    #[test]
    fn 真正的password写法照样能抓到() {
        // 别为了修 bug 把正常写法一起修没了
        let got = extract_passwords("password: hunter2");
        assert_eq!(got[0].0, "hunter2", "实际：{got:?}");
        let got = extract_passwords("解压码 abc123");
        assert_eq!(got[0].0, "abc123", "实际：{got:?}");
        let got = extract_passwords("密码 ＝ 中文密码7");
        assert_eq!(got[0].0, "中文密码7", "实际：{got:?}");
        // 标签被空格围着、在句子中间也要认（`pass: x` 这种写法实测存在）
        let got = extract_passwords("提取 pass: zz99");
        assert!(got.iter().any(|(p, _, _)| p == "zz99"), "实际：{got:?}");
    }
}
