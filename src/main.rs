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

/// 崩溃兜底（panic 时把现场写进 state 目录，并告诉用户怎么报障）
mod crash;
/// 契约服务（`--serve`）：给 GUI / 脚本用的 JSON 管道，见 docs/PROTOCOL.md
mod serve;

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

/// `--version` 的输出（**文字来自语言包**，档位是编译期定死的）
fn 版本信息() -> String {
    let t = 文案();
    let full = cfg!(feature = "full");
    let 档 = t.t(if full { "tier.full" } else { "tier.common" }, &[]);
    let mut s = t.t(
        "version.line",
        &[("version", env!("CARGO_PKG_VERSION")), ("tier", &档)],
    );
    s.push('\n');
    s.push_str(&t.t("version.formats", &[]));
    s.push('\n');
    s.push_str(&t.t(
        if full {
            "version.tier_full"
        } else {
            "version.tier_common"
        },
        &[],
    ));
    s.push('\n');
    s
}

/// 需要跟一个值的选项 —— 收集命令行里的"文件名"时必须跳过它们的值
///
/// 抽成常量而不是散在 `matches!` 里：**漏登记一个就会出真事故**
/// —— `--lang en 包.mp4` 会把 `en` 当成要解压的文件（多语言上线时正好踩到）。
const 带值的选项: &[&str] = &[
    "--dest", "-o", "--password", "-p", "--jobs", "-j", "--select", "--lang",
];

/// 进程级文案仓库
///
/// CLI 一次进程只跑一种语言，用单例最省事。**它放在外壳里** —— 引擎侧保持无全局状态，
/// 这样 GUI / 手机端可以各持一份（比如界面里能实时切换语言）。
static I18N: std::sync::OnceLock<yshjy::i18n::I18n> = std::sync::OnceLock::new();

/// 取进程级文案仓库（`main` 开头已初始化）
fn 文案() -> &'static yshjy::i18n::I18n {
    I18N.get().expect("文案仓库应当在 main 开头初始化")
}

fn main() -> ExitCode {
    // 时区要最先注入：崩溃日志、诊断包、ZIP 条目时间全都靠它显示成"本地时间"
    // （引擎自己碰不了 localtime_r，这一步只能外壳做）
    crash::注入本地时区();
    // 崩溃兜底要**最早装**：后面每一步（读配置、解析语言包、扫目录）都可能炸，
    // 装晚了那几秒里的崩溃就没有现场了。
    crash::装兜底();
    // 顺手清掉上次崩溃 / 被 kill 留下的临时文件（只清 24 小时前的，不误伤正在跑的实例）
    crash::清理临时残骸();

    // 修一个"用起来正常、但报错很吓人"的毛病：
    // Rust 默认会**忽略 SIGPIPE**，于是「管道对端关掉」变成「写 stdout 失败」，
    // 而 `println!` 遇到写失败会 **panic** —— 用户跑
    // `yshjy --list 包.mp4 | head -3` 就会看到一段 panic 回溯（其实一切正常）。
    // 恢复系统默认行为：对端关了管道，我们就安静地结束，像所有 Unix 工具一样。
    #[cfg(unix)]
    {
        // SAFETY: 只是把 SIGPIPE 的处理方式设回系统默认（SIG_DFL），
        // 不涉及任何内存操作，也没有信号处理函数里该避免的事。
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }

    let argv: Vec<String> = std::env::args().collect();
    let quiet = argv.iter().any(|a| a == "-q" || a == "--quiet");

    let val_of = |k: &str| -> Option<String> {
        argv.iter()
            .position(|a| a == k || a == short_of(k))
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let has = |k: &str| argv.iter().any(|a| a == k || a == short_of(k));

    // 配置（读一次就够；文件不在就用默认值 —— 见 config 模块的"读的时候宽容"）
    let cfg = yshjy::config::全局配置();

    // 多语言：`--lang` > 配置文件里的 language > 系统语言（见 i18n::detect）
    let _ = I18N.set(yshjy::i18n::I18n::new(
        val_of("--lang")
            .or_else(|| cfg.general.language.clone())
            .as_deref(),
    ));

    // 契约服务：GUI / 脚本通过它驱动引擎（一行一个 JSON），见 docs/PROTOCOL.md
    if argv.iter().any(|a| a == "--serve") {
        return ExitCode::from(serve::运行());
    }

    // 开发自检用：手动触发一次崩溃，验证「崩溃日志 + 用户提示」这条链路是通的。
    // 放在语言包初始化**之后**，所以它会走真实的多语言文案（而不是兜底那套中英双语）。
    // ⚠️ 只在 debug 构建里存在（release 编译时这段根本不会被编进去），不是后门。
    #[cfg(debug_assertions)]
    if argv.iter().any(|a| a == "--debug-crash") {
        panic!("--debug-crash：手动触发的测试崩溃");
    }
    if !文案().is_requested_ok() {
        // 要的语言没有 → 用兜底语言提示一句（文案本身也走语言包）
        let avail = 文案().available().join(", ");
        eprintln!(
            "{}",
            文案().t(
                "misc.lang_unknown",
                &[
                    ("lang", 文案().requested()),
                    ("fallback", &文案().lang()),
                    ("available", &avail),
                ]
            )
        );
    }

    if argv.len() < 2 {
        eprint!("{}", 文案().t("help.text", &[]));
        return ExitCode::from(code::USAGE);
    }
    if has("--help") || has("-h") {
        print!("{}", 文案().t("help.text", &[]));
        return ExitCode::from(code::OK);
    }
    if has("--version") || has("-V") {
        print!("{}", 版本信息());
        return ExitCode::from(code::OK);
    }

    // 看当前配置（改了哪个文件、每一项现在是什么值）
    if has("--show-config") {
        let 语 = 文案();
        let 路径 = yshjy::config::config_path().display().to_string();
        println!("{}", 语.t("cli.show_config", &[("path", &路径)]));
        for (键, 值) in yshjy::config::配置摘要(cfg) {
            // 两个"空值"要渲染成提示语；密码本那条要拆成「路径 + 条数」两个参数
            let 值 = match 键 {
                "language" if 值.is_empty() => 语.t("config.follow_system", &[]),
                "out_dir" if 值.is_empty() => 语.t("config.beside_package", &[]),
                "passwords" => {
                    let mut it = 值.splitn(2, '|');
                    let p = it.next().unwrap_or_default().to_string();
                    let c = it.next().unwrap_or("0").to_string();
                    语.t("config.passwords_fmt", &[("path", &p), ("count", &c)])
                }
                _ => 值,
            };
            println!(
                "{}",
                语.t(
                    "config.line",
                    &[("key", &语.t(&format!("config.{键}"), &[])), ("value", &值)]
                )
            );
        }
        return ExitCode::from(code::OK);
    }

    // 生成诊断包（报障用：把环境 + 识别结果打包成一个「能直接贴出去」的 zip）
    if has("--report") {
        return do_report(&argv, has("--no-paths"));
    }

    // 导入外部密码本（兼容隐写者的 PW.txt：Tab 取第一列，且不过滤中文）
    if let Some(src) = val_of("--import-passwords") {
        let dst = cfg.passwords_path();
        return match yshjy::config::导入密码本(std::path::Path::new(&src), &dst) {
            Ok(added) => {
                let total = yshjy::config::读密码本(&dst).len();
                println!(
                    "{}",
                    文案().t(
                        "cli.import_done",
                        &[
                            ("added", &added.to_string()),
                            ("total", &total.to_string()),
                            ("path", &dst.display().to_string()),
                        ]
                    )
                );
                ExitCode::from(code::OK)
            }
            Err(e) => {
                eprintln!("❌ {e}");
                ExitCode::from(code::ERR)
            }
        };
    }

    // 收集"非选项参数"（文件名 / 目录 / 子命令）
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if 带值的选项.contains(&a.as_str()) {
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
        eprintln!("{}", 文案().t("cli.no_target", &[]));
        return ExitCode::from(code::USAGE);
    }

    let json = has("--json");
    if has("--list") || has("-l") {
        return do_list(&targets, quiet, json);
    }
    if has("--rescue") {
        return do_rescue(&targets, &val_of("--dest"), quiet);
    }
    // 只剥封面、不解压（DESIGN §3 / §17.5）
    if has("--extract-cover") {
        return do_extract_cover(&targets, quiet);
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
                eprintln!("{}", 文案().t("cli.bad_jobs", &[("value", &s)]));
                return ExitCode::from(code::USAGE);
            }
        },
        None => cfg.general.jobs, // 配置文件里可以定默认并行数
    };
    let selection = match val_of("--select") {
        Some(expr) => match yshjy::select::Selection::parse(&expr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "{}",
                    文案().t("cli.bad_select", &[("expr", &expr), ("reason", &e.to_string())])
                );
                eprintln!("{}", 文案().t("cli.select_examples", &[]));
                return ExitCode::from(code::USAGE);
            }
        },
        None => yshjy::select::Selection::all(),
    };

    do_extract(解压参数 {
        targets,
        // 输出目录优先级：命令行 -o > 配置文件 output.dir > 智能规则
        dest: val_of("--dest").or_else(|| cfg.output_dir().map(|p| p.display().to_string())),
        password: val_of("--password"),
        quiet,
        jobs: jobs_n,
        selection,
        json,
        smart_dir: !has("--no-smart-dir"),
        force: has("--force"),
    })
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
                eprintln!(
                    "{}",
                    文案().t("cli.fail_line", &[("input", t), ("reason", &失败文案(e.code))])
                );
                if !quiet {
                    eprintln!("   （{e}）");
                    if let Some(提示) = 文件类型提示(std::path::Path::new(t)) {
                        eprintln!("   ℹ {提示}");
                    }
                    eprintln!("{}", 文案().t("cli.rescue_hint", &[]));
                }
                if worst == code::OK {
                    worst = e.code.cli_exit_code().clamp(0, 255) as u8;
                }
            }
        }
    }
    ExitCode::from(worst)
}

/// 预览提示码 → 一句话（**引擎只发码，文案归外壳** —— 这里查语言包）
fn 预览提示文案(h: &yshjy::event::PreviewHint) -> String {
    use yshjy::event::PreviewHint as H;
    let t = 文案();
    match h {
        H::VolumesMerged { count } => t.t(
            "preview.hint.volumes_merged",
            &[("count", &count.to_string())],
        ),
        H::NestedArchive { inner } => t.t("preview.hint.nested_archive", &[("inner", inner)]),
        H::NeedsPassword { entries } => t.t(
            "preview.hint.needs_password",
            &[("count", &entries.to_string())],
        ),
        H::LooksTruncated => t.t("preview.hint.looks_truncated", &[]),
        H::HiddenAt { offset } => t.t(
            "preview.hint.hidden_at",
            &[("offset", &offset.to_string())],
        ),
        H::RebuiltIndex => t.t("preview.hint.rebuilt_index", &[]),
        H::VideoUnplayable => t.t("preview.hint.video_unplayable", &[]),
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

    let 语 = 文案();
    println!(
        "{}",
        语.t("preview.title", &[("input", t), ("format", pv.format)])
    );
    for h in &pv.hints {
        println!("   ℹ {}", 预览提示文案(h));
    }
    if !pv.complete {
        println!("{}", 语.t("preview.incomplete", &[]));
    }
    if pv.entries.is_empty() {
        println!("{}", 语.t("preview.empty", &[]));
        return;
    }

    println!();
    for e in pv.entries.iter().take(40) {
        let lock = if e.encrypted { "🔒" } else { "  " };
        let icon = if e.is_dir { "📁" } else { "  " };
        let size = if e.size > 0 {
            人类可读(e.size)
        } else {
            语.t("preview.unknown_size", &[])
        };
        let risk = if e.is_risky {
            语.t("preview.risky", &[])
        } else {
            String::new()
        };
        println!(
            "{}",
            语.t(
                "preview.row",
                &[
                    ("lock", lock),
                    ("icon", icon),
                    ("index", &e.index.to_string()),
                    ("size", &size),
                    ("path", &e.path),
                    ("risk", &risk),
                ]
            )
        );
    }
    if pv.entries.len() > 40 {
        println!(
            "{}",
            语.t(
                "preview.row_more",
                &[("count", &(pv.entries.len() - 40).to_string())]
            )
        );
    }

    println!();
    println!(
        "{}",
        语.t(
            "preview.total",
            &[
                ("size", &人类可读(pv.total_size)),
                ("count", &pv.file_count().to_string()),
            ]
        )
    );
    if pv.encrypted_count > 0 {
        println!("{}", 语.t("preview.password_hint", &[("input", t)]));
    }
    println!("{}", 语.t("preview.select_hint", &[("input", t)]));
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

/// 打印救援模式重建出来的清单（`--list` 的兜底路径与 `--rescue` 共用）
///
/// 为什么抽出来：这段以前在两处各写一遍，都是 `"… {size:>14} 字节 …"` +
/// `"合计 {:.2} GB"` —— 单位写死中文、体量一律 GB（3 字节也显示 0.00 GB），
/// 而且翻译不了。v0.9 第 10 批收成一份：体量走 [`人类可读`]，文案走语言包。
fn 打印救援清单(
    items: &[yshjy::rescue::LocalEntry],
    quiet: bool,
    标题键: &str,
    带提示: bool,
    path: &str,
) {
    let 语 = 文案();
    if quiet {
        for it in items {
            println!("{}", it.path);
        }
        return;
    }
    println!(
        "{}",
        语.t(标题键, &[("count", &items.len().to_string())])
    );
    let mut total = 0u64;
    for it in items.iter().take(50) {
        let lock = if it.encrypted() { "🔒" } else { "  " };
        println!(
            "{}",
            语.t(
                "cli.rescue_item",
                &[
                    ("lock", lock),
                    ("size", &人类可读(it.pack_size)),
                    ("path", &it.path),
                ]
            )
        );
        total += it.pack_size;
    }
    if items.len() > 50 {
        println!(
            "{}",
            语.t("cli.rescue_more", &[("count", &(items.len() - 50).to_string())])
        );
    }
    println!(
        "{}",
        语.t(
            "cli.rescue_total",
            &[("size", &人类可读(total)), ("count", &items.len().to_string())]
        )
    );
    if 带提示 {
        println!("{}", 语.t("cli.rescue_tip", &[("path", path)]));
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
    打印救援清单(&items, quiet, "cli.rescue_rebuilt_header", true, path);
    Some(ExitCode::from(code::OK))
}

/// 解压（v0.5：多目标 + 批量 + 事件驱动渲染）
///
/// 编排全在引擎里（`yshjy::job`），这里只干两件事：
/// 把**事件渲染成人话**、把**结论变成退出码**。
/// 解压命令的全部参数
///
/// 打包成一个结构体而不是往函数签名里塞 9 个参数 —— clippy 的 `too_many_arguments`
/// 拦过一次，这是好事：参数一长串时，调用点极易传错顺序（而 bool 传错了还不报错）。
struct 解压参数 {
    targets: Vec<String>,
    dest: Option<String>,
    password: Option<String>,
    quiet: bool,
    jobs: usize,
    selection: yshjy::select::Selection,
    json: bool,
    smart_dir: bool,
    force: bool,
}

fn do_extract(a: 解压参数) -> ExitCode {
    let cfg = yshjy::config::全局配置();
    let 解压参数 {
        targets,
        dest: dest_opt,
        password,
        quiet,
        jobs: jobs_n,
        selection,
        json,
        smart_dir,
        force,
    } = a;
    use std::sync::Arc;
    use yshjy::batch;
    use yshjy::progress::CancelToken;

    // 目录会递归展开，多文件自动排队（重复的会去掉、大文件先跑）
    let paths: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();
    let expanded = batch::expand_inputs(&paths);
    if expanded.is_empty() {
        eprintln!(
            "{}",
            文案().t("cli.nothing_found", &[("targets", &targets.join(" "))])
        );
        // 说清楚"每个输入到底是什么"—— 空文件夹 / 空文件 / 视频，都比一句
        // "没找到能处理的东西"有用
        for t in &targets {
            if let Some(提示) = 文件类型提示(std::path::Path::new(t)) {
                eprintln!("   ℹ {提示}");
            }
        }
        return ExitCode::from(code::USAGE);
    }
    if !quiet && !json && expanded.len() > 1 {
        println!(
            "{}",
            文案().t("cli.queued", &[("count", &expanded.len().to_string())])
        );
    }

    let jobs = {
        let mut js = batch::make_jobs(&expanded);
        // 多任务且用户指定了 -o：每个包解到自己的子目录，
        // 免得两个包的文件挤在一起互相改名（游戏包撞名会很乱）
        if js.len() > 1
            && let Some(base) = &dest_opt
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
        force,
        limits: yshjy::safety::Limits {
            max_ratio: cfg.safety.zip_bomb_ratio,
            warn_executables: cfg.safety.warn_executables,
            ..Default::default()
        },
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
                eprintln!("\n{}", 文案().t("cli.interrupted_again", &[]));
                std::process::exit(130);
            }
            c.cancel_with("user pressed ctrl-c");
            eprintln!("\n{}", 文案().t("cli.interrupted", &[]));
        });
    }

    let report = batch::run_batch(jobs, &opts, &sink, &cancel, |job, opts, sink, cancel| {
        // **一个坏包不该带走整批**：解压库碰到畸形数据时有可能 panic（自家 fuzz
        // 2800 万次没抓到，但第三方库的 panic 不该让用户来承受）。这里只隔离「单个任务」；
        // main 自己的 bug 仍然是致命的 —— 带着不确定状态继续跑，可能写出坏文件。
        let _隔离 = crash::隔离标记::进入();
        let 跑 = || yshjy::job::run_job(job, opts, sink, cancel);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(跑)) {
            Ok(o) => o,
            Err(_) => batch::JobOutcome::minimal(
                job.id,
                &job.input,
                yshjy::error::Outcome::Failed(yshjy::error::ErrorCode::Internal),
            ),
        }
    });

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
                // 「走了慢路」是性能诊断，跟用户无关；只有排查问题时才要
                // （想看就 --json 拿事件流，那里是完整的）
                if matches!(code, yshjy::event::DiagnosticCode::SlowPath) {
                    return;
                }
                self.清行();
                eprintln!(
                    "{}",
                    文案().t(
                        "cli.diag_line",
                        &[("text", &诊断文案(*code)), ("detail", note)]
                    )
                );
            }
            Event::PasswordNeeded { attempts, .. } => {
                self.清行();
                eprintln!(
                    "{}",
                    文案().t("cli.password_tried", &[("count", &attempts.to_string())])
                );
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
fn 诊断文案(code: yshjy::event::DiagnosticCode) -> String {
    use yshjy::event::DiagnosticCode as D;
    let key = match code {
        D::VolumesMerged => "diag.volumes_merged",
        D::LayoutGuessed => "diag.layout_guessed",
        D::VideoUnplayable => "diag.video_unplayable",
        D::RiskyEntry => "diag.risky_entry",
        D::SlowPath => "diag.slow_path",
        D::PasswordFromNotes => "diag.password_from_notes",
        D::PartialData => "diag.partial_data",
        D::HiddenAtOffset => "diag.hidden_at",
        D::RebuiltIndex => "diag.rebuilt_index",
        D::OffsetCorrected => "diag.offset_corrected",
    };
    文案().t(key, &[])
}

/// 任务结论 → 中文一句话 + 图标
fn 结论文案(o: &yshjy::error::Outcome) -> (String, &'static str) {
    use yshjy::error::Outcome;
    match o {
        Outcome::Success => (文案().t("result.phrase.ok", &[]), "✅"),
        Outcome::Partial { done, total } => (
            文案().t(
                "result.phrase.partial",
                &[("done", &done.to_string()), ("total", &total.to_string())],
            ),
            "⚠️ ",
        ),
        Outcome::Cancelled { done, total } => (
            文案().t(
                "result.phrase.cancelled",
                &[("done", &done.to_string()), ("total", &total.to_string())],
            ),
            "⏹ ",
        ),
        Outcome::Failed(code) => (失败文案(*code), "❌"),
    }
}

/// 错误码 → 文案（**走语言包**，UI-SPEC §10 的 CLI 版渲染）
///
/// 引擎只发码，句子在这一层拼 —— 换语言不用动引擎一行代码。
fn 失败文案(code: yshjy::error::ErrorCode) -> String {
    use yshjy::error::ErrorCode as C;
    let key = match code {
        C::PathNotFound => "error.path_not_found",
        C::InvalidArgument => "error.invalid_argument",
        C::NotEnoughSpace => "error.no_space",
        C::PermissionDenied => "error.permission_denied",
        C::NeedPassword => "error.password_missing",
        C::WrongPassword => "error.password_wrong",
        C::NotArchive => "error.not_archive",
        C::Truncated => "error.truncated",
        C::MissingVolume => "error.missing_volume",
        C::DataLost => "error.data_lost",
        C::EncryptedUnbreakable => "error.encrypted_unbreakable",
        C::VideoUnplayable => "error.video_unplayable",
        C::Cancelled => "error.cancel",
        C::Internal => "error.internal",
    };
    文案().t(key, &[])
}


/// 文件类型 → 人话（CLI 版；与 `失败文案` 同一层，将来一起挪进语言包）
fn 文件类型名(kind: &str) -> &'static str {
    match kind {
        "mp4" => "MP4 视频",
        "mov" => "MOV 视频",
        "mkv" => "MKV 视频",
        "webm" => "WebM 视频",
        "avi" => "AVI 视频",
        "flv" => "FLV 视频",
        "ts" => "MPEG-TS 视频流",
        "png" => "PNG 图片",
        "jpeg" => "JPEG 图片",
        "gif" => "GIF 图片",
        "webp" => "WebP 图片",
        "bmp" => "BMP 图片",
        "tiff" => "TIFF 图片",
        "mp3" => "MP3 音频",
        "flac" => "FLAC 音频",
        "ogg" => "OGG 音频",
        "wav" => "WAV 音频",
        "pdf" => "PDF 文档",
        "elf" => "Linux 可执行程序",
        "exe" => "Windows 可执行程序",
        "zip" => "ZIP 压缩包",
        "7z" => "7z 压缩包",
        "rar" => "RAR 压缩包",
        "gzip" => "gzip 压缩流",
        "xz" => "xz 压缩流",
        "zstd" => "zstd 压缩流",
        "bzip2" => "bzip2 压缩流",
        "lz4" => "LZ4 压缩流",
        "text" => "文本文件",
        _ => "认不出的文件",
    }
}

/// 失败时补一句「这个文件其实是……」
///
/// 为什么值得单独查一遍文件头：定位失败时用户最想知道的是
/// "我给你的这个到底是啥"，而不是"不是压缩包"。尤其**名字与内容不符**时 ——
/// 那正是伪装包的特征（`.mp4` 里其实是 ZIP），必须点出来。
fn 文件类型提示(path: &std::path::Path) -> Option<String> {
    use yshjy::probe;
    if path.is_dir() {
        return Some("这是一个文件夹 —— 里面没有能找到的压缩包".into());
    }
    // 空文件单独说：这是最常见的"下载失败"痕迹，比"认不出类型"有用得多
    if let Ok(m) = std::fs::metadata(path)
        && m.len() == 0
    {
        return Some("这是一个空文件（0 字节）—— 一般是下载失败留下的空壳，重新下一次吧".into());
    }
    let p = probe::probe_path(path).ok()?;
    let 大小 = std::fs::metadata(path)
        .map(|m| 人类可读(m.len()))
        .unwrap_or_default();
    let 类型 = 文件类型名(p.kind.as_str());
    let 扩展名 = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // 名字和内容对不上 —— 伪装包的典型特征，优先说这个
    if !扩展名.is_empty()
        && let Some(名类别) = probe::extension_category(&扩展名)
    {
        let 实类别 = p.kind.category();
        if 名类别 != 实类别 {
            let 名 = format!(".{扩展名}");
            return Some(if 实类别 == "archive" {
                format!("名字写的是{名}，内容其实是{类型}（{大小}）—— 这很可能就是伪装包，用 --rescue 再试一次")
            } else if 实类别 == "program" {
                format!("⚠ 名字写的是{名}，内容其实是{类型}（{大小}）—— 来路不明的程序别运行")
            } else {
                format!("名字写的是{名}，内容其实是{类型}（{大小}）—— 可能被改过名")
            });
        }
    }
    if p.kind == probe::FileKind::Unknown {
        return Some(format!("认不出这是什么文件（{大小}）—— 可能被加密过，或者文件头坏了"));
    }
    if p.kind.is_executable() {
        return Some(format!("⚠ 这个文件其实是{类型}（{大小}）—— 来路不明的程序别运行"));
    }
    Some(format!("这个文件看起来是：{类型}（{大小}）"))
}

/// 打印每个任务的结果
///
/// **失败项不在这个循环里逐条打**：目录里 50 个文件只有 2 个是包时，
/// 逐条打会刷出几十行"这不是压缩包"，用户根本看不见成功的那两个。
/// 失败统一到下面「按原因合并」，同一种原因只占一行。
fn 打印汇总(report: &yshjy::batch::BatchReport, quiet: bool) {
    use yshjy::error::Outcome;

    for j in &report.jobs {
        if matches!(j.outcome, Outcome::Failed(_)) {
            continue; // 留给后面的合并汇报
        }
        let (txt, icon) = 结论文案(&j.outcome);
        if quiet {
            println!("{}", j.stats.dest.as_deref().unwrap_or(&j.input).display());
            continue;
        }
        let 耗时 = format!("{:.1}", j.stats.millis as f64 / 1000.0);
        let 大小 = 人类可读(j.stats.bytes_written);
        println!(
            "{}",
            文案().t(
                "result.line",
                &[
                    ("icon", icon),
                    ("input", &j.input.display().to_string()),
                    ("text", &txt),
                    ("size", &大小),
                    ("secs", &耗时),
                ]
            )
        );
        if let Some(d) = &j.stats.dest {
            println!("     📁 {}", d.display());
        }
        for (name, res) in j.stats.failed.iter().take(5) {
            println!("     ⚠ {name}（{}）", res.as_str());
        }
    }

    let 失败: Vec<&yshjy::batch::JobOutcome> = report
        .jobs
        .iter()
        .filter(|j| matches!(j.outcome, Outcome::Failed(_)))
        .collect();

    if report.len() > 1 {
        println!(
            "\n──── 批量完成：成功 {} · 部分 {} · 失败 {} · 取消 {} ────",
            report.succeeded(),
            report.partial(),
            report.failed(),
            report.cancelled()
        );
        // 同原因合并：20 个 png → 一行「20 个文件：这不是压缩包」
        let mut 按原因: Vec<(String, Vec<&yshjy::batch::JobOutcome>)> = Vec::new();
        for j in &失败 {
            let 原因 = 结论文案(&j.outcome).0;
            match 按原因.iter_mut().find(|(r, _)| *r == 原因) {
                Some((_, v)) => v.push(j),
                None => 按原因.push((原因, vec![*j])),
            }
        }
        for (原因, 项) in 按原因 {
            if 项.len() == 1 {
                println!("   ❌ {}：{原因}", 项[0].input.display());
                if let Some(提示) = 文件类型提示(&项[0].input) {
                    println!("      ℹ {提示}");
                }
            } else {
                println!("   ❌ {} 个文件：{原因}", 项.len());
                for j in 项.iter().take(3) {
                    let 名 = j
                        .input
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    println!("        · {名}");
                }
                if 项.len() > 3 {
                    println!("        … 还有 {} 个", 项.len() - 3);
                }
            }
        }
    } else if let Some(j) = 失败.first() {
        // 单任务：先把「❌ 文件 —— 原因」那行打出来（循环里为了合并汇报跳过了失败项）
        let (txt, icon) = 结论文案(&j.outcome);
        println!(
            "{}",
            文案().t(
                "result.line",
                &[
                    ("icon", icon),
                    ("input", &j.input.display().to_string()),
                    ("text", &txt),
                    ("size", &人类可读(j.stats.bytes_written)),
                    ("secs", &format!("{:.1}", j.stats.millis as f64 / 1000.0)),
                ]
            )
        );
        // 再说清楚"你给的是啥"
        if let Some(提示) = 文件类型提示(&j.input) {
            println!("   ℹ {提示}");
        }
        if let Some(note) = &j.note {
            println!("     ({note})");
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
    // 文案全走语言包（引擎只给结构化数据）。v0.9 第 10 批从 `PackReport::summary()`
    // 搬过来的 —— 那句以前在引擎里拼死中文，换个界面语言也变不过来。
    let 语 = 文案();
    let inputs: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();
    for i in &inputs {
        if !i.exists() {
            eprintln!(
                "{}",
                语.t("cli.pack_missing", &[("path", &i.display().to_string())])
            );
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

    // 输出扩展名决定封面素材：.png → 内置 PNG、.jpg → 内置 JPEG、.mp4 → 内置小视频。
    // 以前这里**从没传过** cover_ext，一律用小视频 —— 于是 `ys -o 图.png` 产出的
    // 是「MP4 头 + .png 后缀」的假图片，看图软件一认魔数就露馅（v0.9 修）。
    let 扩展名 = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    let 素材 = yshjy::pack::cover::BuiltinCover::for_extension(&扩展名);

    let mut opts = yshjy::pack::PackOptions {
        inputs: inputs.clone(),
        password: password.clone(),
        cover_ext: 扩展名,
        ..Default::default()
    };
    // 只有 MP4 才有 `free` 原子（ISO 盒子）；图片/文本用它会直接把封面写坏
    if tail || !素材.is_video() {
        opts.layout = yshjy::pack::PackLayout::TailAppend;
    }

    if !quiet {
        println!(
            "{}",
            语.t(
                "cli.pack_start",
                &[
                    ("count", &inputs.len().to_string()),
                    ("out", &out.display().to_string())
                ]
            )
        );
        if let Some(pw) = password {
            println!("{}", 语.t("cli.pack_locked", &[("mask", &mask(pw))]));
        }
    }

    let f = match std::fs::File::create(&out) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}", 语.t("cli.pack_create_failed", &[("err", &e.to_string())]));
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
                let 布局 = match rep.layout {
                    yshjy::pack::PackLayout::TailAppend => 语.t("cli.pack_layout_tail", &[]),
                    yshjy::pack::PackLayout::FreeAtom => 语.t("cli.pack_layout_free", &[]),
                };
                println!(
                    "{}",
                    语.t(
                        "cli.pack_done",
                        &[
                            ("total", &人类可读(rep.total_size)),
                            ("cover", &人类可读(rep.cover_size)),
                            ("zip", &人类可读(rep.zip_size)),
                            ("entries", &rep.entries.to_string()),
                            ("layout", &布局),
                        ]
                    )
                );
                for w in &rep.warnings {
                    println!("{}", 语.t("cli.pack_warn", &[("warn", w)]));
                }
            }
            // 自检 —— 不能"以为成功就完事"
            match yshjy::pack::self_check(&out, rep.entries) {
                Ok(warns) if warns.is_empty() => {
                    if !quiet {
                        println!("{}", 语.t("cli.pack_selfcheck_ok", &[]));
                    }
                    ExitCode::from(code::OK)
                }
                Ok(warns) => {
                    for w in warns {
                        eprintln!("{}", 语.t("cli.pack_warn", &[("warn", &w)]));
                    }
                    ExitCode::from(code::PARTIAL)
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        语.t("cli.pack_selfcheck_bad", &[("err", &e.to_string())])
                    );
                    ExitCode::from(code::ERR)
                }
            }
        }
        Err(e) => {
            eprintln!("{}", 语.t("cli.pack_failed", &[("err", &e.to_string())]));
            ExitCode::from(code::ERR)
        }
    }
}

/// `--report [包]`：生成诊断包（报障用）
///
/// 里面**不含**文件内容与任何密码（命令行里的密码值会被抹成 `<已抹掉>`）；
/// 路径默认包含，加 `--no-paths` 就换成 `<path-1>` 这类占位符。
/// 详细的红线见 `src/report.rs` 模块头。
fn do_report(argv: &[String], hide_paths: bool) -> ExitCode {
    let 语 = 文案();
    // 位置参数里第一个**真正的**位置参数（选项本身、以及"带值选项的值"都要跳过）
    //
    // ⚠️ 实测踩到过：`yshjy --report -p 密码 包.mp4` 里，`-p` 的**值**（密码）
    //    也是个"非选项参数"，于是被当成了要分析的文件 —— findings.json 里的
    //    target 变成了那串密码。
    let 目标 = 找位置参数(argv, 带值的选项);
    let opts = yshjy::report::ReportOptions {
        target: 目标,
        hide_paths,
        out_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        argv: argv.to_vec(),
    };
    let ctx = yshjy::ctx::UnpackCtx::quiet(1);
    match yshjy::report::生成(&opts, &ctx) {
        Ok(路径) => {
            println!(
                "{}",
                语.t("cli.report_done", &[("path", &路径.display().to_string())])
            );
            ExitCode::from(code::OK)
        }
        Err(e) => {
            eprintln!("{}", 语.t("cli.report_failed", &[("err", &e.to_string())]));
            ExitCode::from(code::ERR)
        }
    }
}

/// 从参数里挑出第一个**真正的**位置参数
///
/// 必须同时跳过两类东西：① 选项本身（`--report` / `-q` …）
/// ② **带值选项的值**（`-p 密码` 里的密码、`--lang en` 里的 en）——
/// 少跳过第二类就会把选项的值当成文件名（实测踩过：target 变成了密码）。
fn 找位置参数(argv: &[String], 带值: &[&str]) -> Option<PathBuf> {
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if 带值.contains(&a.as_str()) {
            i += 2; // 跳过选项本身 + 它的值
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(PathBuf::from(a));
    }
    None
}

/// `--extract-cover`：只把伪装包里的封面区切出来，不解压（DESIGN §3 / §17.5）
///
/// 封面区 = `[0, 压缩层起点)`。起点由定位阶段算出（ZIP 的 `Target::Zip.base`），
/// 所以这里**不需要重新扫一遍文件**。
fn do_extract_cover(targets: &[String], quiet: bool) -> ExitCode {
    use std::sync::Arc;

    let mut 失败 = false;
    for t in targets {
        let p = std::path::Path::new(t);
        let sink: Arc<dyn yshjy::event::EventSink> = Arc::new(yshjy::event::NullSink);
        let ctx = yshjy::ctx::UnpackCtx::new(0, sink);

        match yshjy::job::identify(p, &ctx) {
            Ok(yshjy::job::Target::Zip { base, .. }) => {
                match yshjy::cover_extract::extract_zip_cover(p, base) {
                    Ok(Some(c)) => {
                        if quiet {
                            println!("{}", c.path.display());
                        } else {
                            let kind = match c.kind {
                                "video" => "视频",
                                "image" => "图片",
                                _ => "未知类型",
                            };
                            println!("🎬 {} —— 封面已分离（{kind}）", c.path.display());
                            println!("     {}，原包未改动", 人类可读(c.size));
                        }
                    }
                    Ok(None) => {
                        eprintln!("{t} —— 压缩层就在文件开头，没有封面区可切");
                        失败 = true;
                    }
                    Err(e) => {
                        eprintln!("{t} —— 切封面失败: {e}");
                        失败 = true;
                    }
                }
            }
            Ok(_) => {
                eprintln!("{t} —— 目前只支持 ZIP 伪装包（7z/RAR 的压缩层起点还没接）");
                失败 = true;
            }
            Err(e) => {
                eprintln!("{t} —— 认不出伪装包: {e}");
                失败 = true;
            }
        }
    }
    if 失败 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn do_rescue(targets: &[String], dest_opt: &Option<String>, quiet: bool) -> ExitCode {
    let 语 = 文案();
    let path = &targets[0];
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "{}",
                语.t("cli.rescue_cannot_open", &[("path", path), ("err", &e.to_string())])
            );
            return ExitCode::from(code::ERR);
        }
    };
    let size = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!(
                "{}",
                语.t("cli.rescue_no_meta", &[("err", &e.to_string())])
            );
            return ExitCode::from(code::ERR);
        }
    };

    if !quiet {
        println!("{}", 语.t("cli.rescue_start", &[]));
    }
    // 救援路径也接上事件通道（v0.9 第 10 批）：
    // 「解出了可执行文件」这类安全提醒只有**走事件**才到得了用户眼前
    // （塞进内部 warnings 等于没提醒）。--json 目前只覆盖解压/预览，
    // 救援主路径没有 json 出口，所以这里固定传 false。
    let 出口: std::sync::Arc<dyn yshjy::event::EventSink> =
        std::sync::Arc::new(终端出口::new(quiet, false));
    let ctx = yshjy::ctx::UnpackCtx::new(1, 出口);
    let hits = match yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::strict())
    {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "{}",
                语.t("cli.rescue_scan_failed", &[("err", &e.to_string())])
            );
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
            eprintln!("{}", 语.t("cli.rescue_not_found", &[]));
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
            打印救援清单(&items, quiet, "cli.rescue_rebuilt_ok", false, path);

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
                println!(
                    "{}",
                    语.t("cli.rescue_extract_to", &[("dest", &dest.display().to_string())])
                );
            }
            match yshjy::rescue::extract_by_locals_ctx(&mut f, start, size, &eopts, &ctx) {
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
                    eprintln!(
                        "{}",
                        语.t("cli.rescue_extract_failed", &[("err", &e.to_string())])
                    );
                    ExitCode::from(code::ERR)
                }
            }
        }
        Err(e) => {
            eprintln!(
                "{}",
                语.t("cli.rescue_failed", &[("err", &e.to_string())])
            );
            ExitCode::from(code::ERR)
        }
    }
}
