//! yshjy 命令行 —— 「杨三海解压」
//!
//! 设计原则（见 `docs/CLI-SPEC.md`）：
//! 1. **默认行为最省事**：`yshjy 文件` 就能解压，不用带任何参数
//! 2. **说人话**：输出中文、给下一步提示，不做技术黑话
//! 3. **可脚本化**：退出码可判断、`-q` 只输出结果、`--list` 输出结构化文本
//! 4. **密码永远打码**
//!
//! 退出码：0 成功 / 1 一般错误 / 2 用法错误 / 3 不是压缩包 / 4 密码问题 / 5 部分成功

use std::path::PathBuf;
use std::process::ExitCode;

/// 退出码常量
mod code {
    /// 成功
    pub const OK: u8 = 0;
    /// 一般错误
    pub const ERR: u8 = 1;
    /// 用法错误
    pub const USAGE: u8 = 2;
    /// 不是压缩包
    pub const NOT_ARCHIVE: u8 = 3;
    /// 密码问题
    pub const PASSWORD: u8 = 4;
    /// 部分成功
    pub const PARTIAL: u8 = 5;
}

const HELP: &str = r#"杨三海解压（yshjy） —— 专治「伪装成视频/图片的压缩包」

用法：
  yshjy <文件>                    解压（默认）
  yshjy jy <文件>                 解压（写法完整，脚本推荐）
  yshjy ys <文件或文件夹>          打包成伪装文件
  yshjy --list <文件>             先看看里面有什么（不解压）
  yshjy --rescue <文件>           文件坏了也尽量救出来
  yshjy --help                    看这份帮助

常用选项：
  -o, --dest <目录>               解压到哪（默认：当前目录下的同名文件夹）
  -p, --password <密码>           解压码 / 打包密码
  -q, --quiet                     只输出结果（脚本用）
  --tail                          打包用"尾部追加"布局（默认藏进 free 原子）

例子：
  yshjy 示例视频.mp4            解开藏在视频里的压缩包
  yshjy --list 电影.mp4           只看里面有什么
  yshjy -p "demo-password" 游戏.mp4   带解压码解压
  yshjy ys 我的游戏/               把文件夹打包成伪装视频
  yshjy --rescue 坏了的.zip        文件坏了也尽量救

解不开？先试 yshjy --list 看能不能认出里面有什么。
"#;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let quiet = argv.iter().any(|a| a == "-q" || a == "--quiet");

    let val_of = |k: &str| -> Option<String> {
        argv.iter()
            .position(|a| a == k || a == short_of(k))
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let has = |k: &str| argv.iter().any(|a| a == k || a == short_of(k));

    if argv.len() < 2 {
        eprint!("{HELP}");
        return ExitCode::from(code::USAGE);
    }
    if has("--help") || has("-h") {
        print!("{HELP}");
        return ExitCode::from(code::OK);
    }

    // 收集"非选项参数"（文件名 / 目录 / 子命令）
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == "--dest" || a == "-o" || a == "--password" || a == "-p" {
            i += 2; // 跳过它的值
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        positional.push(a.clone());
        i += 1;
    }

    // 子命令识别（默认解压）
    // 另外：程序名如果是 yshys（安装脚本会建这个软链接），默认走打包模式
    let invoked = argv
        .first()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let mut cmd = if invoked.contains("yshys") {
        "ys"
    } else {
        "jy"
    };
    let mut targets: Vec<String> = Vec::new();
    for (idx, p) in positional.iter().enumerate() {
        if idx == 0 && matches!(p.as_str(), "jy" | "ys" | "x" | "a") {
            cmd = if matches!(p.as_str(), "ys" | "a") {
                "ys"
            } else {
                "jy"
            };
            continue;
        }
        targets.push(p.clone());
    }

    if targets.is_empty() {
        eprintln!("❌ 没告诉我要处理哪个文件。\n\n   用法：yshjy <文件>\n   更多：yshjy --help");
        return ExitCode::from(code::USAGE);
    }

    if has("--list") || has("-l") {
        return do_list(&targets, quiet);
    }
    if has("--rescue") {
        return do_rescue(&targets, &val_of("--dest"), quiet);
    }
    if cmd == "ys" {
        return do_pack(
            &targets,
            &val_of("--dest"),
            &val_of("--password"),
            has("--tail"),
            quiet,
        );
    }
    do_extract(&targets, &val_of("--dest"), &val_of("--password"), quiet)
}

/// 长选项对应的短选项（用于参数识别）
fn short_of(long: &str) -> &str {
    match long {
        "--dest" => "-o",
        "--password" => "-p",
        "--quiet" => "-q",
        "--list" => "-l",
        "--help" => "-h",
        _ => long,
    }
}

/// 把密码打码（**永远不回显**）
fn mask(pw: &str) -> String {
    let n = pw.chars().count();
    format!("{}{}", "•".repeat(n.min(6)), if n > 6 { "…" } else { "" })
}

/// 打开文件并定位压缩包（尾部窗口失败时自动全文件扫描兜底）
fn open_and_locate(
    path: &str,
) -> Result<(std::fs::File, u64, yshjy::layout::zip::ZipLayout), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("打不开 {path}：{e}"))?;
    let size = f
        .metadata()
        .map_err(|e| format!("读不到文件信息：{e}"))?
        .len();

    if let Ok(l) = yshjy::layout::zip::locate(&mut f, size) {
        return Ok((f, size, l));
    }
    // 兜底：全文件扫描（对付"包藏在中间"的情况）
    let hits = yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::strict())
        .map_err(|e| format!("扫描失败：{e}"))?;
    if hits.is_empty() {
        return Err(format!("{path} 里没找到压缩包（也可能这个格式还没支持）"));
    }
    // 用更宽的窗口再定位一次
    let l = yshjy::layout::zip::locate_with_window(&mut f, size, size)
        .map_err(|e| format!("找到了线索但读不出来：{e}"))?;
    Ok((f, size, l))
}

fn do_list(targets: &[String], quiet: bool) -> ExitCode {
    let path = &targets[0];
    match open_and_locate(path) {
        Ok((mut f, _size, layout)) => match yshjy::layout::zip::list_entries(&mut f, &layout) {
            Ok(entries) => {
                if quiet {
                    for e in &entries {
                        println!("{}", e.path);
                    }
                } else {
                    println!("📦 {path} 里有 {} 个文件：\n", entries.len());
                    let mut total = 0u64;
                    for e in entries.iter().take(50) {
                        let lock = if e.encrypted { "🔒" } else { "  " };
                        println!("   {lock} {:>12} 字节   {}", e.size, e.path);
                        total += e.size;
                    }
                    if entries.len() > 50 {
                        println!("   … 还有 {} 个", entries.len() - 50);
                    }
                    let enc = entries.iter().filter(|e| e.encrypted).count();
                    println!("\n   合计 {:.1} MB", total as f64 / 1_048_576.0);
                    if enc > 0 {
                        println!("   其中 {enc} 个带锁 🔒 —— 解压需要解压码");
                    }
                }
                ExitCode::from(code::OK)
            }
            Err(e) => {
                eprintln!("❌ 列不出内容：{e}");
                ExitCode::from(code::NOT_ARCHIVE)
            }
        },
        Err(msg) => {
            // 兜底：中央目录被改坏时，按 local header 重建清单（不写盘）
            if let Some(rc) = try_list_by_rescue(path, quiet) {
                return rc;
            }
            eprintln!("❌ {msg}");
            if !quiet {
                eprintln!("\n   提示：用 yshjy --rescue 试试救援模式。");
            }
            ExitCode::from(code::NOT_ARCHIVE)
        }
    }
}

/// 中央目录坏了时的兜底列举：按 local header 顺序重建
fn try_list_by_rescue(path: &str, quiet: bool) -> Option<ExitCode> {
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    let hits =
        yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::strict()).ok()?;
    let start = hits
        .iter()
        .find(|h| h.format == yshjy::layout::ArchiveFormat::Zip)
        .map(|h| h.offset)?;
    let items = yshjy::rescue::rescue_by_locals(
        &mut f,
        start,
        size,
        &yshjy::rescue::RescueOptions::default(),
    )
    .ok()?;
    if items.is_empty() {
        return None;
    }
    if quiet {
        for it in &items {
            println!("{}", it.path);
        }
    } else {
        println!(
            "🔧 中央目录坏了，已按文件头重建清单（{} 项）：\n",
            items.len()
        );
        let mut total = 0u64;
        for it in items.iter().take(50) {
            let lock = if it.encrypted() { "🔒" } else { "  " };
            println!("   {lock} {:>14} 字节   {}", it.pack_size, it.path);
            total += it.pack_size;
        }
        if items.len() > 50 {
            println!("   … 还有 {} 项", items.len() - 50);
        }
        println!("\n   合计 {:.2} GB", total as f64 / 1e9);
        println!("   提示：用 yshjy \"{path}\" 直接解压（缺的解压码加 -p）");
    }
    Some(ExitCode::from(code::OK))
}

fn do_extract(
    targets: &[String],
    dest_opt: &Option<String>,
    password: &Option<String>,
    quiet: bool,
) -> ExitCode {
    let path = &targets[0];
    // 先看是不是 7z（含分卷）—— 这类不是 ZIP，走另一条路
    if let Some(vols) = as_7z_volumes(std::path::Path::new(path)) {
        return do_extract_7z(&vols, dest_opt, password, quiet);
    }
    // 再看是不是 RAR
    if is_rar(std::path::Path::new(path)) {
        return do_extract_rar(path, dest_opt, password, quiet);
    }
    // 单流格式（gz / xz / zstd / bzip2）与 TAR —— 走流式解压
    if let Some(d) = sniff_stream(std::path::Path::new(path)) {
        return do_extract_stream(path, d, dest_opt, quiet);
    }

    let (mut f, size, layout) = match open_and_locate(path) {
        Ok(v) => v,
        Err(_) => {
            // 兜底：索引被改坏的包，用「扫起点 + 修索引」的路子救
            return do_extract_broken(path, dest_opt, password, quiet);
        }
    };
    let _ = size;

    // 默认输出目录：文件名去掉"外壳扩展名"后的同名文件夹
    let dest: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => {
            let stem = std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "解压结果".into());
            let cleaned = stem
                .trim_end_matches(".mp4")
                .trim_end_matches(".mkv")
                .trim_end_matches(".zip")
                .to_string();
            PathBuf::from(if cleaned.is_empty() { stem } else { cleaned })
        }
    };

    if !quiet {
        println!("📦 {path}");
        println!(
            "   里面 {} 个文件，解开到：{}",
            layout.entries_total,
            dest.display()
        );
        if let Some(pw) = password {
            println!("   解压码：{}（不回显）", mask(pw));
        }
    }

    let mut opts = yshjy::extract::ExtractOptions::new(&dest);
    opts.password = password.clone();

    // ── 用户没给密码？先看看旁边的「说明.txt」里有没有 ──
    let mut auto_pw_note: Option<String> = None;
    if opts.password.is_none() && layout.entries_total > 0 {
        let hints = yshjy::password_hint::find_hints_near(std::path::Path::new(path));
        if !hints.is_empty() {
            if !quiet {
                println!(
                    "   🔑 旁边 {} 里找到 {} 个候选解压码，试着用…",
                    hints[0]
                        .source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    hints.len()
                );
            }
            // 用一个条目先探一下：能列出内容就说明不需要密码，
            // 需要密码的话就逐个候选试（试到成功为止）
            for h in &hints {
                let mut probe = yshjy::extract::ExtractOptions::new(&dest);
                probe.password = Some(h.password.clone());
                if let Ok((rep, _)) =
                    yshjy::extract::extract_zip_auto(&mut f, layout.base, layout.data_len(), &probe)
                    && !rep.files.is_empty()
                {
                    auto_pw_note = Some(format!(
                        "从「{}」找到并用上了（原始行：{}）",
                        h.source.file_name().unwrap_or_default().to_string_lossy(),
                        h.line.trim()
                    ));
                    opts.password = Some(h.password.clone());
                    break;
                }
                if quiet {
                    continue;
                }
                println!("      试过：{}（不对）", mask(&h.password));
            }
        }
    }

    let started = std::time::Instant::now();

    // 用「带自动修复」的版本：索引坏了也能救（不改磁盘文件）
    match yshjy::extract::extract_zip_auto(&mut f, layout.base, layout.data_len(), &opts) {
        Ok((rep, repair_note)) => {
            if let Some(note) = &repair_note
                && !quiet
            {
                println!("   🔧 走了修复路径：{note}");
            }
            if let Some(note) = &auto_pw_note
                && !quiet
            {
                println!("   🔑 {note}");
            }
            if quiet {
                for fe in &rep.files {
                    println!("{}", fe.out_path.display());
                }
            } else {
                println!(
                    "\n✅ {}（用时 {:.1} 秒）",
                    rep.summary(),
                    started.elapsed().as_secs_f32()
                );
                for fe in rep.files.iter().take(8) {
                    println!("   {}  {}", if fe.encrypted { "🔓" } else { "  " }, fe.name);
                }
                if rep.files.len() > 8 {
                    println!("   … 还有 {} 个", rep.files.len() - 8);
                }
                for (n, why) in &rep.skipped {
                    println!("   ⚠ 跳过 {n}：{why}");
                }
                for w in &rep.warnings {
                    println!("   ℹ {w}");
                }
            }

            // 退出码：全成 0 / 部分成 5 / 全失败 1（或 4 密码问题）
            if !rep.skipped.is_empty() && !rep.files.is_empty() {
                return ExitCode::from(code::PARTIAL);
            }
            if rep.files.is_empty() {
                let pw_issue = rep
                    .skipped
                    .iter()
                    .any(|(_, why)| why.contains("密码") || why.contains("解密"));
                return ExitCode::from(if pw_issue { code::PASSWORD } else { code::ERR });
            }
            ExitCode::from(code::OK)
        }
        Err(e) => {
            eprintln!("❌ 解压失败：{e}");
            ExitCode::from(code::ERR)
        }
    }
}

/// 嗅探单流格式 / TAR（看文件头）
fn sniff_stream(path: &std::path::Path) -> Option<yshjy::extract_stream::Detected> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 512];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    yshjy::extract_stream::detect(&head)
}

/// 单流格式 / TAR 解压分支
fn do_extract_stream(
    path: &str,
    d: yshjy::extract_stream::Detected,
    dest_opt: &Option<String>,
    quiet: bool,
) -> ExitCode {
    use yshjy::layout::ArchiveFormat;
    let p = std::path::Path::new(path);
    let dest: PathBuf = match dest_opt {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(
            p.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "解压结果".into()),
        ),
    };

    let started = std::time::Instant::now();
    // 压缩数据大小（用于压缩比防护）
    let packed = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);

    // ── TAR 家族：展开成目录树 ──
    // 注意：`.tar.gz` 这种看头部看不出是 tar（只看到 gzip），必须按文件名判断
    let is_tar = d.is_tar || yshjy::extract_stream::looks_like_tar_by_name(p);
    if is_tar {
        let outer = if d.format == ArchiveFormat::Tar {
            None
        } else {
            Some(d.format)
        };
        if !quiet {
            println!(
                "📦 {path}（{}）",
                if outer.is_some() {
                    "压缩的 TAR"
                } else {
                    "TAR"
                }
            );
            println!("   展开到：{}", dest.display());
        }
        let f = match std::fs::File::open(p) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("❌ 打不开：{e}");
                return ExitCode::from(code::ERR);
            }
        };
        let mut opts = yshjy::extract_stream::ExtractStreamOptions::new(&dest);
        opts.packed_hint = packed;
        match yshjy::extract_stream::extract_tar(f, outer, &opts) {
            Ok(rep) => {
                if quiet {
                    for fe in &rep.files {
                        println!("{}", fe.out_path.display());
                    }
                } else {
                    println!(
                        "\n✅ {}（用时 {:.1} 秒）",
                        rep.summary(),
                        started.elapsed().as_secs_f32()
                    );
                    for fe in rep.files.iter().take(8) {
                        println!("     {}", fe.name);
                    }
                    for (n, why) in rep.skipped.iter().take(5) {
                        println!("   ⚠ 跳过 {n}：{why}");
                    }
                }
                if rep.files.is_empty() {
                    ExitCode::from(code::ERR)
                } else if rep.skipped.is_empty() {
                    ExitCode::from(code::OK)
                } else {
                    ExitCode::from(code::PARTIAL)
                }
            }
            Err(e) => {
                eprintln!("❌ 解压失败：{e}");
                ExitCode::from(code::ERR)
            }
        }
    } else {
        // ── 单流格式：解出一个文件 ──
        let out_name = yshjy::extract_stream::strip_suffix(p, d.format)
            .map(|x| x.to_string_lossy().to_string());
        let out_path = match &out_name {
            Some(n) => dest.join(n),
            None => {
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "解出".into());
                dest.join(stem)
            }
        };
        if !quiet {
            println!("📦 {path}（{:?} 单流格式）", d.format);
            println!("   解开到：{}", out_path.display());
        }
        let f = match std::fs::File::open(p) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("❌ 打不开：{e}");
                return ExitCode::from(code::ERR);
            }
        };
        match yshjy::extract_stream::decompress_stream(
            d.format,
            f,
            &out_path,
            &yshjy::safety::Limits::default(),
            packed,
        ) {
            Ok(n) => {
                if quiet {
                    println!("{}", out_path.display());
                } else {
                    println!(
                        "\n✅ 解出 {} 字节（用时 {:.1} 秒）",
                        n,
                        started.elapsed().as_secs_f32()
                    );
                }
                ExitCode::from(code::OK)
            }
            Err(e) => {
                eprintln!("❌ 解压失败：{e}");
                ExitCode::from(code::ERR)
            }
        }
    }
}

/// 按扩展名或签名判断是不是 RAR
fn is_rar(path: &std::path::Path) -> bool {
    let by_ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .map(|e| e == "rar")
        .unwrap_or(false);
    if by_ext {
        return true;
    }
    // 看签名（RAR4: "Rar!\x1a\x07\x00"，RAR5: "Rar!\x1a\x07\x01\x00"）
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 8];
        if f.read_exact(&mut buf).is_ok() {
            return buf.starts_with(b"Rar!\x1a\x07");
        }
    }
    false
}

/// RAR 解压分支
fn do_extract_rar(
    path: &str,
    dest_opt: &Option<String>,
    password: &Option<String>,
    quiet: bool,
) -> ExitCode {
    let dest: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(
            std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "解压结果".into()),
        ),
    };
    if !quiet {
        println!("📦 {path}（RAR）");
        println!("   解开到：{}", dest.display());
        if let Some(pw) = password {
            println!("   密码：{}（不回显）", mask(pw));
        }
    }
    let mut opts = yshjy::extract_rar::ExtractRarOptions::new(&dest);
    opts.password = password.clone();
    let started = std::time::Instant::now();

    match yshjy::extract_rar::extract_rar_file(std::path::Path::new(path), &opts) {
        Ok(rep) => {
            if quiet {
                for fe in &rep.files {
                    println!("{}", fe.out_path.display());
                }
            } else {
                println!(
                    "\n✅ {}（用时 {:.1} 秒）",
                    rep.summary(),
                    started.elapsed().as_secs_f32()
                );
                for fe in rep.files.iter().take(8) {
                    println!("     {}", fe.name);
                }
                for (n, why) in rep.skipped.iter().take(5) {
                    println!("   ⚠ 跳过 {n}：{why}");
                }
                for w in rep.warnings.iter().take(3) {
                    println!("   ℹ {w}");
                }
            }
            if rep.files.is_empty() {
                ExitCode::from(code::ERR)
            } else if rep.skipped.is_empty() {
                ExitCode::from(code::OK)
            } else {
                ExitCode::from(code::PARTIAL)
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let pw_issue = msg.contains("密码")
                || msg.to_lowercase().contains("password")
                || msg.to_lowercase().contains("crypto")
                || msg.contains("校验失败");
            eprintln!("❌ 解压失败：{e}");
            ExitCode::from(if pw_issue { code::PASSWORD } else { code::ERR })
        }
    }
}

/// 判断是不是 7z（含分卷），是的话返回完整的分卷列表
///
/// 支持的形式：
/// - `x.7z`（单卷）
/// - `x.7z.001` / `x.7z.002` …（7z 分卷）
/// - `x.001` / `x.002` …（裸分卷）
fn as_7z_volumes(path: &std::path::Path) -> Option<Vec<PathBuf>> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let lower = name.to_lowercase();

    // 单卷 .7z
    if lower.ends_with(".7z") {
        // 顺手检查是不是还有 .001 系列（有些人会两个都留）
        return Some(vec![path.to_path_buf()]);
    }

    // 分卷：找出"序号后缀"的基准名
    let (base, _num, width) = split_volume_name(&name)?;
    if !base.to_lowercase().ends_with(".7z") && !base.to_lowercase().ends_with(".001") {
        // 只在明显是 7z 分卷名时自动串联；否则让调用方自己给全路径
        let first = path.to_path_buf();
        if !base.to_lowercase().ends_with(".7z") {
            // 仍然当作 7z 分卷试一次（裸 xxx.001）
        }
        let _ = first;
    }

    // 依序找出所有分卷
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut vols = Vec::new();
    for i in 1..=9999u32 {
        let cand_name = format!("{base}{i:0width$}");
        let cand = dir.join(&cand_name);
        if cand.exists() {
            vols.push(cand);
        } else if i > 1 {
            break; // 序号断了就停
        } else {
            return None; // 第一块都不在，说明不是分卷
        }
    }
    if vols.is_empty() { None } else { Some(vols) }
}

/// 从 `x.7z.001` 这样的名字里拆出 (前缀, 序号, 序号宽度)
fn split_volume_name(name: &str) -> Option<(String, u32, usize)> {
    // 从尾部取连续数字
    let digits_start = name.rfind(|c: char| !c.is_ascii_digit())? + 1;
    if digits_start >= name.len() {
        return None;
    }
    let digits = &name[digits_start..];
    let num: u32 = digits.parse().ok()?;
    let base = name[..digits_start].to_string();
    Some((base, num, digits.len()))
}

/// 7z（含分卷）解压分支
fn do_extract_7z(
    volumes: &[PathBuf],
    dest_opt: &Option<String>,
    password: &Option<String>,
    quiet: bool,
) -> ExitCode {
    let dest: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => {
            let stem = volumes[0]
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "解压结果".into());
            // 去掉 .7z / .001 之类后缀
            let cleaned = stem
                .trim_end_matches(".7z")
                .trim_end_matches(".001")
                .to_string();
            PathBuf::from(if cleaned.is_empty() { stem } else { cleaned })
        }
    };

    if !quiet {
        println!("📦 {} 个分卷（7z）", volumes.len());
        for v in volumes.iter().take(4) {
            println!("   {}", v.file_name().unwrap_or_default().to_string_lossy());
        }
        if volumes.len() > 4 {
            println!("   … 还有 {} 个", volumes.len() - 4);
        }
        println!("   解开到：{}", dest.display());
        if let Some(pw) = password {
            println!("   密码：{}（不回显）", mask(pw));
        }
    }

    let mut opts = yshjy::extract7z::Extract7zOptions::new(&dest);
    opts.password = password.clone();
    let started = std::time::Instant::now();

    match yshjy::extract7z::extract_7z_volumes(volumes, &opts) {
        Ok(rep) => {
            if quiet {
                for fe in &rep.files {
                    println!("{}", fe.out_path.display());
                }
            } else {
                println!(
                    "\n✅ {}（用时 {:.1} 秒）",
                    rep.summary(),
                    started.elapsed().as_secs_f32()
                );
                for fe in rep.files.iter().take(8) {
                    println!("     {}", fe.name);
                }
                if rep.files.len() > 8 {
                    println!("   … 还有 {} 个", rep.files.len() - 8);
                }
                for (n, why) in rep.skipped.iter().take(5) {
                    println!("   ⚠ 跳过 {n}：{why}");
                }
                for w in rep.warnings.iter().take(3) {
                    println!("   ℹ {w}");
                }
            }
            if rep.files.is_empty() {
                ExitCode::from(code::ERR)
            } else if rep.skipped.is_empty() {
                ExitCode::from(code::OK)
            } else {
                ExitCode::from(code::PARTIAL)
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let pw_issue = msg.contains("密码")
                || msg.to_lowercase().contains("password")
                || msg.to_lowercase().contains("crypto");
            eprintln!("❌ 解压失败：{e}");
            ExitCode::from(if pw_issue { code::PASSWORD } else { code::ERR })
        }
    }
}

/// 索引被改坏时的兜底解压路径
fn do_extract_broken(
    path: &str,
    dest_opt: &Option<String>,
    password: &Option<String>,
    quiet: bool,
) -> ExitCode {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("❌ 打不开 {path}：{e}");
            return ExitCode::from(code::ERR);
        }
    };
    let size = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("❌ 读不到文件信息：{e}");
            return ExitCode::from(code::ERR);
        }
    };
    let dest: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(
            std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "解压结果".into()),
        ),
    };
    let mut opts = yshjy::extract::ExtractOptions::new(&dest);
    opts.password = password.clone();

    if !quiet {
        println!("🔧 {path} 的索引有问题，尝试修复后解压…");
    }
    match yshjy::repair::rescue_broken_index(&mut f, size, &opts) {
        Ok((rep, note)) => {
            if !quiet {
                println!("   🔧 {note}");
                println!("   ✅ {}", rep.summary());
                for fe in rep.files.iter().take(8) {
                    println!(
                        "      {}  {}",
                        if fe.encrypted { "🔓" } else { "  " },
                        fe.name
                    );
                }
            } else {
                for fe in &rep.files {
                    println!("{}", fe.out_path.display());
                }
            }
            if rep.files.is_empty() {
                ExitCode::from(code::ERR)
            } else if rep.skipped.is_empty() {
                ExitCode::from(code::OK)
            } else {
                ExitCode::from(code::PARTIAL)
            }
        }
        Err(e) => {
            eprintln!("❌ 修不了：{e}");
            ExitCode::from(code::NOT_ARCHIVE)
        }
    }
}

fn do_pack(
    targets: &[String],
    dest_opt: &Option<String>,
    password: &Option<String>,
    tail: bool,
    quiet: bool,
) -> ExitCode {
    let inputs: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();
    for i in &inputs {
        if !i.exists() {
            eprintln!("❌ 找不到：{}", i.display());
            return ExitCode::from(code::ERR);
        }
    }
    let out: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => {
            let base = inputs[0]
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "packed".into());
            PathBuf::from(format!("{base}.mp4"))
        }
    };

    let mut opts = yshjy::pack::PackOptions {
        inputs: inputs.clone(),
        password: password.clone(),
        ..Default::default()
    };
    if tail {
        opts.layout = yshjy::pack::PackLayout::TailAppend;
    }

    if !quiet {
        println!("📦 打包 {} 项 → {}", inputs.len(), out.display());
        if let Some(pw) = password {
            println!("   加密：{}（不回显）", mask(pw));
        }
    }

    let f = match std::fs::File::create(&out) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("❌ 建不了输出文件：{e}");
            return ExitCode::from(code::ERR);
        }
    };
    let mut bw = std::io::BufWriter::new(f);
    match yshjy::pack::build(&opts, &mut bw) {
        Ok(rep) => {
            use std::io::Write;
            let _ = bw.flush();
            drop(bw);
            if quiet {
                println!("{}", out.display());
            } else {
                println!("✅ {}", rep.summary());
            }
            // 自检 —— 不能"以为成功就完事"
            match yshjy::pack::self_check(&out, rep.entries) {
                Ok(warns) if warns.is_empty() => {
                    if !quiet {
                        println!("   ✅ 自检通过（这个文件能播放，也能被我们解回来）");
                    }
                    ExitCode::from(code::OK)
                }
                Ok(warns) => {
                    for w in warns {
                        eprintln!("   ⚠ {w}");
                    }
                    ExitCode::from(code::PARTIAL)
                }
                Err(e) => {
                    eprintln!("   ❌ 自检失败：{e}");
                    ExitCode::from(code::ERR)
                }
            }
        }
        Err(e) => {
            eprintln!("❌ 打包失败：{e}");
            ExitCode::from(code::ERR)
        }
    }
}

fn do_rescue(targets: &[String], dest_opt: &Option<String>, quiet: bool) -> ExitCode {
    let path = &targets[0];
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("❌ 打不开 {path}：{e}");
            return ExitCode::from(code::ERR);
        }
    };
    let size = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("❌ 读不到文件信息：{e}");
            return ExitCode::from(code::ERR);
        }
    };

    if !quiet {
        println!("🩺 救援模式：不看中央目录，按每个文件的头重建清单…");
    }
    let hits = match yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::strict())
    {
        Ok(h) => h,
        Err(e) => {
            eprintln!("❌ 扫描失败：{e}");
            return ExitCode::from(code::ERR);
        }
    };
    let start = match hits
        .iter()
        .find(|h| h.format == yshjy::layout::ArchiveFormat::Zip)
        .map(|h| h.offset)
    {
        Some(s) => s,
        None => {
            eprintln!("❌ 没找到任何压缩包的痕迹");
            return ExitCode::from(code::NOT_ARCHIVE);
        }
    };

    match yshjy::rescue::rescue_by_locals(
        &mut f,
        start,
        size,
        &yshjy::rescue::RescueOptions::default(),
    ) {
        Ok(items) => {
            if quiet {
                for it in &items {
                    println!("{}", it.path);
                }
            } else {
                println!("✅ 重建出 {} 个条目：", items.len());
                for it in items.iter().take(30) {
                    let lock = if it.encrypted() { "🔒" } else { "  " };
                    println!("   {lock} {:>14} 字节   {}", it.pack_size, it.path);
                }
            }

            // ── 真的把数据救出来（中央目录坏了时的救命名路径）──
            let dest: PathBuf = match dest_opt {
                Some(d) => PathBuf::from(d),
                None => {
                    let stem = std::path::Path::new(path)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "救援结果".into());
                    PathBuf::from(format!("{stem}.rescue"))
                }
            };
            let eopts = yshjy::extract::ExtractOptions::new(&dest);
            if !quiet {
                println!("\n📤 按文件头逐个抽取数据 → {}", dest.display());
            }
            match yshjy::rescue::extract_by_locals(&mut f, start, size, &eopts) {
                Ok(rep) => {
                    if quiet {
                        for fe in &rep.files {
                            println!("{}", fe.out_path.display());
                        }
                    } else {
                        println!("   ✅ {}", rep.summary());
                        for fe in rep.files.iter().take(8) {
                            println!("      {}  ({} 字节)", fe.name, fe.size);
                        }
                        for (n, why) in rep.skipped.iter().take(5) {
                            println!("      ⚠ {n}：{why}");
                        }
                    }
                    if rep.files.is_empty() {
                        ExitCode::from(code::ERR)
                    } else {
                        ExitCode::from(code::OK)
                    }
                }
                Err(e) => {
                    eprintln!("   ❌ 抽取失败：{e}");
                    ExitCode::from(code::ERR)
                }
            }
        }
        Err(e) => {
            eprintln!("❌ 救援失败：{e}");
            ExitCode::from(code::ERR)
        }
    }
}
