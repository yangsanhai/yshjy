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

/// 退出码常量 —— **全部从引擎取**，外壳不许自己定数
///
/// 台账 `7zip-工程层.md` §7.6 的反面案例：7-Zip 的命令行壳和 GUI 壳各算一份退出码，
/// 结果两边行为不一致。所以我们只在 `yshjy::error::exit_code` 里定义一次。
mod code {
    /// 成功
    pub const OK: u8 = yshjy::error::exit_code::OK as u8;
    /// 一般错误
    pub const ERR: u8 = yshjy::error::exit_code::ERROR as u8;
    /// 用法错误（参数不对 —— 这一档只有外壳用得上）
    pub const USAGE: u8 = yshjy::error::exit_code::USAGE as u8;
    /// 不是压缩包
    pub const NOT_ARCHIVE: u8 = yshjy::error::exit_code::NOT_ARCHIVE as u8;
    /// 部分成功
    pub const PARTIAL: u8 = yshjy::error::exit_code::PARTIAL as u8;
}

const HELP: &str = r#"杨三海解压（yshjy） —— 专治「伪装成视频/图片的压缩包」

用法：
  yshjy <文件>                    解压（默认）
  yshjy jy <文件>                 解压（写法完整，脚本推荐）
  yshjy ys <文件或文件夹>          打包成伪装文件
  yshjy --list <文件>             先看看里面有什么（不解压）
  yshjy --rescue <文件>           文件坏了也尽量救出来
  yshjy --help                    看这份帮助
  yshjy -V, --version             看版本和档位（常用版 / 全能版）

常用选项：
  -o, --dest <目录>               解压到哪（默认：当前目录下的同名文件夹）
  -p, --password <密码>           解压码 / 打包密码
  -j, --jobs <N>                  同时跑几个（默认 1，按顺序来最省磁盘）
  --select <条件>                 只解一部分：
                                    --select "*.exe"      只要 exe
                                    --select "游戏数据/**" 只要某个目录
                                    --select "1,3,5"      只要第 1、3、5 个
                                    --select required     跳过 .DS_Store 之类的垃圾
  --json                          按行输出事件流/结果（给脚本用）
  --no-smart-dir                  关掉「智能解压」：默认会看包里有没有单一顶层目录来决定
                                  要不要多套一层同名文件夹，加这个就永远建同名文件夹
  -q, --quiet                     只输出结果（脚本用）
  --tail                          打包用"尾部追加"布局（默认藏进 free 原子）

例子：
  yshjy 示例视频.mp4            解开藏在视频里的压缩包
  yshjy 下载/                     整个目录里的包排队解（一个坏了不影响其余）
  yshjy --list 电影.mp4           只看里面有什么
  yshjy --select "*.exe" 游戏.mp4  只要里面的 exe
  yshjy -p "demo-password" 游戏.mp4   带解压码解压
  yshjy ys 我的游戏/               把文件夹打包成伪装视频
  yshjy --rescue 坏了的.zip        文件坏了也尽量救
  yshjy --json 包.mp4 | jq .       给脚本/前端消费

解不开？先试 yshjy --list 看能不能认出里面有什么。
"#;

/// `--version` 的输出
///
/// **档位是编译期定死的**：不带 `--features full` 编出来的就是「常用版」。
fn 版本信息() -> String {
    let 档 = if cfg!(feature = "full") {
        "全能版"
    } else {
        "常用版"
    };
    let mut s = format!("yshjy {}（{档}）—— 杨三海解压\n", env!("CARGO_PKG_VERSION"));
    s.push_str(
        "  支持的格式：ZIP（含分卷）/ 7z / RAR / TAR / gzip / xz / zstd / bzip2 / brotli / lz4\n",
    );
    if cfg!(feature = "full") {
        s.push_str("  全能版：冷门格式（ISO/UDF、CAB、ARJ…）陆续补进来，当前覆盖与常用版一致\n");
    } else {
        s.push_str("  常用版：日常下载会遇到的格式全都在（体积更小、依赖更少）\n");
    }
    s
}

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
    if has("--version") || has("-V") {
        print!("{}", 版本信息());
        return ExitCode::from(code::OK);
    }

    // 收集"非选项参数"（文件名 / 目录 / 子命令）
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if matches!(
            a.as_str(),
            "--dest" | "-o" | "--password" | "-p" | "--jobs" | "-j" | "--select"
        ) {
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

    let json = has("--json");
    if has("--list") || has("-l") {
        return do_list(&targets, quiet, json);
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

    // ── 解压（v0.5：多目标 / 批量 / 挑选 / 进度事件）──
    let jobs_n: usize = match val_of("--jobs") {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("❌ --jobs 要写个数，比如 --jobs 2（收到 {s}）");
                return ExitCode::from(code::USAGE);
            }
        },
        None => 1,
    };
    let selection = match val_of("--select") {
        Some(expr) => match yshjy::select::Selection::parse(&expr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("❌ --select 看不懂：{expr}\n   （{e}）");
                eprintln!("   例子：--select \"*.exe\" / --select \"游戏数据/**\" / --select 1,3 / --select required");
                return ExitCode::from(code::USAGE);
            }
        },
        None => yshjy::select::Selection::all(),
    };

    do_extract(
        &targets,
        &val_of("--dest"),
        &val_of("--password"),
        quiet,
        jobs_n,
        selection,
        json,
        !has("--no-smart-dir"),
    )
}

/// 长选项对应的短选项（用于参数识别）
fn short_of(long: &str) -> &str {
    match long {
        "--dest" => "-o",
        "--password" => "-p",
        "--jobs" => "-j",
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

/// 列内容（v0.6：走引擎的 `preview`，**所有格式**都能列，且支持 `--json`）
///
/// 和 v0.4 的区别：以前只有 ZIP 能列，现在 ZIP / 7z / RAR / 单流都能列；
/// 并且会发出 `PreviewReady` 事件 —— 这就是界面 S3「列出内容 → 勾选」的数据源。
fn do_list(targets: &[String], quiet: bool, json: bool) -> ExitCode {
    use std::sync::Arc;
    use yshjy::ctx::UnpackCtx;
    use yshjy::event::EventSink;

    let mut worst: u8 = code::OK;
    for t in targets {
        let path = PathBuf::from(t);
        let sink: Arc<dyn EventSink> = Arc::new(终端出口::new(quiet, json));
        let ctx = UnpackCtx::new(1, sink);

        match yshjy::preview::preview(&path, &ctx, None) {
            Ok((_target, pv)) => {
                if json {
                    println!("{}", 预览_json(t, &pv));
                } else {
                    打印预览(t, &pv, quiet);
                }
            }
            Err(e) => {
                // 兜底：中央目录被改坏 → 按 local header 重建清单（v0.4 的老本事，不能丢）
                if try_list_by_rescue(t, quiet).is_some() {
                    continue;
                }
                eprintln!("❌ {t}：{}", 失败文案(e.code));
                if !quiet {
                    eprintln!("   （{e}）");
                    eprintln!("   提示：解不开时加 --rescue 试试救援模式。");
                }
                if worst == code::OK {
                    worst = e.code.cli_exit_code().clamp(0, 255) as u8;
                }
            }
        }
    }
    ExitCode::from(worst)
}

/// 预览提示码 → 中文一句话（引擎只发码，文案归这里）
fn 预览提示文案(h: &yshjy::event::PreviewHint) -> String {
    use yshjy::event::PreviewHint as H;
    match h {
        H::VolumesMerged { count } => format!("检测到 {count} 个分卷，解压时会自动拼起来"),
        H::NestedArchive { inner } => format!("里面还套了一层压缩包（{inner}）"),
        H::NeedsPassword { entries } => format!("其中 {entries} 个带锁 —— 解压需要解压码"),
        H::LooksTruncated => "这个包可能没下完（尾部数据缺失）".to_string(),
        H::HiddenAt { offset } => {
            format!("包藏在文件中间（偏移 {offset}），已按真实位置定位")
        }
        H::RebuiltIndex => "索引坏了，清单是按文件头重建的".to_string(),
        H::VideoUnplayable => "封面视频本身有问题，但里面的文件还能解".to_string(),
    }
}

/// 把预览结果打印成人话
fn 打印预览(t: &str, pv: &yshjy::preview::Preview, quiet: bool) {
    if quiet {
        for e in &pv.entries {
            println!("{}", e.path);
        }
        return;
    }

    println!("📦 {t}（{}）", pv.format);
    for h in &pv.hints {
        println!("   ℹ {}", 预览提示文案(h));
    }
    if !pv.complete {
        println!("   ⚠ 这个格式只能列出部分信息（大小未知）");
    }
    if pv.entries.is_empty() {
        println!("   （列不出内容）");
        return;
    }

    println!();
    for e in pv.entries.iter().take(40) {
        let lock = if e.encrypted { "🔒" } else { "  " };
        let icon = if e.is_dir { "📁" } else { "  " };
        let size = if e.size > 0 {
            人类可读(e.size)
        } else {
            "—".to_string()
        };
        let risk = if e.is_risky { " ⚠可执行" } else { "" };
        println!("   {lock}{icon} {:>4}.  {size:>10}  {}{risk}", e.index, e.path);
    }
    if pv.entries.len() > 40 {
        println!("   … 还有 {} 个", pv.entries.len() - 40);
    }

    println!();
    println!(
        "   合计 {} / {} 个文件",
        人类可读(pv.total_size),
        pv.file_count()
    );
    if pv.encrypted_count > 0 {
        println!("   解压码：看旁边的「说明.txt」，或 yshjy -p \"解压码\" \"{t}\"");
    }
    println!("   只解一部分：yshjy --select \"1,3\" \"{t}\"");
}

/// 预览结果 → JSON（给脚本/前端消费）
fn 预览_json(t: &str, pv: &yshjy::preview::Preview) -> String {
    serde_json::json!({
        "input": t,
        "format": pv.format,
        "complete": pv.complete,
        "files": pv.file_count(),
        "total_size": pv.total_size,
        "encrypted": pv.encrypted_count,
        "hints": pv.hints.iter().map(预览提示文案).collect::<Vec<_>>(),
        "entries": pv.entries.iter().map(|e| serde_json::json!({
            "index": e.index,
            "path": e.path,
            "size": e.size,
            "is_dir": e.is_dir,
            "encrypted": e.encrypted,
            "risky": e.is_risky,
        })).collect::<Vec<_>>(),
    })
    .to_string()
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

/// 解压（v0.5：多目标 + 批量 + 事件驱动渲染）
///
/// 编排全在引擎里（`yshjy::job`），这里只干两件事：
/// 把**事件渲染成人话**、把**结论变成退出码**。
fn do_extract(
    targets: &[String],
    dest_opt: &Option<String>,
    password: &Option<String>,
    quiet: bool,
    jobs_n: usize,
    selection: yshjy::select::Selection,
    json: bool,
    smart_dir: bool,
) -> ExitCode {
    use std::sync::Arc;
    use yshjy::batch;
    use yshjy::progress::CancelToken;

    // 目录会递归展开，多文件自动排队（重复的会去掉、大文件先跑）
    let paths: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();
    let expanded = batch::expand_inputs(&paths);
    if expanded.is_empty() {
        eprintln!("❌ 没找到能处理的东西：{}", targets.join(" "));
        return ExitCode::from(code::USAGE);
    }
    if !quiet && !json && expanded.len() > 1 {
        println!("📋 排好 {} 个任务（大的先跑）", expanded.len());
    }

    let jobs = {
        let mut js = batch::make_jobs(&expanded);
        // 多任务且用户指定了 -o：每个包解到自己的子目录，
        // 免得两个包的文件挤在一起互相改名（游戏包撞名会很乱）
        if js.len() > 1
            && let Some(base) = dest_opt
        {
            for j in &mut js {
                j.dest = Some(PathBuf::from(base).join(yshjy::job::output_dir_name(&j.input)));
            }
        }
        js
    };
    let opts = batch::BatchOptions {
        jobs: jobs_n.max(1),
        continue_on_error: true,
        password: password.clone(),
        selection,
        dest: dest_opt.clone().map(PathBuf::from),
        smart_dir,
    };

    let sink: Arc<dyn yshjy::event::EventSink> = Arc::new(终端出口::new(quiet, json));
    let cancel = CancelToken::new();

    // Ctrl-C 接管：把信号转成引擎的「取消」——
    // 好处是取消走的是同一条路（已解出的文件保留、写了一半的删掉、退出码 130），
    // 而不是被信号直接打死留一堆半成品。
    {
        let c = cancel.clone();
        let _ = ctrlc::set_handler(move || {
            if c.is_cancelled() {
                // 第二次 Ctrl-C：用户等不及了，直接退
                eprintln!("\n⏹  再按一次就强制退出");
                std::process::exit(130);
            }
            c.cancel_with("user pressed ctrl-c");
            eprintln!("\n⏹  收到 Ctrl-C —— 正在收尾（已解出的文件会保留，再按一次强制退出）");
        });
    }

    let report = batch::run_batch(jobs, &opts, &sink, &cancel, yshjy::job::run_job);

    if json {
        println!("{}", 汇总_json(&report));
    } else {
        打印汇总(&report, quiet);
    }
    ExitCode::from(report.exit_code().clamp(0, 255) as u8)
}

/// 把引擎事件渲染成终端上的人话（引擎只发码和数字，文案归这里）
struct 终端出口 {
    quiet: bool,
    json: bool,
    /// stderr 是不是终端：不是（管道/重定向）就不画进度条，免得刷出一堆 \r
    tty: bool,
    last_draw: std::sync::Mutex<std::time::Instant>,
}

impl 终端出口 {
    fn new(quiet: bool, json: bool) -> Self {
        use std::io::IsTerminal;
        Self {
            quiet,
            json,
            tty: std::io::stderr().is_terminal(),
            last_draw: std::sync::Mutex::new(std::time::Instant::now()),
        }
    }

    /// 清掉当前那行进度（**只在终端上做**：管道/日志里不该出现 \r 与转义序列）
    fn 清行(&self) {
        if self.tty {
            eprint!("\r\x1b[K");
        }
    }
}

impl yshjy::event::EventSink for 终端出口 {
    fn emit(&self, ev: &yshjy::event::Event) {
        use yshjy::event::Event;

        // `--json`：原样把事件按行吐出去（脚本/其它前端消费）
        if self.json {
            if let Ok(s) = serde_json::to_string(ev) {
                println!("{s}");
            }
            return;
        }
        if self.quiet {
            return;
        }

        match ev {
            Event::ExtractProgress {
                progress, current, ..
            } => {
                if !self.tty {
                    return; // 管道模式：别画进度条
                }
                // 重画节流：引擎侧已按 100ms 合并，这里再降到 200ms，避免刷屏
                let now = std::time::Instant::now();
                if let Ok(mut last) = self.last_draw.lock() {
                    if now.duration_since(*last) < std::time::Duration::from_millis(200) {
                        return;
                    }
                    *last = now;
                }
                let pct = progress
                    .fraction()
                    .map(|f| format!("{:>5.1}%", f * 100.0))
                    .unwrap_or_else(|| "  …  ".into());
                let what = current.as_deref().unwrap_or("");
                eprint!(
                    "\r   {pct}  {}/{}  {what}\x1b[K",
                    人类可读(progress.done_bytes),
                    人类可读(progress.total_bytes)
                );
            }
            Event::EntryFinished { path, result, .. } if !result.is_ok() => {
                self.清行();
                eprintln!("   ⚠ {} 没解开（{}）", path, result.as_str());
            }
            Event::Diagnostic { code, note, .. } => {
                self.清行();
                eprintln!("   ℹ {}：{note}", 诊断文案(*code));
            }
            Event::PasswordNeeded { attempts, .. } => {
                self.清行();
                eprintln!("   🔒 这个包有密码（已试 {} 次）", attempts);
            }
            Event::JobFinished { .. } => {
                self.清行();
            }
            _ => {}
        }
    }

    /// 批量时进度条会互相覆盖，只保留进度与条目事件
    fn wants_progress(&self) -> bool {
        !self.json
    }
}

/// 诊断码 → 中文一句话（UI-SPEC 的「说人话」原则）
fn 诊断文案(code: yshjy::event::DiagnosticCode) -> &'static str {
    use yshjy::event::DiagnosticCode as D;
    match code {
        D::HiddenAtOffset => "包藏在文件中间，已按真实位置定位",
        D::RebuiltIndex => "索引坏了，已按文件头重建清单",
        D::OffsetCorrected => "索引里的偏移是错的，已在内存里改对（没动你的文件）",
        D::VolumesMerged => "分卷已自动拼起来",
        D::LayoutGuessed => "布局是猜的，不一定准",
        D::VideoUnplayable => "封面视频本身坏了，但里面的文件还能解",
        D::RiskyEntry => "解出了可执行文件，运行前请确认来源",
        D::SlowPath => "走了慢路（全文件扫描）",
        D::PasswordFromNotes => "解压码是从旁边的说明里找到的",
        D::PartialData => "数据不完整",
    }
}

/// 任务结论 → 中文一句话 + 图标
fn 结论文案(o: &yshjy::error::Outcome) -> (String, &'static str) {
    use yshjy::error::Outcome;
    match o {
        Outcome::Success => ("解开".to_string(), "✅"),
        Outcome::Partial { done, total } => (format!("解开 {done}/{total} 个"), "⚠️ "),
        Outcome::Cancelled { done, total } => (format!("已取消（解出 {done}/{total}）"), "⏹ "),
        Outcome::Failed(code) => (失败文案(*code).to_string(), "❌"),
    }
}

/// 错误码 → 中文（UI-SPEC §10 的 CLI 版渲染）
fn 失败文案(code: yshjy::error::ErrorCode) -> &'static str {
    use yshjy::error::ErrorCode as C;
    match code {
        C::PathNotFound => "文件不在这个位置",
        C::InvalidArgument => "参数不对",
        C::NotEnoughSpace => "磁盘空间不够",
        C::PermissionDenied => "这个位置不让写",
        C::NeedPassword => "有密码，但没给解压码",
        C::WrongPassword => "解压码不对",
        C::NotArchive => "既不是压缩包，里面也没藏着压缩包",
        C::Truncated => "文件不完整（常见于下载没下完）",
        C::MissingVolume => "缺分卷",
        C::DataLost => "中间有一段数据丢了",
        C::EncryptedUnbreakable => "加密了，需要发布者给解压码",
        C::VideoUnplayable => "封面视频本身有问题",
        C::Cancelled => "已取消",
        C::Internal => "程序内部出错（这属于 bug，麻烦反馈一下）",
    }
}

/// 打印每个任务的结果
fn 打印汇总(report: &yshjy::batch::BatchReport, quiet: bool) {
    for j in &report.jobs {
        let (txt, icon) = 结论文案(&j.outcome);
        if quiet {
            // 脚本模式：只吐解出来的文件路径
            println!("{}", j.stats.dest.as_deref().unwrap_or(&j.input).display());
            continue;
        }
        println!(
            "{icon} {} —— {txt}（{}，用时 {:.1} 秒）",
            j.input.display(),
            人类可读(j.stats.bytes_written),
            j.stats.millis as f64 / 1000.0
        );
        if let Some(d) = &j.stats.dest {
            println!("     📁 {}", d.display());
        }
        for (name, res) in j.stats.failed.iter().take(5) {
            println!("     ⚠ {name}（{}）", res.as_str());
        }
        if matches!(j.outcome, yshjy::error::Outcome::Failed(_))
            && let Some(note) = &j.note
        {
            println!("     ({note})");
        }
    }

    if report.len() > 1 {
        println!(
            "\n──── 批量完成：成功 {} · 部分 {} · 失败 {} · 取消 {} ────",
            report.succeeded(),
            report.partial(),
            report.failed(),
            report.cancelled()
        );
        for f in report.failures().iter().take(10) {
            let (txt, _) = 结论文案(&f.outcome);
            println!("   ❌ {}：{txt}", f.input.display());
        }
    }
}

/// 汇总成 JSON（脚本消费）
fn 汇总_json(report: &yshjy::batch::BatchReport) -> String {
    let items: Vec<serde_json::Value> = report
        .jobs
        .iter()
        .map(|j| {
            serde_json::json!({
                "input": j.input.display().to_string(),
                "outcome": j.outcome.as_str(),
                "exit_code": j.outcome.exit_code(),
                "done": j.stats.entries_done,
                "total": j.stats.entries_total,
                "bytes": j.stats.bytes_written,
                "dest": j.stats.dest.as_ref().map(|d| d.display().to_string()),
                "failed": j.stats.failed.iter().map(|(n, r)| serde_json::json!({
                    "path": n, "result": r.as_str()
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({
        "engine": yshjy::VERSION,
        "total": report.len(),
        "ok": report.succeeded(),
        "partial": report.partial(),
        "failed": report.failed(),
        "cancelled": report.cancelled(),
        "exit_code": report.exit_code(),
        "jobs": items,
    })
    .to_string()
}

/// 人类可读的体量（1.2 GB / 345 MB）
fn 人类可读(n: u64) -> String {
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;
    let f = n as f64;
    if f >= G {
        format!("{:.2} GB", f / G)
    } else if f >= M {
        format!("{:.1} MB", f / M)
    } else if f >= K {
        format!("{:.0} KB", f / K)
    } else {
        format!("{n} B")
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
