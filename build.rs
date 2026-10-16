//! 构建脚本：把 `locales/*.toml` 嵌进二进制
//!
//! **为什么要 build.rs 而不是手写 `include_str!` 列表**：
//! 这样「加一种新语言」只要往 `locales/` 丢一个 toml 文件就行 —— **不用改一行代码**。
//! 这是我们对社区的承诺（谁想加自己母语，翻译一个文本文件即可）。

use std::path::{Path, PathBuf};

fn main() {
    let dir = Path::new("locales");
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=build.rs");

    // 扫出所有语言包：文件名（不含扩展名）就是语言代码
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let Some(code) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let code = code.to_string();
            if code.is_empty() {
                continue;
            }
            files.push((code, path));
        }
    }
    files.sort();

    let mut src = String::from(
        "/// 编译期嵌入的语言包：(语言代码, TOML 文本)\n\
         ///\n\
         /// 由 `build.rs` 扫描 `locales/*.toml` 生成 —— 加语言只需加文件。\n\
         pub const BUILTIN: &[(&str, &str)] = &[\n",
    );
    for (code, path) in &files {
        // 用绝对路径：生成的代码在 OUT_DIR 里，相对路径会找不到
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        src.push_str(&format!(
            "    ({code:?}, include_str!({:?})),\n",
            abs.to_string_lossy()
        ));
    }
    src.push_str("];\n");

    let out = std::env::var("OUT_DIR").expect("OUT_DIR 未设置");
    std::fs::write(Path::new(&out).join("locales.rs"), src).expect("写 locales.rs 失败");

    if files.is_empty() {
        println!("cargo:warning=locales/ 里没有语言包 —— 界面会只显示键名");
    } else {
        println!("cargo:warning=已嵌入 {} 种语言包", files.len());
    }
}
