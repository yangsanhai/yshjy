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

/// 各格式的能力表（阶段 1 梯队三·第 3 项）
///
/// 由来：`Caps` 以前只是引擎内部的说法（`InArchive::caps()`，连单测都在守着
/// 「7z 还原不了时间戳别写 true」），但**界面完全拿不到** —— 界面只能硬写「都支持」，
/// 用户点到不支持的格式才撞墙。现在这个表能查了，界面按位显示就行。
///
/// `--json` 给外壳消费，不带就是给人看的表。
fn 打印各格式能力(json: bool) -> ExitCode {
    use yshjy::layout::ArchiveFormat;
    use yshjy::unpack::{self, Shape};

    // 组合清单与能力位都在引擎里（`unpack::能力组合` / `能力行`）—— 协议 `hello.formats`
    // 读的是同一份，这里只负责**显示名**（展示文案归外壳，引擎不产句子）。
    let 名 = |f: ArchiveFormat, s: Shape| -> String {
        match (f, s) {
            (ArchiveFormat::Zip, Shape::Volumes) => format!("{}（分卷）", f.as_str()),
            _ => f.as_str().to_string(),
        }
    };

    if json {
        let mut 行: Vec<String> = Vec::new();
        for (fmt, shape, c) in unpack::能力行() {
            行.push(format!(
                "{{\"format\":\"{}\",\"shape\":\"{}\",\"encrypted\":{},\"encrypted_names\":{},\"split_volumes\":{},\"selective\":{},\"restores_mtime\":{},\"can_create\":{}}}",
                fmt.as_str(), shape.as_str(),
                c.encrypted, c.encrypted_names, c.split_volumes, c.selective, c.restores_mtime, c.can_create
            ));
        }
        println!("[{}]", 行.join(","));
        return ExitCode::from(code::OK);
    }

    // 给人看的表：✓ 支持 / - 不支持
    println!("各格式能力（✓ 支持，- 不支持）");
    println!("格式          密码  密名  分卷  选择  时间  打包");
    for (fmt, shape, c) in unpack::能力行() {
        let 名 = 名(fmt, shape);
        let m = |b: bool| if b { "✓" } else { "-" };
        println!(
            "{:<12} {:>4} {:>5} {:>5} {:>5} {:>5} {:>5}",
            名,
            m(c.encrypted),
            m(c.encrypted_names),
            m(c.split_volumes),
            m(c.selective),
            m(c.restores_mtime),
            m(c.can_create)
        );
    }
    ExitCode::from(code::OK)
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

/// CLI 选项表 —— **唯一真源**（`src/cli_spec.rs`）
///
/// 补全脚本与 man 页也从这张表生成，别再手抄副本（`docs/CLI-GUI-PLAN.md` §1）。
mod cli_spec;

/// `--pick`：全屏勾选要解哪些条目（用 dialoguer，不手搓；见模块头注释）
mod pick;

/// 需要跟一个值的选项 —— 收集命令行里的"文件名"时必须跳过它们的值
///
/// **不再手抄**：从 `cli_spec::OPTS` 推导。手抄版实测已经漂过 ——
/// `--import-passwords` 会取值却没登记，与当年 `--lang en 包.mp4` 把 `en`
/// 当成要解压的文件是同一个坑（2026-10-28 三方对账揪出）。
fn 带值的选项() -> &'static [&'static str] {
    static CACHE: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    CACHE.get_or_init(cli_spec::value_flags)
}

/// 不带值的选项 —— 用来拦住拼错的参数
///
/// 以前 `-extract-cover`（少打一个横线）会被静默忽略 → 默认解压照跑，
/// 用户以为在提封面，实际解了整个 5 GB 包（2026-10-17 实测踩到）。
fn 无值选项() -> &'static [&'static str] {
    static CACHE: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    CACHE.get_or_init(cli_spec::switch_flags)
}

/// 进程级文案仓库
///
/// CLI 一次进程只跑一种语言，用单例最省事。**它放在外壳里** —— 引擎侧保持无全局状态，
/// 这样 GUI / 手机端可以各持一份（比如界面里能实时切换语言）。
static I18N: std::sync::OnceLock<yshjy::i18n::I18n> = std::sync::OnceLock::new();

/// 取进程级文案仓库（`main` 开头已初始化）
fn 文案() -> &'static yshjy::i18n::I18n {
    I18N.get().expect("文案仓库应当在 main 开头初始化")
}

// ─────────────────────── Ctrl-C 接管（全进程只装一次）───────────────────────

/// Ctrl-C 的退出码 —— SIGINT 惯例（128 + 2；shell 里被 Ctrl-C 杀掉看到的也是这个数）
const 中断退出码: i32 = 130;

/// 解压阶段的取消令牌（`run_batch` 之前挂上来，见 `装中断接管`）
static 取消令牌: std::sync::OnceLock<yshjy::progress::CancelToken> = std::sync::OnceLock::new();

/// 装 Ctrl-C 接管 —— **全进程只调一次**，且必须早于 `--pick`。
///
/// 为什么非要"只装一次"：`ctrlc` 的 `set_handler` 全进程只允许成功一次，第二次返回
/// `MultipleHandlers`。以前这段装在解压流程里（`let _ =` 把错误吞了），而 `--pick`
/// 也要接管 Ctrl-C —— 两边抢同一个名额：谁先装谁生效，后装的那次**静默失效**。
/// 2026-10-29 改成一处安装、按阶段分派，把那个隐患一起收掉。
///
/// 三个阶段（`ctrlc` 的 handler 跑在自己的专用线程上，不是信号上下文，可以放心用 Rust）：
/// 1. `--pick` 全屏里：`console` 后端把 Ctrl-C 变成 SIGINT，`dialoguer` 的复原代码
///    （写 `\x1b[?25h` 显示光标）**根本没有执行机会** → 终端光标一直不回来，得 `reset`。
///    见 `docs/TODO.md` 那条；这里补上复原再按惯例退 130。
/// 2. 解压阶段：转成引擎的「取消」—— 已解出的文件保留、写了一半的删掉，比硬杀干净。
/// 3. 其余阶段（`--list` / 预览 / 参数还没走完）：直接退 130，与系统默认行为等价
///    （被 SIGINT 杀掉时 shell 看到的就是 130）。
fn 装中断接管() {
    let 结果 = ctrlc::set_handler(|| {
        if pick::中断复原() {
            std::process::exit(中断退出码);
        }
        match 取消令牌.get() {
            Some(c) if c.is_cancelled() => {
                // 第二次 Ctrl-C：用户等不及了，直接退
                eprintln!("\n{}", 文案().t("cli.interrupted_again", &[]));
                std::process::exit(中断退出码);
            }
            Some(c) => {
                c.cancel_with("user pressed ctrl-c");
                eprintln!("\n{}", 文案().t("cli.interrupted", &[]));
            }
            None => std::process::exit(中断退出码),
        }
    });
    if let Err(e) = 结果 {
        // 装不上只会退化成系统默认行为（Ctrl-C 直接杀进程），不该阻塞任何功能
        详细(format!("Ctrl-C 接管没装上（{e}），中断走系统默认行为"));
    }
}

// ───────────────────────── 终端日志与颜色（组 E）─────────────────────────

/// 日志器：`-v` 详细 / `-vv` 调试；`--log <文件>` 同时写文件
///
/// 为什么不用 `log` / `env_logger`：这个项目的依赖是**刻意最小**的
/// （少一个依赖就少一个被投毒的面），而且我们只需要「往 stderr / 文件写一行」。
struct 日志器 {
    级别: u8,
    文件: std::sync::Mutex<Option<std::fs::File>>,
}

static 日志: std::sync::OnceLock<日志器> = std::sync::OnceLock::new();

/// 初始化日志（`-v` / `-vv` / `--log`）
fn 初始化日志(级别: u8, 路径: Option<&str>) -> Result<(), String> {
    let f = match 路径 {
        Some(p) => Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .map_err(|e| format!("日志文件打不开（{p}）：{e}"))?,
        ),
        None => None,
    };
    let _ = 日志.set(日志器 {
        级别,
        文件: std::sync::Mutex::new(f),
    });
    Ok(())
}

fn 记日志(级别: u8, msg: &str) {
    let Some(l) = 日志.get() else { return };
    // 终端只显示到用户要的级别；文件里永远写全（--log 就是给排错用的）
    if 级别 <= l.级别 {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "{msg}");
    }
    if let Ok(mut guard) = l.文件.try_lock()
        && let Some(f) = guard.as_mut()
    {
        use std::io::Write;
        let _ = writeln!(f, "{msg}");
    }
}

/// 详细（`-v`）
fn 详细(msg: impl AsRef<str>) {
    记日志(1, msg.as_ref());
}

/// 调试（`-vv`）
fn 调试(msg: impl AsRef<str>) {
    记日志(2, msg.as_ref());
}

/// `--no-color` / 输出不是终端 → 不上色
static 颜色关闭: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn 设定颜色(开: bool) {
    颜色关闭.store(!开, std::sync::atomic::Ordering::Relaxed);
}

/// 上色（只在终端 + 没关颜色时生效）
fn 上色(s: &str, 码: &str) -> String {
    use std::io::IsTerminal;
    if 颜色关闭.load(std::sync::atomic::Ordering::Relaxed)
        || !std::io::stdout().is_terminal()
    {
        return s.to_string();
    }
    format!("\x1b[{码}m{s}\x1b[0m")
}

/// 红（失败）/ 绿（成功）/ 黄（警告）
const 红: &str = "31";
const 绿: &str = "32";
const 黄: &str = "33";

/// Windows 可用内存（MiB）—— 给引擎解压前的内存预检（W5）
///
/// lib 是 `#![forbid(unsafe_code)]`，碰 `GlobalMemoryStatusEx` 的活归外壳：
/// 启动时把这个函数注册进 `hardware::注册内存探针`，预检每次实时来取。
/// 读不到就返回 `None`（预检降级放行 —— 与「读不到 /proc」同一待遇，
/// 绝不能因为探针失败拦住解压）。
#[cfg(windows)]
fn windows可用内存mib() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: 只写我们自己的 MEMORYSTATUSEX（先填 dwLength），API 不在调用后持有指针；
    // 返回 0 = 失败（典型是结构体长度没填对），按 None 降级。
    unsafe {
        let mut st: MEMORYSTATUSEX = std::mem::zeroed();
        st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut st) == 0 {
            return None;
        }
        Some(st.ullAvailPhys / 1024 / 1024)
    }
}

fn main() -> ExitCode {
    // 时区要最先注入：崩溃日志、诊断包、ZIP 条目时间全都靠它显示成"本地时间"
    // （引擎自己碰不了 localtime_r，这一步只能外壳做）
    crash::注入本地时区();
    // Windows 上 lib 碰不了 GlobalMemoryStatusEx（禁 unsafe），外壳注册探针后
    // 解压前的内存预检才有真实数字 —— 以前永远读不到，防 OOM 的内存闸形同虚设（W5）
    #[cfg(windows)]
    yshjy::hardware::注册内存探针(windows可用内存mib);
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

    // ── 组 E 的 CLI 参数：必须在读配置之前处理 ──
    // ① `--config <文件>`：指定配置文件（跟 `--show-config` 不是一回事）
    if let Some(p) = val_of("--config") {
        yshjy::config::指定配置文件路径(std::path::Path::new(&p));
        if !yshjy::config::指定配置文件存在() {
            // 读配置的原则是「宽容」：文件不在就用默认值，但必须让用户知道
            eprintln!("⚠ --config 指定的文件不存在：{p}（这次用默认配置）");
        }
    }
    // ② `--password-file <文件>`：本次命令的密码本（每行一个，# 注释）
    if let Some(p) = val_of("--password-file") {
        let path = std::path::PathBuf::from(&p);
        if !path.exists() {
            eprintln!("❌ --password-file 指定的文件不存在：{p}");
            return ExitCode::from(code::USAGE);
        }
        let list = yshjy::config::读密码本(&path);
        if list.is_empty() {
            eprintln!("❌ --password-file 里没有可用的密码（空行和 # 注释不算）：{p}");
            return ExitCode::from(code::USAGE);
        }
        yshjy::config::设定额外密码(list);
    }
    // ③ `--no-auto-password`：不自动从说明/密码本找（显式 -p 不受影响）
    if has("--no-auto-password") {
        yshjy::config::关闭自动密码();
    }
    // ③b 只关其中一档（界面 S4 的两个勾选各自对应一条）
    if has("--no-auto-password-notes") {
        yshjy::config::关闭说明文件找();
    }
    if has("--no-auto-password-book") {
        yshjy::config::关闭密码本找();
    }
    // ④ `-v` / `-vv` / `--log` / `--no-color` / `--keep`
    let 日志级别 = if has("-vv") {
        2
    } else if has("-v") || has("--verbose") {
        1
    } else {
        0
    };
    if let Err(e) = 初始化日志(日志级别, val_of("--log").as_deref()) {
        eprintln!("❌ {e}");
        return ExitCode::from(code::USAGE);
    }
    if has("--no-color") {
        设定颜色(false);
    }
    if has("--keep") {
        yshjy::pack::设定保留中间产物(true);
        详细("--keep：打包的中间 ZIP 会保留");
    }

    // 配置（读一次就够；文件不在就用默认值 —— 见 config 模块的"读的时候宽容"）
    let cfg = yshjy::config::全局配置();
    详细(format!(
        "配置：{} 个任务上限 · 密码本 {} 条",
        cfg.general.jobs,
        cfg.passwords().len()
    ));

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

    // Ctrl-C 接管：全进程**只在这里装一次**（`--pick` 与解压两条路共用同一份分派逻辑）
    装中断接管();

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

    // 各格式支持什么 —— 界面据此决定显示/隐藏功能（以前只在引擎内部，外面查不到）
    if has("--caps") {
        return 打印各格式能力(has("--json"));
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

    // 内部：把 CLI 选项表输出成 JSON（`scripts/gen-cli-assets.py` 读它生成补全与 man 页）
    //
    // 为什么走这条路：生成器如果去解析 `src/main.rs` 的源码，正则一漂就错；
    // 让二进制自己吐 JSON，真源永远是那张表。
    if has("--dump-cli-spec") {
        print!("{}", cli_spec::dump_json());
        return ExitCode::from(code::OK);
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
    let mut 选项结束 = false;
    while i < argv.len() {
        let a = &argv[i];
        // `--` 之后一律当文件名（名字本身以 - 开头的文件也能解）
        if 选项结束 {
            positional.push(a.clone());
            i += 1;
            continue;
        }
        if a == "--" {
            选项结束 = true;
            i += 1;
            continue;
        }
        if 带值的选项().contains(&a.as_str()) {
            i += 2; // 跳过它的值
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            if 无值选项().contains(&a.as_str()) {
                i += 1;
                continue;
            }
            // 不认识的选项：**必须报错**，绝不静默忽略。
            // 静默忽略的后果是「拼错参数 → 默认解压照跑」，用户完全察觉不到。
            let 猜 = format!("--{}", a.trim_start_matches('-'));
            let 提示 = if 无值选项().contains(&猜.as_str()) || 带值的选项().contains(&猜.as_str())
            {
                format!("（你是不是想写 {猜}？）")
            } else {
                // 再试一次「少打横线」以外的常见错：`--lst` 之类就只提示看帮助
                String::new()
            };
            eprintln!("❌ 不认识的选项 {a}{提示}");
            eprintln!("   看帮助：yshjy --help");
            return ExitCode::from(code::USAGE);
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

    // 文件名编码（--encoding）：非法值直接拒绝，别让用户以为生效了
    let 编码: Option<String> = match val_of("--encoding") {
        None => None,
        Some(v) => {
            let low = v.to_ascii_lowercase();
            let ok = matches!(
                low.as_str(),
                "auto" | "gbk" | "gb18030" | "big5" | "sjis" | "shift-jis" | "utf-8"
            );
            if !ok {
                eprintln!("--encoding 只认 auto / gbk / big5 / sjis / utf-8（收到 {v}）");
                return ExitCode::from(code::USAGE);
            }
            if low == "auto" { None } else { Some(low) }
        }
    };

    let json = has("--json");
    if has("--list") || has("-l") {
        // 密码必须传下去：7z 头加密（-mhe=on）的包，不给密码连清单都读不出来，
        // 以前这里写死 None，于是 `--list -p 密码 头加密.7z` 永远报「需要解压码」。
        return do_list(&targets, quiet, json, 编码.as_deref(), val_of("--password").as_deref());
    }
    if has("--rescue") {
        return do_rescue(&targets, &val_of("--dest"), quiet);
    }
    // 只剥封面、不解压（DESIGN §3 / §17.5）
    if has("--extract-cover") {
        // `-o` 别名在 `val_of` 里已归一到 `--dest`；封面也是产物，跟着 `-o` 走
        let dest_o = val_of("--dest");
        return do_extract_cover(&targets, dest_o.as_deref().map(std::path::Path::new), quiet);
    }
    if cmd == "ys" {
        // 先把 Option 绑定成变量再取引用：`&val_of(...)` 直接塞进结构体会拿到
        // 临时值的引用，编译器虽然可能放行，但读代码的人会以为它活得更久。
        let dest_o = val_of("--dest");
        let pw_o = val_of("--password");
        let split_o = val_of("--split");
        let layout_o = val_of("--layout");
        let cover_o = val_of("--cover");
        // 归档格式（T17-4）：一给就走普通压缩（不加外壳）
        let format_o = val_of("--format");
        // 压缩级别（P23-4）：不给就用引擎默认（6）；给了就必须是 0-9，越界报错不静默
        let level = match val_of("--level") {
            None => None,
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if n <= 9 => Some(n),
                _ => {
                    eprintln!(
                        "❌ --level 只能是 0-9 的整数（0=仅存储最快，9=压缩最小；收到 {v}）"
                    );
                    return ExitCode::from(code::USAGE);
                }
            },
        };
        // 干扰数据（第三期 B）：段数 / 每段大小 / 是否撒假签名
        let decoy = {
            let count = match val_of("--decoy") {
                None => 0usize,
                Some(v) => match v.trim().parse::<usize>() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("❌ --decoy 只能是整数段数（收到 {v}）");
                        return ExitCode::from(code::USAGE);
                    }
                },
            };
            let size = match val_of("--decoy-size") {
                None => 16 * 1024,
                Some(v) => match yshjy::pack_post::parse_size(&v) {
                    Some(n) if n <= usize::MAX as u64 => n as usize,
                    _ => {
                        eprintln!("❌ --decoy-size 看不懂：{v}（写法如 16K / 1M / 65536）");
                        return ExitCode::from(code::USAGE);
                    }
                },
            };
            yshjy::pack::decoy::DecoyOptions {
                count,
                size,
                signatures: has("--decoy-signatures"),
            }
        };
        // 越界在这里先拦一次：别等到引擎里再报（这样 --help 旁边的错误更好懂）
        if let Err(e) = decoy.validate() {
            eprintln!("❌ {e}");
            return ExitCode::from(code::USAGE);
        }
        if decoy.signatures && !decoy.is_on() {
            eprintln!("❌ --decoy-signatures 要配合 --decoy <段数> 用（单独给没有意义）");
            return ExitCode::from(code::USAGE);
        }
        return do_pack(打包参数 {
            targets: &targets,
            dest_opt: &dest_o,
            password: &pw_o,
            tail: has("--tail"),
            quiet,
            split: &split_o,
            checksum: !has("--no-checksum"),
            // `--format` 走普通压缩：说明书那套（「不是视频是压缩包」）对 tar 没意义，一并关掉
            readme: !has("--no-readme")
                && !has("--sfx")
                && !has("--sfx-linux")
                && format_o.is_none(),
            layout: &layout_o,
            cover: &cover_o,
            format: &format_o,
            // 第 23 批：--sfx-linux 生成 .sh；--sfx 生成 Windows exe
            sfx: if has("--sfx-linux") {
                Some("linux")
            } else if has("--sfx") {
                Some("windows")
            } else {
                None
            },
            level,
            decoy,
            json: has("--json"),
        });
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
    // `--candidate <字节偏移>`：解「第 N 处」（S10 多候选在命令行上的入口）
    //
    // 值必须是数字 —— 写错了直接报 USAGE，绝不「当成没给」静默跑默认那处：
    // 用户点名要第 3 段，程序偷偷解第 1 段，比报错更糟。
    let candidate: Option<u64> = match val_of("--candidate") {
        None => None,
        Some(v) => match v.trim().parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) => {
                eprintln!("{}", 文案().t("cli.bad_candidate", &[("value", &v)]));
                return ExitCode::from(code::USAGE);
            }
        },
    };
    let selection = if has("--pick") {
        // ── --pick：全屏勾选（2026-10-28）──
        // 非交互终端（管道 / 脚本 / cron）直接拒绝，并告诉用户改用 --select ——
        // 全屏勾选在那种环境里只会把管道搞乱（验收标准②）。
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            eprintln!("{}", 文案().t("cli.pick_no_tty", &[]));
            return ExitCode::from(code::USAGE);
        }
        let 目标 = match targets.first() {
            Some(t) => t.clone(),
            None => {
                eprintln!("{}", 文案().t("cli.no_target", &[]));
                return ExitCode::from(code::USAGE);
            }
        };
        match pick::交互勾选(&目标, 编码.as_deref()) {
            Ok(Some(sel)) => sel,
            Ok(None) => {
                // 用户按 Esc / q —— 取消不是错误
                eprintln!("{}", 文案().t("cli.pick_cancelled", &[]));
                return ExitCode::from(code::OK);
            }
            Err(e) => {
                eprintln!("❌ {e}");
                return ExitCode::from(code::ERR);
            }
        }
    } else {
        match val_of("--select") {
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
        }
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
        encoding: 编码.clone(),
        // 原包处置：命令行是「这一次」的口子。两个都写以 --delete-source 为准
        // （更狠的那个），二次确认里会说清楚到底哪个生效。
        verify_only: has("--test"),
        candidate,
        source_action: if has("--delete-source") {
            源包处置::永久删除
        } else if has("--trash-source") {
            源包处置::回收站
        } else {
            源包处置::不动
        },
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
fn do_list(
    targets: &[String],
    quiet: bool,
    json: bool,
    encoding: Option<&str>,
    password: Option<&str>,
) -> ExitCode {
    use std::sync::Arc;
    use yshjy::ctx::UnpackCtx;
    use yshjy::event::EventSink;

    let mut worst: u8 = code::OK;
    for t in targets {
        let path = PathBuf::from(t);
        let sink: Arc<dyn EventSink> = Arc::new(终端出口::new(quiet, json));
        let ctx = UnpackCtx::new(1, sink).with_encoding(encoding.map(str::to_string));

        match yshjy::preview::preview(&path, &ctx, password) {
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

    // 真终端里画成表格；管道 / 重定向保持原来一行一条
    // （脚本靠这个格式解析，`--json` 更是一个字段都没动）
    //
    // 窄于 50 列也不画表格：实测 40 列时路径会被折成「大文件.b / in」这种，
    // 还不如老格式好读。
    use std::io::IsTerminal;
    let 够宽 = crossterm::terminal::size()
        .map(|(w, _)| w >= 50)
        .unwrap_or(false);
    if std::io::stdout().is_terminal() && 够宽 {
        打印预览表格(pv);
    } else {
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

/// `--list` 在真终端里画成表格（中文按显示宽度对齐、窄终端自动折行）
///
/// 只在 `stdout` 是终端时调用 —— 管道 / 重定向仍走原来的一行一条，
/// 免得把脚本解析搞崩（`--json` 更是一个字段都没动）。
/// 列宽自适应来自 comfy-table（MIT）：它用 unicode-width 算中文宽度，
/// README §205/§255-283 的优先级是「显式宽度 → 终端宽度」。
fn 打印预览表格(pv: &yshjy::preview::Preview) {
    use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};

    let 语 = 文案();
    let mut tb = Table::new();
    tb.load_style(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    tb.set_header(vec![
        语.t("preview.th_index", &[]),
        语.t("preview.th_size", &[]),
        语.t("preview.th_lock", &[]),
        语.t("preview.th_path", &[]),
    ]);

    for e in pv.entries.iter().take(40) {
        let size = if e.size > 0 {
            人类可读(e.size)
        } else {
            语.t("preview.unknown_size", &[])
        };
        let mut 路径 = e.path.clone();
        if e.is_dir {
            路径 = format!("📁 {路径}");
        }
        if e.is_risky {
            路径.push_str(&语.t("preview.risky", &[]));
        }
        tb.add_row(vec![
            Cell::new(e.index.to_string()),
            Cell::new(size),
            Cell::new(if e.encrypted { "🔒" } else { "" }),
            Cell::new(路径),
        ]);
    }
    println!("{tb}");
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

/// 解压完怎么处置原包（**命令行这一次**的意图）
///
/// 为什么不复用 `config.output.on_done`：那是长期偏好（界面设置页写进去的），
/// 命令行是「这一条命令就这么干」的临时口子 —— 命令行没写就按配置走，写了以命令行为准。
#[derive(Clone, Copy, PartialEq, Eq)]
enum 源包处置 {
    /// 不动（默认）
    不动,
    /// 移进系统回收站（可还原）
    回收站,
    /// 永久删除（**不可恢复**，必须二次确认）
    永久删除,
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
    /// 文件名编码（--encoding）
    encoding: Option<String>,
    /// 解完怎么处置原包（--trash-source / --delete-source）
    source_action: 源包处置,
    /// 只测不写（--test）：完整解码 + 校验，一个字节都不落盘
    verify_only: bool,
    /// 解「第 N 处」（--candidate）：一个文件里藏着多处归档时，按字节偏移精确挑一处
    candidate: Option<u64>,
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
        encoding,
        source_action,
        verify_only,
        candidate,
    } = a;
    use std::sync::Arc;
    use yshjy::batch;
    use yshjy::progress::CancelToken;

    // 目录会递归展开，多文件自动排队（重复的会去掉、大文件先跑）
    let paths: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();
    let expanded = batch::expand_inputs(&paths);

    // 交互式补密码：终端里遇到加密包**先问一句**，不要直接失败
    // （用户反馈 2026-10-17：「能改成需要密码用户输入吗，直接失败就很难受」）
    //
    // 只在「没给 -p、不是 -q/--json、stdin 是终端」时问 —— 脚本/管道里
    // 绝不能停下来等人，否则批量任务会挂死。
    let mut password = password;
    if password.is_none() && !quiet && !json {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal()
            && let Some(带锁) = 找_带锁的包(&expanded)
        {
            password = 问解压码(&带锁);
        }
    }

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
    // `--candidate` 说的是「**这一个**文件里的第几处」。批量时它没有意义 ——
    // 同一次运行里几个包各自藏着东西，一个偏移不可能同时对得上。
    // 与其猜（比如只对第一个生效、其余的静默走默认），不如直接拒绝。
    // 放在「排好 N 个任务」之前：先拒绝，再谈排队，免得用户以为跑起来了。
    if candidate.is_some() && expanded.len() > 1 {
        eprintln!("{}", 文案().t("cli.candidate_one_file", &[]));
        return ExitCode::from(code::USAGE);
    }
    if !quiet && !json && expanded.len() > 1 {
        println!(
            "{}",
            文案().t("cli.queued", &[("count", &expanded.len().to_string())])
        );
    }

    // --delete-source 是不可恢复的破坏性动作：开工前必须问一句。
    //
    // 非交互（管道 / 脚本 / --quiet / --json）一律拒绝 —— 破坏性动作绝不在没人
    // 看着的场合里静默通过（用户口径：删东西要先问）。想自动化请用 --trash-source
    // （进回收站，能还原）。
    if source_action == 源包处置::永久删除 {
        use std::io::IsTerminal;
        if quiet || json || !std::io::stdin().is_terminal() {
            eprintln!("{}", 文案().t("cli.delete_source_needs_tty", &[]));
            return ExitCode::from(code::USAGE);
        }
        eprintln!(
            "{}",
            文案().t(
                "cli.delete_source_warn",
                &[("count", &expanded.len().to_string())]
            )
        );
        if !问是否继续() {
            eprintln!("{}", 文案().t("cli.delete_source_aborted", &[]));
            return ExitCode::from(code::OK);
        }
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
    // `--test` 与「处置原包」互斥：只测不写时根本没有产物可以对照，
    // 这时候删原包等于让用户丢掉唯一的材料 —— 直接拒绝，不做事后解释。
    if verify_only && source_action != 源包处置::不动 {
        eprintln!(
            "{}",
            文案().t("cli.test_source_conflict", &[])
        );
        return ExitCode::from(code::USAGE);
    }
    if verify_only && !quiet && !json {
        eprintln!("{}", 文案().t("cli.test_mode", &[]));
    }

    let opts = batch::BatchOptions {
        jobs: jobs_n.max(1),
        continue_on_error: true,
        password: password.clone(),
        selection,
        dest: dest_opt.clone().map(PathBuf::from),
        smart_dir,
        force,
        encoding,
        verify_only,
        limits: yshjy::safety::Limits {
            max_ratio: cfg.safety.zip_bomb_ratio,
            warn_executables: cfg.safety.warn_executables,
            ..Default::default()
        },
        // S10「都试试」：界面逐个候选调 `extract` + `candidate`；命令行同一件事走
        // `--candidate <字节偏移>`（非法值在参数解析那里就报 USAGE，绝不静默忽略）。
        candidate_offset: candidate,
    };

    let sink: Arc<dyn yshjy::event::EventSink> = Arc::new(终端出口::new(quiet, json));
    let cancel = CancelToken::new();

    // Ctrl-C 的接管**已经在 `装中断接管()` 里装好**（全进程唯一一次）——
    // 这里只把令牌挂上去，handler 就会在解压阶段把它转成引擎的「取消」：
    // 取消走的是同一条路（已解出的文件保留、写了一半的删掉、退出码 130），
    // 而不是被信号直接打死留一堆半成品。
    取消令牌.set(cancel.clone()).ok();

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

    // 处置原包（--trash-source / --delete-source）：放在打印结果之前，
    // 让用户看到的顺序是「解开了 → 原包已移入回收站」，而不是看完结果才悄悄动文件。
    // --json 模式不打印这句（不能污染 JSON 流）。
    if source_action != 源包处置::不动 {
        let (成, _败) = 处置原包(&report, source_action);
        if !json && 成 > 0 {
            println!(
                "{}",
                文案().t(
                    if source_action == 源包处置::回收站 {
                        "cli.source_trashed"
                    } else {
                        "cli.source_deleted"
                    },
                    &[("count", &成.to_string())]
                )
            );
        }
    }

    if json {
        println!("{}", 汇总_json(&report));
    } else {
        打印汇总(&report, quiet, verify_only);
    }
    ExitCode::from(report.exit_code().clamp(0, 255) as u8)
}

/// 只读索引，找第一个「带锁」的包（不解压、不打印）
///
/// 预览失败（缺分卷 / 索引坏）就当没锁 —— 探测失败不能反过来拦住正常解压。
fn 找_带锁的包(paths: &[PathBuf]) -> Option<PathBuf> {
    use std::sync::Arc;
    use yshjy::ctx::UnpackCtx;
    use yshjy::event::EventSink;

    struct 静默;
    impl EventSink for 静默 {
        fn emit(&self, _ev: &yshjy::event::Event) {}
    }

    for p in paths {
        let sink: Arc<dyn EventSink> = Arc::new(静默);
        let ctx = UnpackCtx::new(1, sink);
        if let Ok((_, pv)) = yshjy::preview::preview(p, &ctx, None)
            && pv.encrypted_count > 0
        {
            return Some(p.clone());
        }
    }
    None
}

/// 处置原包：移回收站 / 永久删除。返回（成功条数, 失败条数）。
///
/// 只动「成功 / 部分成功」的 —— 失败时原包是用户重试的唯一材料（与 `job.rs` 里
/// `on_done == "trash"` 的口径一致）。单个失败不影响退出码：内容已经出来了，
/// 删不掉原包是次要问题，说一句就够，不该让整条命令变红。
fn 处置原包(report: &yshjy::batch::BatchReport, action: 源包处置) -> (usize, usize) {
    use yshjy::error::Outcome;
    let (mut 成, mut 败) = (0usize, 0usize);
    if action == 源包处置::不动 {
        return (0, 0);
    }
    for j in &report.jobs {
        if !matches!(
            &j.outcome,
            Outcome::Success | Outcome::Partial { .. }
        ) {
            continue;
        }
        let 结果: Result<(), String> = match action {
            源包处置::回收站 => yshjy::trash::move_to_trash(&j.input)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            _ => std::fs::remove_file(&j.input).map_err(|e| e.to_string()),
        };
        match 结果 {
            Ok(()) => 成 += 1,
            Err(e) => {
                败 += 1;
                eprintln!(
                    "{}",
                    文案().t(
                        "cli.source_action_failed",
                        &[("input", &j.input.display().to_string()), ("reason", &e)]
                    )
                );
            }
        }
    }
    (成, 败)
}

/// 危险动作前的二次确认：只认 y / yes，回车或别的都当「不要」。
///
/// 为什么不用 dialoguer：CLI 里其它交互（密码、--pick）都是自己写的，这里也只读
/// 一行；非交互场景本来就被上游拦掉了（`IsTerminal`），不值得为此多背一条依赖。
fn 问是否继续() -> bool {
    use std::io::Write;
    eprint!("   继续请输 y（其它任意输入 = 取消）：");
    let _ = std::io::stderr().flush();
    let mut buf = String::new();
    if std::io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    matches!(buf.trim(), "y" | "Y" | "yes" | "YES" | "Yes")
}

/// 问用户要解压码（关闭回显；直接回车 = 放弃）
fn 问解压码(包: &std::path::Path) -> Option<String> {
    let 名 = 包
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| 包.display().to_string());
    eprintln!("🔑 {名} 带锁，需要解压码");
    match 读一行不回显() {
        Some(p) if !p.trim().is_empty() => Some(p.trim().to_string()),
        _ => {
            eprintln!("   （没输入，按「没有解压码」继续）");
            None
        }
    }
}

/// 从终端读一行，**不回显**（解压码不该出现在屏幕上，也不该留在 shell 历史里）
///
/// 以前只有 `#[cfg(unix)]` 的手写 termios 分支关得掉回显，Windows 上落进普通读，
/// 解压码明文显示在屏幕上（审计 W4）。现在统一交给 `dialoguer::Password` ——
/// 它把「关回显 + 退格编辑 + 回车提交」跨平台都处理好了（`--pick` 同族，依赖本来就在）。
fn 读一行不回显() -> Option<String> {
    use crossterm::tty::IsTty;
    // 两种情况走普通读（最坏只是回显关不掉 —— 旧手写 termios 分支的注释同样接受这个降级）：
    // ① 管道喂密码（`echo 码 | yshjy 包`）：不是终端就谈不上回显，必须保住脚本可用；
    // ② stderr 被重定向：dialoguer 只认终端（它往 stderr 画提示），硬上会直接报错
    if !std::io::stdin().is_tty() || !std::io::stderr().is_tty() {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok()?;
        return Some(line.trim_end_matches(['\r', '\n']).to_string());
    }
    use dialoguer::Password;
    // Ctrl-C 在 raw mode 里是普通按键，dialoguer 报 Interrupted → .ok() 把它归入 None ——
    // 与「直接回车」同路（放弃），让流程继续走「没有解压码」
    Password::new()
        .with_prompt("   请输入解压码（输入时不显示；直接回车 = 放弃）")
        .allow_empty_password(true)
        .interact()
        .ok()
}

/// 多任务进度状态（只有 `-j 2` 以上真的并行时才会启用）
///
/// 为什么要有它：手绘版只能画**一行**（`\r` 回去覆盖），两个任务交替 emit 时
/// 后一个会把前一个盖掉 —— 这正是 TODO 里「-j 2 时两行互相覆盖」那条。
/// 做法是等**真的出现第二个任务**才切到 indicatif 的 `MultiProgress`：
/// 单任务仍然走原来那条已经测过 7 条单测的路径，不冒险。
struct 多任务状态 {
    /// 见过的任务号 —— 凑到 2 个才说明是批量/并行
    见过: std::collections::HashSet<u64>,
    /// 每个任务一条进度行
    条: std::collections::HashMap<u64, indicatif::ProgressBar>,
    /// indicatif 的多行管理（延迟创建）
    多行: Option<indicatif::MultiProgress>,
}

impl 多任务状态 {
    fn new() -> Self {
        Self {
            见过: Default::default(),
            条: Default::default(),
            多行: None,
        }
    }

    /// 拿这个任务的进度行；返回 `None` = 还只有一个任务，调用方继续用手绘单行
    fn 取条(&mut self, job: u64) -> Option<indicatif::ProgressBar> {
        self.见过.insert(job);
        if self.见过.len() < 2 {
            return None;
        }
        // TERM=dumb 时 indicatif 会**静默退化成 hidden**（`draw_target.rs` 里
        // `is_dumb()` 一票否决，连报错都没有）。这种终端下退回单行手绘 ——
        // 至少还有进度可看，而不是一片沉默。
        //
        // 实测记录：Hermes 的终端执行环境就是 `TERM=dumb`，所以本地测试时
        // `-j 2` 一度「一帧进度都没有」，排查了很久才发现是这个。
        if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
            return None;
        }
        if self.多行.is_none() {
            let m = indicatif::MultiProgress::new();
            // 实测踩到：默认的 `new()` 内部先做一次 TTY 检测，检测不到就**静默退化**
            // 成 hidden。这里强制指定 stderr 绘制目标。
            m.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(10));
            self.多行 = Some(m);
        }
        if let Some(pb) = self.条.get(&job) {
            return Some(pb.clone());
        }
        let 多行 = self.多行.as_ref()?;
        let pb = 多行.add(indicatif::ProgressBar::new(1000));
        let (满, 空) = 进度条字符();
        // indicatif 要三个字符：已完成 / 当前位置 / 未完成
        let chars: String = [满, 满, 空].iter().collect();
        if let Ok(sty) = indicatif::ProgressStyle::with_template("   [{bar:20}] {percent:>3}%  {msg}")
        {
            pb.set_style(sty.progress_chars(&chars));
        }
        self.条.insert(job, pb.clone());
        Some(pb)
    }

    /// 任务结束：收掉它那一行（`finish_and_clear` 避免留下半截条）
    fn 收尾(&mut self, job: u64) {
        if let Some(pb) = self.条.remove(&job) {
            pb.finish_and_clear();
        }
    }
}

/// 把引擎事件渲染成终端上的人话（引擎只发码和数字，文案归这里）
struct 终端出口 {
    quiet: bool,
    json: bool,
    /// stderr 是不是终端：不是（管道/重定向）就不画进度条，免得刷出一堆 \r
    tty: bool,
    /// `None` = 还没画过第一帧。**首次必须画**，否则 200ms 节流会把
    /// 短任务（内存盘上 150MB 只要 0.2 秒）的第一帧也吃掉，用户什么都看不到。
    last_draw: std::sync::Mutex<Option<std::time::Instant>>,
    /// 速度 / 剩余时间估算（复用引擎里那个 EtaEstimator：α=0.3 指数滑动平均）
    eta: std::sync::Mutex<yshjy::progress::EtaEstimator>,
    /// 终端列数（构造时取一次就够；取不到按 80）
    cols: usize,
    /// 多任务进度（`-j ≥ 2` 才启用，见 `多任务状态`）
    多任务: std::sync::Mutex<多任务状态>,
    /// 每个任务（job）的**第一帧时刻** —— 进度行里的「已用时」从这儿算。
    /// 引擎的进度事件里不带 elapsed（协议面不用为一行文字升版），所以外壳自己记。
    /// 按 job 分开记：`-j 2` 时两个任务各有各的起点，不能共用一个。
    /// 任务结束不主动清 —— job 号递增、一次运行里条数有限，留着比加清理更省事。
    开始时刻: std::sync::Mutex<std::collections::HashMap<u64, std::time::Instant>>,
}

impl 终端出口 {
    fn new(quiet: bool, json: bool) -> Self {
        use std::io::IsTerminal;
        Self {
            quiet,
            json,
            tty: std::io::stderr().is_terminal(),
            last_draw: std::sync::Mutex::new(None),
            eta: std::sync::Mutex::new(yshjy::progress::EtaEstimator::new()),
            // 注意：非交互环境（script / CI / 管道）下 ioctl 常常返回 0 列 ——
            // 直接 clamp 到 40 会让后面「速度 / 剩余 / 第几项」全被丢掉。
            // 拿到 0 或异常就按 80 列算，宁可保守也别把信息丢光。
            cols: match crossterm::terminal::size() {
                Ok((c, _)) if c >= 20 => (c as usize).clamp(20, 300),
                _ => 80,
            },
            多任务: std::sync::Mutex::new(多任务状态::new()),
            开始时刻: std::sync::Mutex::new(std::collections::HashMap::new()),
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

        // `-v` / `-vv` / `--log`：事件进日志（quiet 也记，--log 本来就是给排错用的）
        if !ev.is_progress() {
            调试(format!("事件 {ev:?}"));
        }
        if let Event::Diagnostic { code, note, .. } = ev {
            详细(format!("诊断 {code:?}：{note}"));
        }
        if let Event::EntryFinished { path, result, .. } = ev
            && !result.is_ok()
        {
            详细(format!("条目失败 {path}：{}", result.人话()));
        }

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
                job,
                progress,
                current,
            }
            | Event::PackProgress {
                job,
                progress,
                current,
            } => {
                if !self.tty {
                    return; // 管道模式：别画进度条
                }
                let eta = match self.eta.lock() {
                    Ok(mut e) => e.observe(progress.done_bytes, progress.total_bytes),
                    // 锁中毒（别处 panic 过）不该让进度条罢工：取回内部值继续用
                    Err(p) => p
                        .into_inner()
                        .observe(progress.done_bytes, progress.total_bytes),
                };
                // 「已用时」：第一次见到这个 job 时记下时刻，之后每帧算差值
                let 已用 = {
                    let mut m = match self.开始时刻.lock() {
                        Ok(m) => m,
                        Err(p) => p.into_inner(),
                    };
                    Some(
                        m.entry(*job)
                            .or_insert_with(std::time::Instant::now)
                            .elapsed(),
                    )
                };

                // ── 多任务（-j 2 以上）：每个任务一条独立行，交给 indicatif 统一重绘 ──
                // 这条路径不用我们的 200ms 节流 —— indicatif 自带绘制节流。
                let 多行条 = match self.多任务.lock() {
                    Ok(mut st) => st.取条(*job),
                    Err(p) => p.into_inner().取条(*job),
                };
                if let Some(pb) = 多行条 {
                    // 前缀「   [bar:20] 100%  」约占 30 列
                    let 可用 = self.cols.saturating_sub(30);
                    pb.set_message(进度消息(progress, &eta, current.as_deref(), 可用, 已用));
                    let 千分 = progress
                        .fraction()
                        .map(|f| (f * 1000.0).round() as u64)
                        .unwrap_or(0);
                    pb.set_position(千分.min(1000));
                    if std::env::var_os("YSHJY_DEBUG_PROGRESS").is_some() {
                        eprintln!("[DBG] 多行 job={job} 千分={千分}");
                    }
                    return;
                }
                if std::env::var_os("YSHJY_DEBUG_PROGRESS").is_some() {
                    eprintln!("[DBG] 单行 job={job}");
                }

                // ── 单任务：原来那条路，一个字没改 ──
                // 重画节流：引擎侧已按 100ms 合并，这里再降到 200ms，避免刷屏
                let now = std::time::Instant::now();
                if let Ok(mut last) = self.last_draw.lock() {
                    // 第一帧无条件画（`None`）—— 短任务也能看到进度，不是一片空白
                    if let Some(上一帧) = *last
                        && now.duration_since(上一帧) < std::time::Duration::from_millis(200)
                    {
                        return;
                    }
                    *last = Some(now);
                }
                let line = 画进度行(progress, &eta, current.as_deref(), self.cols, 已用);
                eprint!("\r{line}\x1b[K");
            }
            Event::EntryFinished { path, result, .. } if !result.is_ok() => {
                self.清行();
                eprintln!("   ⚠ {} 没解开（{}）", path, result.人话());
            }
            Event::Resumed { done, .. } => {
                // 断点续传：让用户明白「为什么一上来就有一大截进度、前面那堆文件没动」
                self.清行();
                eprintln!(
                    "{}",
                    文案().t("cli.resume_line", &[("done", &done.to_string())])
                );
            }
            Event::Diagnostic { code, note, .. } => {
                // 「走了慢路」是性能诊断，跟用户无关；只有排查问题时才要
                // （想看就 --json 拿事件流，那里是完整的）
                //
                // 「移动设备未检验」也跳过：下面打印汇总时会按结果单独说一遍
                // （Done/Cancelled/Unsupported 三种说法不同），这里再说就重复了。
                if matches!(
                    code,
                    yshjy::event::DiagnosticCode::SlowPath
                        | yshjy::event::DiagnosticCode::MovableNotVerified
                ) {
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
            Event::FlushProgress {
                job,
                pending,
                elapsed_ms,
            } => {
                // 移动设备落盘检验：必须让用户看见「在动」
                // —— 那块廉价 U 盘写 300 MB 要好几分钟，界面一动不动就是卡死
                if !self.tty {
                    return;
                }
                let 多行条 = match self.多任务.lock() {
                    Ok(mut m) => m.取条(*job),
                    Err(p) => p.into_inner().取条(*job),
                };
                let msg = 文案().t(
                    "flush.progress",
                    &[
                        ("mb", &format!("{:.0}", *pending as f64 / 1_048_576.0)),
                        ("secs", &format!("{:.0}", *elapsed_ms as f64 / 1000.0)),
                    ],
                );
                if let Some(pb) = 多行条 {
                    pb.set_message(msg);
                    return;
                }
                eprint!("\r\x1b[K{msg}");
            }
            Event::FlushFinished {
                outcome, millis, ..
            } => {
                self.清行();
                let key = match outcome {
                    yshjy::flush::FlushOutcome::Done => "flush.done",
                    yshjy::flush::FlushOutcome::Cancelled => "flush.cancelled",
                    yshjy::flush::FlushOutcome::Unsupported => "flush.unsupported",
                };
                eprintln!(
                    "{}",
                    文案().t(
                        key,
                        &[("secs", &format!("{:.1}", *millis as f64 / 1000.0))]
                    )
                );
            }
            Event::JobFinished { job, .. } => {
                // 多任务：收掉这个任务那一行（否则会留在屏幕上不消失）
                if let Ok(mut st) = self.多任务.lock() {
                    st.收尾(*job);
                }
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
        D::MovableNotVerified => "diag.movable_not_verified",
        D::HiddenAtOffset => "diag.hidden_at",
        D::RebuiltIndex => "diag.rebuilt_index",
        D::OffsetCorrected => "diag.offset_corrected",
    };
    文案().t(key, &[])
}

/// 任务结论 → 中文一句话 + 图标
fn 结论文案(o: &yshjy::error::Outcome, verify_only: bool) -> (String, &'static str) {
    use yshjy::error::Outcome;
    match o {
        // 只测不写：成功 = 校验通过，不能说「解开」（用户会去找产物）
        Outcome::Success if verify_only => (文案().t("result.phrase.verified", &[]), "✅"),
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
        C::PathTooLong => "error.path_too_long",
        C::DestIsFile => "error.dest_is_file",
        C::NeedPassword => "error.password_missing",
        C::WrongPassword => "error.password_wrong",
        C::HeadEncrypted => "error.head_encrypted",
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


/// 空间不足的三个人话数字（需要 / 只有 / 还差）
///
/// 引擎只给 `need` / `available` 两个结构化参数（`JobOutcome.error_detail`），
/// 句子在这一层拼 —— 换语言不用动引擎。缺参数就返回 `None`（不乱猜，宁可只说
/// 一句「磁盘空间不够」，也不给一个算错的「还差」）。
fn 空间不足说明(detail: &[(String, String)]) -> Option<String> {
    let 取 = |键: &str| {
        detail
            .iter()
            .find(|(k, _)| k == 键)
            .and_then(|(_, v)| v.parse::<u64>().ok())
    };
    let need = 取("need")?;
    let available = 取("available")?;
    let short = need.saturating_sub(available);
    Some(文案().t(
        "result.space_shortage",
        &[
            ("need", &人类可读(need)),
            ("available", &人类可读(available)),
            ("short", &人类可读(short)),
        ],
    ))
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
fn 打印汇总(report: &yshjy::batch::BatchReport, quiet: bool, verify_only: bool) {
    use yshjy::error::Outcome;

    for j in &report.jobs {
        if matches!(j.outcome, Outcome::Failed(_)) {
            continue; // 留给后面的合并汇报
        }
        let (txt, icon) = 结论文案(&j.outcome, verify_only);
        if quiet {
            println!("{}", j.stats.dest.as_deref().unwrap_or(&j.input).display());
            continue;
        }
        let 耗时 = format!("{:.1}", j.stats.millis as f64 / 1000.0);
        let 大小 = 人类可读(j.stats.bytes_written);
        let 行 = 文案().t(
            "result.line",
            &[
                ("icon", icon),
                ("input", &j.input.display().to_string()),
                ("text", &txt),
                ("size", &大小),
                ("secs", &耗时),
            ],
        );
        // 成功绿 / 部分成功黄（--no-color 或非终端时原样输出）
        println!(
            "{}",
            上色(&行, if matches!(j.outcome, Outcome::Success) { 绿 } else { 黄 })
        );
        // 移动设备提示（用户 2026-10-30 拍板的口径）
        //
        // 开关关着的时候**这一句也必须有**：没开检验的人照样会「看到 ✅ 就拔」，
        // 不提这一句，等于我们什么都没做。
        // 三种结局说法不同，不能混成一句「完事了」。
        if matches!(j.outcome, Outcome::Success | Outcome::Partial { .. })
            && let Some(d) = j.stats.dest.as_deref()
        {
            let 提示键 = match j.stats.flush_outcome {
                Some(yshjy::flush::FlushOutcome::Done) => Some("result.hint.movable_flushed"),
                Some(yshjy::flush::FlushOutcome::Cancelled) => Some("result.hint.movable_cancelled"),
                Some(yshjy::flush::FlushOutcome::Unsupported) => {
                    Some("result.hint.movable_unsupported")
                }
                None => yshjy::flush::is_removable(d).then_some("result.hint.movable_eject"),
            };
            if let Some(k) = 提示键 {
                eprintln!(
                    "{}",
                    文案().t(
                        k,
                        &[("secs", &format!("{:.1}", j.stats.verify_millis as f64 / 1000.0))]
                    )
                );
            }
        }
        if verify_only {
            // 只测不写：没有产物目录可给
            continue;
        }
        if let Some(d) = &j.stats.dest {
            // 显示**绝对路径**：`.` 对用户等于没说
            // （实测踩到：产物落在 ~ 里，用户看完输出还是找不到）
            let abs = std::fs::canonicalize(d).unwrap_or_else(|_| d.clone());
            println!("     📁 {}", abs.display());
        }
        for (name, res, why) in j.stats.failed.iter().take(5) {
            match why {
                Some(w) => println!("     ⚠ {name}（{}）—— {w}", res.人话()),
                None => println!("     ⚠ {name}（{}）", res.人话()),
            }
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
            let 原因 = 结论文案(&j.outcome, verify_only).0;
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
        let (txt, icon) = 结论文案(&j.outcome, verify_only);
        let 行 = if j.stats.bytes_written == 0 && j.stats.millis < 1000 {
            // 一个字节都没写、也几乎没耗时（识别 / 预检阶段就被拦下）：
            // 别印「0 B，用时 0.0 秒」—— 那两个数对用户零信息量（台账 ⑤）
            文案().t(
                "result.line_short",
                &[
                    ("icon", icon),
                    ("input", &j.input.display().to_string()),
                    ("text", &txt),
                ],
            )
        } else {
            文案().t(
                "result.line",
                &[
                    ("icon", icon),
                    ("input", &j.input.display().to_string()),
                    ("text", &txt),
                    ("size", &人类可读(j.stats.bytes_written)),
                    ("secs", &format!("{:.1}", j.stats.millis as f64 / 1000.0)),
                ],
            )
        };
        // 失败红（--no-color 或非终端时原样）
        println!("{}", 上色(&行, 红));
        // 再说清楚"你给的是啥"
        if let Some(提示) = 文件类型提示(&j.input) {
            println!("   ℹ {提示}");
        }
        // 空间不足：把「需要 / 只有 / 还差」三个人话数字补在后面。
        // 以前这里只有一句「磁盘空间不够」，用户不知道要清多少（S5 的另一半）。
        if let Some(句) = 空间不足说明(&j.error_detail) {
            println!("     {句}");
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
                "failed": j.stats.failed.iter().map(|(n, r, why)| serde_json::json!({
                    "path": n, "result": r.as_str(), "human": r.人话(), "reason": why
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

// ───────────────────── 终端进度仪表盘（第二十二批） ─────────────────────
//
// 之前终端只画「百分比 + 字节 + 文件名」一行数字，看不出还要等多久；界面上却有
// 进度条和剩余时间 —— 同一份数据在两边显示不一致。这里把 CLI 也做成一行仪表盘：
//   条 + 百分比 + 字节 + 第几项 + 速度 + 剩余时间 + 当前文件
//
// 速度与剩余时间**复用引擎里的 `EtaEstimator`**（α=0.3 指数滑动平均、样本 <3 不给估算），
// 不在这里重写一遍算法 —— 那个坑 7-Zip 已经踩过（见 src/progress.rs 注释）。

/// 进度条填充字符：UTF-8 终端用方块，其余退回 ASCII
///
/// 非 UTF-8 终端（LANG=C 之类）画 █░ 会变成一堆问号或方框，宁可难看也别乱码。
fn 进度条字符() -> (char, char) {
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .any(|v| v.to_ascii_uppercase().contains("UTF-8"));
    if utf8 { ('█', '░') } else { ('#', '-') }
}

/// 剩余时间的说法：45 秒 / 3 分 20 秒 / 1 小时 5 分
fn 时长可读(d: std::time::Duration) -> String {
    let 总秒 = d.as_secs();
    if 总秒 < 60 {
        format!("{总秒} 秒")
    } else if 总秒 < 3600 {
        let (分, 秒) = (总秒 / 60, 总秒 % 60);
        if 秒 == 0 {
            format!("{分} 分")
        } else {
            format!("{分} 分 {秒} 秒")
        }
    } else {
        let (时, 分) = (总秒 / 3600, (总秒 % 3600) / 60);
        if 分 == 0 {
            format!("{时} 小时")
        } else {
            format!("{时} 小时 {分} 分")
        }
    }
}

/// 按**显示宽度**截断（中文占两列 —— 按字符数截断会让中文文件名错位）
fn 截断显示宽度(s: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if s.width() <= width {
        return s.to_string();
    }
    if width <= 1 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > width - 1 {
            break; // 留一格给省略号
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// 画一行进度条（**纯函数**，不碰终端，便于单测）
///
/// 放不下时**从右往左依次丢**：当前文件名 → 已用时 → 剩余 → 速度 → 第几项，
/// 保证「进度条 + 百分比 + 字节」这三样永远在。
/// `已用时` 由调用方给（外壳按 job 记第一帧时刻算出来）—— 这里只管渲染，
/// 所以单测能直接塞一个值进去，不用等真实时间流逝。
fn 画进度行(
    progress: &yshjy::progress::ProgressView,
    eta: &yshjy::progress::Eta,
    current: Option<&str>,
    cols: usize,
    已用时: Option<std::time::Duration>,
) -> String {
    use unicode_width::UnicodeWidthStr;

    let (满, 空) = 进度条字符();
    let 条宽 = if cols >= 110 {
        24
    } else if cols >= 90 {
        18
    } else if cols >= 60 {
        12
    } else {
        8
    };
    let 已填 = progress
        .fraction()
        .map(|f| (f * 条宽 as f64).round() as usize)
        .unwrap_or(0)
        .min(条宽);
    let mut bar = String::with_capacity(条宽);
    for i in 0..条宽 {
        bar.push(if i < 已填 { 满 } else { 空 });
    }

    let 百分比 = progress
        .fraction()
        .map(|f| format!("{:>5.1}%", f * 100.0))
        .unwrap_or_else(|| "  …  ".into());
    let mut line = format!("   [{bar}] {百分比}");

    // 文字段（字节 · 项号 · 速度 · 剩余 · 文件名）走共用函数 —— 多任务路径也用它
    line.push_str(&进度消息(
        progress,
        eta,
        current,
        cols.saturating_sub(line.width()),
        已用时,
    ));
    line
}

/// 进度行的**文字部分**（字节 · 项号 · 速度 · 剩余 · 已用时 · 文件名），不含条与百分比。
///
/// 抽出来给两条渲染路径共用：
/// - 单任务：`画进度行` 拼上条和百分比，用 `\r` 重画（已测好，不动）
/// - 多任务（`-j 2` 以上）：塞进 indicatif 的 `{msg}`，条和重画交给它管
///
/// **算法只写一份** —— 否则两个路径的「剩余时间」会算出不同结果。
/// `可用宽` 是这一行还剩多少列可写；不够就按「文件名 → 已用时 → 剩余 → 速度 → 项号」放弃
/// （最先丢的永远是文件名，最后才轮到「第几项」，因为「进度条 + 百分比 + 字节」是底线）。
fn 进度消息(
    progress: &yshjy::progress::ProgressView,
    eta: &yshjy::progress::Eta,
    current: Option<&str>,
    可用宽: usize,
    已用时: Option<std::time::Duration>,
) -> String {
    use unicode_width::UnicodeWidthStr;

    let mut line = String::new();
    // 字节：总量未知时就只报已解，别硬凑一个「/0 B」
    let 字节段 = if progress.total_bytes > 0 {
        format!(
            "{}/{}",
            人类可读(progress.done_bytes),
            人类可读(progress.total_bytes)
        )
    } else {
        人类可读(progress.done_bytes)
    };
    line.push_str(&字节段);

    let 项段 = if progress.total_files > 0 {
        let 当前项 = (progress.done_files + 1).min(progress.total_files);
        format!("  {当前项}/{} 项", progress.total_files)
    } else {
        String::new()
    };
    let 速度段 = eta
        .speed_bps
        .map(|s| format!("  {}/s", 人类可读(s as u64)))
        .unwrap_or_default();
    // 不足 1 秒不显示「剩 0 秒」—— 那比不显示更让人困惑
    let 剩余段 = eta
        .remaining
        .filter(|d| d.as_secs() >= 1)
        .map(|d| format!("  剩 {}", 时长可读(d)))
        .unwrap_or_default();
    // 已用时（对的就是 indicatif 里 `{elapsed}` 那个字段）：跟「剩余」是同一类信息，挨着放。
    // 同样「不足 1 秒不显示」—— 刚起来的任务写「已 0 秒」纯噪音，还白占宽度。
    let 已用段 = 已用时
        .filter(|d| d.as_secs() >= 1)
        .map(|d| format!("  已 {}", 时长可读(d)))
        .unwrap_or_default();

    // 先加「第几项」，再加速度/剩余/已用 —— 宽度不够就从后往前放弃（已用最先丢）
    if line.width() + 项段.width() <= 可用宽 {
        line.push_str(&项段);
    }
    if line.width() + 速度段.width() <= 可用宽 {
        line.push_str(&速度段);
    }
    if line.width() + 剩余段.width() <= 可用宽 {
        line.push_str(&剩余段);
    }
    // 「已用时」是最后才加的，而且要先给文件名留位置 ——
    // 实测抓到的坑：加完已用后 100 列终端上「当前文件名」被挤掉了，而文件名恰恰是
    // 用户最想看的（「现在在解哪个文件」）。所以：宁可不显示已用，也别挤掉文件名。
    // 留 8 列跟下面文件名的准入线是同一个数（`剩余宽 >= 8` 才放文件名）：
    // 少于 8 列的文件名只剩省略号，等于没显示，那就该牺牲已用。
    let 给文件名留 = if current.is_some_and(|n| !n.is_empty()) {
        8
    } else {
        0
    };
    if line.width() + 已用段.width() + 给文件名留 <= 可用宽 {
        line.push_str(&已用段);
    }

    // 剩下的位置给当前文件名
    if let Some(name) = current
        && !name.is_empty()
    {
        let 剩余宽 = 可用宽.saturating_sub(line.width() + 2); // 两个空格分隔
        if 剩余宽 >= 8 {
            line.push_str("  ");
            line.push_str(&截断显示宽度(name, 剩余宽));
        }
    }
    line
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

/// 打包参数
///
/// 参数已经涨到 10 个了 —— 继续用位置参数，两个 `bool` 传反了编译器不会报错
/// （v0.8 的 `do_extract` 就栽在这上面，后来改成参数结构体）。
struct 打包参数<'a> {
    targets: &'a [String],
    dest_opt: &'a Option<String>,
    password: &'a Option<String>,
    /// `--tail`（等价于 `--layout L1`）
    tail: bool,
    quiet: bool,
    split: &'a Option<String>,
    checksum: bool,
    readme: bool,
    /// `--layout L1|L2|…`
    layout: &'a Option<String>,
    /// `--cover <文件>`
    cover: &'a Option<String>,
    /// `--format zip|tar.gz|tar.zst`：一给就走「普通压缩」（不加外壳），不给＝伪装包
    format: &'a Option<String>,
    /// `--sfx` / `--sfx-linux`：自解压种类（第 23 批）
    sfx: Option<&'static str>,
    /// `--level 0-9`：压缩级别（P23-4）
    level: Option<u32>,
    /// `--decoy` / `--decoy-size` / `--decoy-signatures`：干扰数据（第三期 B）
    decoy: yshjy::pack::decoy::DecoyOptions,
    /// `--json`：事件按行输出（打包进度也要走这条路，否则脚本拿不到）
    json: bool,
}

fn do_pack(a: 打包参数<'_>) -> ExitCode {
    let 打包参数 {
        targets,
        dest_opt,
        password,
        tail,
        quiet,
        split,
        checksum,
        readme,
        layout,
        cover,
        format,
        sfx,
        level,
        decoy,
        json,
    } = a;
    // 文案全走语言包（引擎只给结构化数据）。v0.9 第 10 批从 `PackReport::summary()`
    // 搬过来的 —— 那句以前在引擎里拼死中文，换个界面语言也变不过来。
    let 语 = 文案();
    let inputs: Vec<PathBuf> = targets.iter().map(PathBuf::from).collect();

    // 打包格式（T17-4）：`--format` 一给就走「普通压缩」（不加外壳），不给就是原来的伪装包
    // —— 老行为一个字都不变（这条是「只加不删」）。
    let 格式 = match format.as_deref() {
        None => yshjy::pack::PackFormat::Zip,
        Some(s) => match yshjy::pack::PackFormat::解析(s) {
            Some(f) => f,
            None => {
                eprintln!("❌ --format 只认 zip / tar.gz / tar.zst（收到 {s}）");
                return ExitCode::from(code::USAGE);
            }
        },
    };
    let 普通压缩 = format.is_some();
    if 格式.是tar() {
        if password.is_some() {
            eprintln!("❌ tar 格式不支持加密 —— 想设密码请用 zip");
            return ExitCode::from(code::USAGE);
        }
        if split.is_some() {
            eprintln!("❌ tar 格式不支持分卷 —— 分卷只有 zip 能做");
            return ExitCode::from(code::USAGE);
        }
        if sfx.is_some() {
            eprintln!("❌ tar 格式不能做自解压 —— 自解压包装的是 zip");
            return ExitCode::from(code::USAGE);
        }
        if decoy.is_on() {
            eprintln!("❌ tar 格式不支持干扰数据 —— 那套是给伪装包用的");
            return ExitCode::from(code::USAGE);
        }
        if cover.is_some() {
            eprintln!("❌ --format 走普通压缩，不加外壳（--cover 只对伪装包有意义）");
            return ExitCode::from(code::USAGE);
        }
    }

    for i in &inputs {
        if !i.exists() {
            eprintln!(
                "{}",
                语.t("cli.pack_missing", &[("path", &i.display().to_string())])
            );
            return ExitCode::from(code::ERR);
        }
    }
    let mut out: PathBuf = match dest_opt {
        Some(d) => PathBuf::from(d),
        None => {
            let base = inputs[0]
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "packed".into());
            let 后缀 = if 普通压缩 {
                // `--format` 默认名跟着格式走：a.txt → a.txt.tar.gz
                格式.名字()
            } else {
                match sfx {
                    Some("linux") => "sh",
                    Some(_) => "exe",
                    None => "mp4",
                }
            };
            PathBuf::from(format!("{base}.{后缀}"))
        }
    };

    // 自解压（第 23 批）：产物是能双击运行的 exe，和「伪装 / 分卷 / 解压码」互斥。
    // 这些限制都**明确报错**，不静默忽略（D20 的教训）。
    if let Some(种类) = sfx {
        let 后缀 = if 种类 == "linux" { "sh" } else { "exe" };
        if split.is_some() {
            eprintln!("❌ 自解压包不能分卷（双击要是一个完整文件）");
            return ExitCode::from(code::USAGE);
        }
        if cover.is_some() {
            eprintln!("❌ --sfx 不需要 --cover：外壳就是自带的存根");
            return ExitCode::from(code::USAGE);
        }
        match out.extension().and_then(|e| e.to_str()) {
            None => {
                out.set_extension(后缀);
            }
            Some(e) if e.eq_ignore_ascii_case(后缀) => {}
            Some(e) => {
                eprintln!("❌ 自解压输出必须以 .{后缀} 结尾（现在 .{e}）");
                return ExitCode::from(code::USAGE);
            }
        }
    }

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
        format: 格式,
        ..Default::default()
    };
    // 压缩级别（P23-4）：0=仅存储（最快）/ 1-9 越大越小越慢
    if let Some(n) = level {
        opts.level = n;
    }
    // 只有 MP4 才有 `free` 原子（ISO 盒子）；图片/文本用它会直接把封面写坏
    if tail || !素材.is_video() {
        opts.layout = yshjy::pack::PackLayout::TailAppend;
    }
    // `--layout`：显式指定优先（L1 尾部追加 / L2 free 原子）
    if let Some(l) = layout {
        match l.to_ascii_lowercase().as_str() {
            "l1" => opts.layout = yshjy::pack::PackLayout::TailAppend,
            "l2" => opts.layout = yshjy::pack::PackLayout::FreeAtom,
            "l5" | "l6" => {
                eprintln!(
                    "❌ --layout {l} 不做：实测 L5 解压残废、L6 视频播不了 —— \
                     这两种「看起来更隐蔽、实际把用户坑了」的布局不提供"
                );
                return ExitCode::from(code::USAGE);
            }
            "l3" | "l4" => {
                eprintln!("❌ --layout {l} 还没实现（假签名 / 干扰数据排在 0.9.22-c 评估）");
                return ExitCode::from(code::USAGE);
            }
            _ => {
                eprintln!("❌ --layout 只认 L1（尾部追加）/ L2（free 原子），收到 {l}");
                return ExitCode::from(code::USAGE);
            }
        }
    }
    // `--cover <文件>`：用户自己的封面。读不到就报错 —— 静默换内置素材
    // 等于「以为用了自己的图，实际没有」
    if let Some(c) = cover {
        match std::fs::read(c) {
            Ok(bytes) => {
                opts.cover = Some(bytes);
                详细(format!("封面：{c}（{} 字节）", opts.cover.as_ref().map_or(0, |b| b.len())));
            }
            Err(e) => {
                eprintln!("❌ --cover 读不了 {c}：{e}");
                return ExitCode::from(code::ERR);
            }
        }
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

    // 打包进度（梯队四第 11 项）：CLI 以前**一条进度事件都不发** —— 打 2 GB 包时
    // 终端几十秒没有任何动静，跟卡死没区别（实测 2 GB 用了 194 秒，全程无声）。
    // 分母 = 输入总字节；拼装阶段由 `TotalHint` 把分母放大成「输入 + 中间 ZIP」。
    let sink: std::sync::Arc<dyn yshjy::event::EventSink> =
        std::sync::Arc::new(终端出口::new(quiet, json));
    let ctx = yshjy::ctx::UnpackCtx::new(1, sink);
    let (总字节, 总文件) = yshjy::pack::输入统计(&opts.inputs);
    ctx.set_total(总字节, 总文件);
    let mut 报 = |t: yshjy::pack::PackTick| match t {
        yshjy::pack::PackTick::Wrote(n, 名字) => ctx.advance_pack(n, 名字.as_deref()),
        yshjy::pack::PackTick::TotalHint(额外) => ctx.set_total(总字节 + 额外, 总文件),
    };
    // 自解压与干扰数据互斥：存根靠**固定偏移**找 ZIP，前面插噪声就再也定位不到
    if decoy.is_on() && sfx.is_some() {
        eprintln!(
            "❌ --decoy 不能和 --sfx / --sfx-linux 同用：自解压包靠写死的偏移找数据，\
             前面插噪声会把它弄坏"
        );
        return ExitCode::from(code::USAGE);
    }
    let 打包结果 = match sfx {
        Some("linux") => yshjy::pack::build_sfx_sh_ctx(&opts, &mut bw, &mut 报),
        Some(_) => yshjy::pack::build_sfx_ctx(&opts, &mut bw, &mut 报),
        // `--format` = 普通压缩（标准 zip / tar.gz / tar.zst，没有外壳）
        None if 普通压缩 => yshjy::pack::build_plain_ctx(&opts, &mut bw, &mut 报),
        None => yshjy::pack::build_ctx_decoy(&opts, &decoy, &mut bw, &mut 报),
    };
    // 收尾补一条终态，保证终端/脚本看到的进度是最终值
    ctx.force_pack_progress(None);
    match 打包结果 {
        Ok(rep) => {
            use std::io::Write;
            let _ = bw.flush();
            drop(bw);
            // Linux 自解压产物要能直接双击 / 执行
            #[cfg(unix)]
            if sfx == Some("linux") {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &out,
                    std::fs::Permissions::from_mode(0o755),
                );
            }
            if quiet {
                println!("{}", out.display());
            } else if 普通压缩 {
                // 普通压缩没有封面、也没有布局 —— 别硬套伪装包那句
                // （套上去就会说「封面 0 B … 布局 尾部追加」，用户看着糊涂）
                println!(
                    "{}",
                    语.t(
                        "cli.pack_done_archive",
                        &[
                            ("total", &人类可读(rep.total_size)),
                            ("entries", &rep.entries.to_string()),
                            ("format", 格式.名字()),
                        ]
                    )
                );
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
                match sfx {
                    Some("linux") => println!(
                        "🖱 双击（或终端里 sh 这个文件）就会解到同目录的新文件夹（Linux）"
                    ),
                    Some(_) => println!(
                        "🖱 双击这个 exe 就会自动解压到同目录的新文件夹（Windows）"
                    ),
                    None => {}
                }
                // 干扰数据：用户得知道自己撒了多少（不然「设置了没生效」看不出来）
                if rep.noise_size > 0 {
                    // 「假签名开没开」也要说清楚 —— 开关类参数不给反馈等于没生效
                    let 签名 = 语.t(
                        if decoy.signatures {
                            "cli.pack_decoy_sig_on"
                        } else {
                            "cli.pack_decoy_sig_off"
                        },
                        &[],
                    );
                    println!(
                        "{}",
                        语.t(
                            "cli.pack_decoy",
                            &[
                                ("count", &decoy.count.to_string()),
                                ("size", &人类可读(decoy.size as u64)),
                                ("total", &人类可读(rep.noise_size)),
                                ("sig", &签名),
                            ]
                        )
                    );
                }
                for w in &rep.warnings {
                    println!("{}", 语.t("cli.pack_warn", &[("warn", w)]));
                }
            }
            // `--keep`：把中间 ZIP 的位置告诉用户（不然「保留了」等于没说）
            if !quiet
                && let Some(tmp) = &rep.intermediate
            {
                println!("中间文件（--keep 保留）：{}", tmp.display());
            }
            // 落盘检验 —— **必须排在自检之前**
            //
            // 自检是把包读回来验证，但读回来命中的是**页缓存**：盘上有没有东西它不知道。
            // 2026-10-30 记一笔：打包到 U 盘报完成、自检还说"通过"，拔盘包就没了。
            // 先等真落盘，再自检读到的才是盘上的数据。
            {
                let cancel = yshjy::progress::CancelToken::new();
                let t0 = std::time::Instant::now();
                let r = yshjy::flush::flush_if_needed(
                    std::path::Path::new(&out),
                    rep.total_size,
                    &cancel,
                    &mut |pending| {
                        // 只在真终端里画覆盖行：管道（`| tail`、脚本）里 \r 不生效，
                        // 会刷成一整屏重复的"正在检验落盘"
                        if !quiet && std::io::IsTerminal::is_terminal(&std::io::stderr()) {
                            let msg = 语.t(
                                "flush.progress",
                                &[
                                    ("mb", &format!("{:.0}", pending as f64 / 1_048_576.0)),
                                    ("secs", &format!("{:.0}", t0.elapsed().as_secs())),
                                ],
                            );
                            eprint!("\r\x1b[K{msg}");
                        }
                        // 事件照发（--json / 界面都要能看到，不能只在终端上画）
                        ctx.emit(&yshjy::event::Event::FlushProgress {
                            job: 0,
                            pending,
                            elapsed_ms: t0.elapsed().as_millis() as u64,
                        });
                    },
                );
                if let Some((结局, millis)) = r {
                    ctx.emit(&yshjy::event::Event::FlushFinished {
                        job: 0,
                        outcome: 结局,
                        millis,
                    });
                    let key = match 结局 {
                        yshjy::flush::FlushOutcome::Done => "flush.done",
                        yshjy::flush::FlushOutcome::Cancelled => "flush.cancelled",
                        yshjy::flush::FlushOutcome::Unsupported => "flush.unsupported",
                    };
                    if !quiet {
                        println!();
                        println!(
                            "{}",
                            语.t(key, &[("secs", &format!("{:.1}", millis as f64 / 1000.0))])
                        );
                    }
                }
            }
            // 自检 —— 不能"以为成功就完事"
            //
            // 分派：普通压缩的 tar 系没有 EOCD，走 tar 自己那份索引数条目；
            // ZIP 那条把「有没有封面」如实传下去 —— 普通压缩 zip_offset 就是 0，
            // 传 Some(0) 会被自己的「ZIP 起点为 0，封面没包住数据」误报（T17-4 前没暴露，
            // 因为 CLI 当时根本没有普通压缩这条路）。
            let 自检结果 = if 普通压缩 && 格式.是tar() {
                yshjy::pack::self_check_tar(&out, 格式, rep.entries)
            } else {
                let 起点 = (rep.zip_offset > 0).then_some(rep.zip_offset);
                yshjy::pack::self_check_at(&out, rep.entries, 起点, rep.noise_size)
            };
            match 自检结果 {
                Ok(warns) if warns.is_empty() => {
                    if !quiet {
                        if sfx.is_some() {
                            println!("   ✅ 自检通过（存根能找到尾部数据）");
                        } else if 普通压缩 {
                            println!(
                                "{}",
                                语.t(
                                    "cli.pack_selfcheck_ok_archive",
                                    &[("entries", &rep.entries.to_string())]
                                )
                            );
                        } else {
                            println!("{}", 语.t("cli.pack_selfcheck_ok", &[]));
                        }
                    }
                    // 交付形态：分卷 / 校验和 / 说明书（DESIGN §14.2 / §17.8）
                    let post = yshjy::pack_post::PostOptions {
                        split: match split.as_deref() {
                            Some(s) => match yshjy::pack_post::parse_size(s) {
                                Some(n) => Some(n),
                                None => {
                                    eprintln!("--split 的大小看不懂：{s}（写法如 2G / 500M / 1048576）");
                                    return ExitCode::from(code::USAGE);
                                }
                            },
                            None => None,
                        },
                        checksum,
                        readme,
                        password: password.clone(),
                    };
                    match yshjy::pack_post::postprocess(&out, &post) {
                        Ok(p) => {
                            if !quiet {
                                if !p.volumes.is_empty() {
                                    println!(
                                        "已分卷：{} 个文件（{}.001 起）",
                                        p.volumes.len(),
                                        out.display()
                                    );
                                }
                                if let Some(c) = &p.checksum {
                                    println!("校验和：{}", c.display());
                                }
                                if let Some(r) = &p.readme {
                                    println!("说明书：{}", r.display());
                                }
                            }
                        }
                        // 后处理失败不改判：包已经打好了，只是少了附赠文件
                        Err(e) => eprintln!("交付处理失败（包本身没问题）：{e}"),
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
    let 目标 = 找位置参数(argv, 带值的选项());
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
fn do_extract_cover(
    targets: &[String],
    dest: Option<&std::path::Path>,
    quiet: bool,
) -> ExitCode {
    use std::sync::Arc;

    let mut 失败 = false;
    for t in targets {
        let p = std::path::Path::new(t);
        let sink: Arc<dyn yshjy::event::EventSink> = Arc::new(yshjy::event::NullSink);
        let ctx = yshjy::ctx::UnpackCtx::new(0, sink);

        match yshjy::job::identify(p, &ctx) {
            Ok(yshjy::job::Target::Zip { base, .. }) => {
                // 落点：给了 `-o` 就跟着走，没给就包旁边（历史行为）
                match yshjy::cover_extract::extract_zip_cover_to(p, base, dest) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use yshjy::progress::{Eta, ProgressView};

    #[cfg(windows)]
    #[test]
    fn windows内存探针给得出真实数字() {
        let m = windows可用内存mib().expect("真 Windows 上应读得到可用内存（W5）");
        assert!(m > 0, "可用内存不可能为 0");
        // 就算 2 TB 内存的怪机器也到不了 2^31 MiB；超了说明换算出错
        assert!(m < (1u64 << 31), "可用内存 {m} MiB 不合常理");
    }

    #[cfg(windows)]
    #[test]
    fn windows注册探针后引擎读得到内存() {
        // 探针没注册时 available_mem_mib 在 Windows 上是 None（没有 /proc），
        // 注册后必须给得出数 —— 这就是内存闸从「形同虚设」到「有数可比」的那一步
        yshjy::hardware::注册内存探针(windows可用内存mib);
        let m = yshjy::hardware::available_mem_mib()
            .expect("注册探针后 available_mem_mib 不该再是 None");
        assert!(m > 0);
    }

    #[test]
    fn 空间不足说明给出三个人话数字() {
        // 文案仓库在 main 开头才初始化；单测直接调要自己先装一份
        let _ = I18N.set(yshjy::i18n::I18n::new(Some("zh-CN")));
        let d = vec![
            ("need".to_string(), (300u64 * 1024 * 1024).to_string()),
            ("available".to_string(), (110u64 * 1024 * 1024).to_string()),
        ];
        let s = 空间不足说明(&d).expect("两个参数都在就该拼出句子");
        assert!(s.contains("300.0 MB"), "需要量没进文案：{s}");
        assert!(s.contains("110.0 MB"), "可用量没进文案：{s}");
        assert!(s.contains("190.0 MB"), "还差没算对：{s}");
    }

    #[test]
    fn 空间不足缺参数就不乱猜() {
        // 宁可只说一句「磁盘空间不够」，也不给一个算错的「还差」
        assert!(空间不足说明(&[]).is_none());
        assert!(空间不足说明(&[("need".to_string(), "123".to_string())]).is_none());
        // 参数坏掉（不是数字）也不能崩
        let 坏 = vec![
            ("need".to_string(), "很多".to_string()),
            ("available".to_string(), "10".to_string()),
        ];
        assert!(空间不足说明(&坏).is_none());
    }

    #[test]
    fn 时长说法分档() {
        assert_eq!(时长可读(Duration::from_secs(45)), "45 秒");
        assert_eq!(时长可读(Duration::from_secs(60)), "1 分");
        assert_eq!(时长可读(Duration::from_secs(200)), "3 分 20 秒");
        assert_eq!(时长可读(Duration::from_secs(3600)), "1 小时");
        assert_eq!(时长可读(Duration::from_secs(3900)), "1 小时 5 分");
    }

    #[test]
    fn 中文文件名按显示宽度截断() {
        // 中文占两列：10 列宽最多放 4 个汉字 + 省略号
        let got = 截断显示宽度("我的世界整合包.zip", 10);
        assert!(got.ends_with('…'), "截断了要带省略号，实际 {got}");
        use unicode_width::UnicodeWidthStr;
        assert!(
            got.width() <= 10,
            "不能超出 10 列，实际 {}",
            got.width()
        );
        // 放得下就原样返回，不加省略号
        assert_eq!(截断显示宽度("a.zip", 20), "a.zip");
    }

    #[test]
    fn 进度行含速度与剩余时间() {
        let p = ProgressView {
            done_bytes: 1024 * 1024 * 300,
            total_bytes: 1024 * 1024 * 600,
            done_files: 3,
            total_files: 10,
        };
        let eta = Eta {
            speed_bps: Some(12.4 * 1024.0 * 1024.0),
            remaining: Some(Duration::from_secs(72)),
        };
        // 已用时塞个真值：80 秒 → 「已 1 分 20 秒」（就是 indicatif 的 {elapsed}）
        // 宽度给 160：这条测的是「每段都在」，不是窄屏取舍（窄屏另有一条测试）
        let line = 画进度行(
            &p,
            &eta,
            Some("data/game.bin"),
            160,
            Some(Duration::from_secs(80)),
        );
        assert!(line.contains("50.0%"), "要有百分比：{line}");
        assert!(line.contains('█') && line.contains('░'), "要有条状进度：{line}");
        assert!(line.contains("12.4 MB/s"), "要有速度：{line}");
        assert!(line.contains("剩 1 分 12 秒"), "要有剩余时间：{line}");
        assert!(line.contains("已 1 分 20 秒"), "要有已用时：{line}");
        // 显示的是「正在处理第几个」：done_files=3 → 第 4/10 项
        assert!(line.contains("4/10 项"), "要有第几项：{line}");
        assert!(line.contains("data/game.bin"), "要有当前文件：{line}");
    }

    #[test]
    fn 剩余不足一秒不显示剩零秒() {
        let p = ProgressView {
            done_bytes: 990,
            total_bytes: 1000,
            done_files: 0,
            total_files: 1,
        };
        let eta = Eta {
            speed_bps: Some(100.0),
            remaining: Some(Duration::from_millis(300)),
        };
        let line = 画进度行(&p, &eta, None, 100, None);
        assert!(!line.contains("剩 0 秒"), "别写「剩 0 秒」：{line}");
        assert!(!line.contains("剩"), "不足一秒就别显示剩余：{line}");
    }

    #[test]
    fn 窄终端不超宽也不缺关键项() {
        let p = ProgressView {
            done_bytes: 100,
            total_bytes: 1000,
            done_files: 1,
            total_files: 9,
        };
        let eta = Eta {
            speed_bps: Some(1_000_000.0),
            remaining: Some(Duration::from_secs(5)),
        };
        for cols in [40usize, 60, 80, 120, 200] {
            let line = 画进度行(
                &p,
                &eta,
                Some("很长的中文文件名要截断/并且/还有/子目录/文件.bin"),
                cols,
                Some(Duration::from_secs(9)),
            );
            use unicode_width::UnicodeWidthStr;
            assert!(
                line.width() <= cols,
                "{cols} 列下画出了 {} 列宽的行：{line}",
                line.width()
            );
            assert!(line.contains("10.0%"), "{cols} 列下百分比不能丢：{line}");
        }
    }

    #[test]
    fn 总量未知时不硬凑分母() {
        let p = ProgressView {
            done_bytes: 5 * 1024 * 1024,
            total_bytes: 0,
            done_files: 2,
            total_files: 0,
        };
        let eta = Eta::default();
        let line = 画进度行(&p, &eta, None, 80, None);
        assert!(!line.contains("/0 B"), "总量未知别显示 /0 B：{line}");
        assert!(line.contains("5.0 MB"), "已解量要显示：{line}");
        assert!(line.contains("…"), "未知进度用省略号表示：{line}");
        // 没有速度样本时不该出现 /s
        assert!(!line.contains("/s"), "没有估算就别瞎写速度：{line}");
    }

    #[test]
    fn 已用不足一秒不显示已零秒() {
        let p = ProgressView {
            done_bytes: 10,
            total_bytes: 100,
            done_files: 0,
            total_files: 1,
        };
        let eta = Eta::default();
        let line = 画进度行(&p, &eta, None, 100, Some(Duration::from_millis(400)));
        assert!(!line.contains("已 0 秒"), "别写「已 0 秒」：{line}");
        assert!(!line.contains("已 "), "不足一秒就别显示已用：{line}");
    }

    #[test]
    fn 窄终端先丢已用再丢剩余() {
        let p = ProgressView {
            done_bytes: 500,
            total_bytes: 1000,
            done_files: 0,
            total_files: 1,
        };
        let eta = Eta {
            speed_bps: Some(1_000_000.0),
            remaining: Some(Duration::from_secs(30)),
        };
        // 宽终端：已用和剩余都在
        let 宽 = 画进度行(&p, &eta, None, 200, Some(Duration::from_secs(12)));
        assert!(宽.contains("已 12 秒"), "宽终端要有已用：{宽}");
        assert!(宽.contains("剩 30 秒"), "宽终端要有剩余：{宽}");
        // 窄终端（40 列、没有文件名）：已用被丢，百分比保住
        let 窄 = 画进度行(&p, &eta, None, 40, Some(Duration::from_secs(12)));
        use unicode_width::UnicodeWidthStr;
        assert!(窄.width() <= 40, "40 列下画出了 {} 列：{窄}", 窄.width());
        assert!(!窄.contains("已 "), "窄下来该先丢已用：{窄}");
        assert!(窄.contains("50.0%"), "百分比不能丢：{窄}");
        // 属性断言（比「某个具体列数下丢谁」稳得多）：任何宽度下都
        // ①不超宽 ②「已用出现 ⇒ 文件名也一定在」—— 这就是「给文件名留 12 列」的保险丝
        for cols in [60usize, 80, 95, 100, 110, 120, 160] {
            let l = 画进度行(
                &p,
                &eta,
                Some("data/game.bin"),
                cols,
                Some(Duration::from_secs(12)),
            );
            assert!(l.width() <= cols, "{cols} 列超宽了：{l}");
            if l.contains("已 12 秒") {
                assert!(l.contains("game.bin"), "{cols} 列下有已用却没文件名：{l}");
            }
        }
    }
}
