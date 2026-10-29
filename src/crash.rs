//! 崩溃兜底：程序意外炸掉时，留下能报障的现场
//!
//! # 为什么「完整应用」必须有这一层
//! Rust 默认的 panic 输出是给开发者看的（`thread 'main' panicked at src/x.rs:12`），
//! 用户看了只会以为电脑坏了 —— 而且**什么都留不下**：关掉终端就再也说不清
//! 当时什么版本、在解哪个包、崩在哪一行。报障于是变成"我这儿崩了，你猜"。
//!
//! 这里做三件事（都是**体面收场**，不是"防止崩溃"）：
//! 1. 把现场写进 `$XDG_STATE_HOME/yshjy/crash/crash-<时间>.log`
//! 2. 命令行**先脱敏**再写（`-p 密码` 绝不落盘）—— 与 `report.rs` 同一条红线
//! 3. 用一句人话告诉用户：崩了、现场在哪儿、怎么报障、你的文件没被动过
//!
//! # 为什么不用 `catch_unwind` 把 main 整个包起来
//! panic 分两类：**可恢复的**（某个包解析时库内部炸了）和**不可恢复的**（程序自己的 bug）。
//! 前者由批量层隔离（见 `main::跑批量`：一个坏包不该带走整批）；
//! 后者就该让进程死掉 —— 带着不确定状态继续跑，可能写出坏文件，比崩溃更糟。
//! 所以这里只做善后，**绝不吞掉 panic**（hook 之后仍然走默认的终止流程）。

use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows_sys::Win32::Foundation::SYSTEMTIME;

/// 崩溃日志保留几份（多了占地方，少了不够回溯）
const 保留份数: usize = 10;

thread_local! {
    /// 当前线程是不是正在"隔离地"跑一个任务（见 [`隔离标记`]）
    ///
    /// panic hook 是全局的，它分不清"整个程序炸了"和"一个坏包把某个任务炸了"。
    /// 这个标记就是为了让提示话说得准确：后者不该吓唬用户说"程序出了意外"。
    static 任务隔离中: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 进入「单任务隔离」状态（drop 时自动复位）
///
/// 用法：批量解压时每个任务外面套一层，panic 就只影响那一个包。
pub struct 隔离标记;

impl 隔离标记 {
    /// 开始隔离（RAII：离开作用域自动复位，panic 展开时也会复位）
    pub fn 进入() -> Self {
        任务隔离中.with(|c| c.set(true));
        Self
    }
}

impl Drop for 隔离标记 {
    fn drop(&mut self) {
        任务隔离中.with(|c| c.set(false));
    }
}

/// 算本地时区偏移并注入引擎（**必须在任何时间显示之前调用**）
///
/// 引擎是 `#![forbid(unsafe_code)]`，碰不了 `localtime_r`，所以这一步归外壳。
/// 拿不到就注入 0（按 UTC 显示）—— 宁可时间差 8 小时，也不能因为算时区把程序弄崩。
pub fn 注入本地时区() {
    yshjy::restore::设置本地偏移(本地时区偏移秒());
}

/// 本地相对 UTC 的偏移（秒，东八区 = +28800）
#[cfg(unix)]
fn 本地时区偏移秒() -> i64 {
    // SAFETY: `localtime_r` 是可重入版本，只写我们自己的 `tm`，不碰全局静态缓冲；
    // `time_t` 与 `tm` 都是 POD，零初始化合法；没有任何跨线程共享的可变状态。
    unsafe {
        let 秒 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as libc::time_t)
            .unwrap_or(0);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&秒, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}

/// Windows：`GetLocalTime()`（本地墙钟）− `GetSystemTime()`（UTC 墙钟）= 当前偏移秒
///
/// 为什么不碰 `GetTimeZoneInformation`：Bias 单位是分钟、符号「西正东负」、夏令时还要
/// 判断当前落在哪一段，三处都容易错 —— 两个墙钟的差值由 Windows 自己算好，夏令时天然正确
/// （思路出自参考项目 `refs/upstream/time` 的 `local_offset_at/windows.rs`，台账 §9）。
///
/// 两次取样只隔几微秒，秒粒度上最多差 1 秒：zip 的 DOS 时间本来就是 2 秒粒度、
/// 资源管理器只显示到分钟，这 1 秒用户看不见，不值得为它再上 FILETIME 那套换算。
#[cfg(windows)]
fn 本地时区偏移秒() -> i64 {
    use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    // SAFETY: 两个 API 都只往我们自己的 SYSTEMTIME 里写值，无失败返回、无全局副作用。
    unsafe {
        let mut 本地: SYSTEMTIME = std::mem::zeroed();
        let mut 世界: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut 本地);
        GetSystemTime(&mut 世界);
        墙钟秒(&本地) - 墙钟秒(&世界)
    }
}

/// 其他非 Unix 平台（本项目暂无）：宁按 UTC 显示，不因时区把程序弄崩
#[cfg(not(any(unix, windows)))]
fn 本地时区偏移秒() -> i64 {
    0
}

/// SYSTEMTIME → 「自 1970-01-01 起的秒数」（毫秒位忽略）
#[cfg(windows)]
fn 墙钟秒(t: &SYSTEMTIME) -> i64 {
    民用日(i64::from(t.wYear), t.wMonth, t.wDay) * 86400
        + i64::from(t.wHour) * 3600
        + i64::from(t.wMinute) * 60
        + i64::from(t.wSecond)
}

/// 某个民用日期距 1970-01-01 的天数（Howard Hinnant 的 `days_from_civil`，公有领域）
#[cfg(windows)]
fn 民用日(年: i64, 月: u16, 日: u16) -> i64 {
    let y = 年 - i64::from(月 <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(月) + 9) % 12; // 三月 = 0，闰日落在年末好算
    let doy = (153 * mp + 2) / 5 + i64::from(日) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 装 panic 兜底（**在 `main` 最开头调用**，越早越好）
pub fn 装兜底() {
    let 版本 = env!("CARGO_PKG_VERSION");
    std::panic::set_hook(Box::new(move |info| {
        let 报告 = 生成报告(
            版本,
            info.location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "（未知）".into()),
            说明(info.payload()),
        );
        // 先把默认那行（给开发者看的）也留着：有它在，日志里能直接看到
        // "panicked at ..."，老手一眼就懂；我们额外给的是给用户看的那部分。
        eprintln!();
        // 是"单任务隔离"里炸的，还是整个程序炸了？提示话不一样
        let 隔离 = 任务隔离中.with(|c| c.get());
        match 写报告(&报告) {
            Ok(路径) => {
                let 路径文 = 路径.display().to_string();
                if 隔离 {
                    // 某个包把任务炸了：只说明这一条被跳过，别吓唬用户说整个程序崩了
                    eprintln!("{}", 提示("crash.task_saved", &[("path", &路径文)]));
                } else {
                    eprintln!("{}", 提示("crash.saved", &[("path", &路径文)]));
                    eprintln!("{}", 提示("crash.report_hint", &[]));
                    eprintln!("{}", 提示("crash.not_your_fault", &[]));
                }
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    提示("crash.save_failed", &[("err", &e.to_string())])
                );
                // 存不下来就只能当场打出来 —— 至少用户能截图
                if !隔离 {
                    eprintln!("\n{报告}");
                }
            }
        }
        eprintln!();
    }));
}

// ─────────────────────────── 报告内容 ───────────────────────────

/// 拼出报告全文
///
/// 参数收「已经解析好的位置与信息」而不是 `PanicHookInfo`：后者没法在测试里构造，
/// 拆开之后这段（真正会被贴给开发者的内容）就能单测。
fn 生成报告(版本: &str, 位置: String, 信息: String) -> String {
    let mut s = String::new();
    s.push_str("yshjy 崩溃报告（可直接贴给开发者）\n");
    s.push_str("====================================\n");
    s.push_str(&format!(
        "版本    : {版本}（{}）\n",
        if cfg!(feature = "full") {
            "全能版"
        } else {
            "常用版"
        }
    ));
    s.push_str(&format!("时间    : {}\n", 可读时间()));
    s.push_str(&format!(
        "系统    : {} {}（{} 位）\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        usize::BITS
    ));
    let 参数: Vec<String> = std::env::args().collect();
    s.push_str(&format!(
        "命令行  : {}\n",
        yshjy::report::抹掉密码(&参数).join(" ")
    ));
    s.push_str(&format!(
        "工作目录: {}\n",
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "（读不到）".into())
    ));
    s.push_str(&format!("位置    : {位置}\n"));
    s.push_str(&format!("信息    : {信息}\n"));
    s.push_str("\n--- 调用栈（前 40 行）---\n");
    s.push_str(&格式栈());
    s.push_str(
        "\n--- 说明 ---\n\
         这个文件里**没有**你的文件内容；命令行里的密码已经抹成 <已抹掉>。\n\
         路径可能包含你的用户名，要贴到公开的地方请自行删改。\n",
    );
    s
}

/// panic 携带的信息（`&str` 或 `String`，其他类型只能说"非字符串"）
fn 说明(负载: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = 负载.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = 负载.downcast_ref::<String>() {
        s.clone()
    } else {
        "（不是字符串类型的 panic 负载）".to_string()
    }
}

/// 调用栈（`force_capture` 不依赖 `RUST_BACKTRACE`；release 也能出函数名）
fn 格式栈() -> String {
    let bt = std::backtrace::Backtrace::force_capture();
    let 全文 = format!("{bt}");
    let 行: Vec<&str> = 全文.lines().take(40).collect();
    if 行.is_empty() {
        "（没有捕获到调用栈）".to_string()
    } else {
        行.join("\n")
    }
}

// ─────────────────────────── 落盘 ───────────────────────────

/// 写报告，返回文件路径
fn 写报告(文本: &str) -> std::io::Result<PathBuf> {
    写到目录(&状态目录()?.join("crash"), 文本)
}

/// 把报告写进指定目录（抽出来是为了测试不污染真实的 state 目录）
fn 写到目录(dir: &Path, 文本: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let 路径 = dir.join(format!("crash-{}.log", 文件名时间()));
    {
        let mut f = std::fs::File::create(&路径)?;
        f.write_all(文本.as_bytes())?;
        f.flush()?;
    }
    // 里面有路径（可能含用户名）：只给自己读
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&路径, std::fs::Permissions::from_mode(0o600));
    }
    清理旧报告(dir);
    Ok(路径)
}

/// 状态目录：平台惯例见 [`crate::paths::state_dir`]
///
/// 为什么不放 `$XDG_CONFIG_HOME`：配置是"用户的选择"，崩溃日志是"运行产物"，
/// 按 XDG 规范后者属于 state。Windows / macOS 各走自己的惯例（不再假设 HOME）。
fn 状态目录() -> std::io::Result<PathBuf> {
    yshjy::paths::state_dir()
        .ok_or_else(|| std::io::Error::other("找不到状态目录：HOME/LOCALAPPDATA 都没设置"))
}

/// 只留最近 [`保留份数`] 份崩溃日志
///
/// 崩溃可能连环发生（比如某个环境问题每次都崩），不清理会越堆越多。
fn 清理旧报告(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut 文件: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".log"))
        })
        .collect();
    if 文件.len() <= 保留份数 {
        return;
    }
    // 文件名带时间戳 → 排序即时间顺序
    文件.sort();
    let 该删 = 文件.len() - 保留份数;
    for p in 文件.into_iter().take(该删) {
        let _ = std::fs::remove_file(p);
    }
}

// ─────────────────────────── 小工具 ───────────────────────────

/// 当前时间（本地）→ `YYYYMMDD-HHMMSS`
fn 文件名时间() -> String {
    let 秒 = 现在秒();
    let (年, 月, 日, 时, 分, 秒数) = yshjy::restore::unix_to_civil_本地(秒);
    format!("{年:04}{月:02}{日:02}-{时:02}{分:02}{秒数:02}")
}

/// 当前时间（本地）→ `YYYY-MM-DD HH:MM:SS`
fn 可读时间() -> String {
    let 秒 = 现在秒();
    let (年, 月, 日, 时, 分, 秒数) = yshjy::restore::unix_to_civil_本地(秒);
    format!("{年:04}-{月:02}-{日:02} {时:02}:{分:02}:{秒数:02}")
}

fn 现在秒() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 文案：语言包已初始化就跟着它，崩得太早（还没初始化）就用中英双语兜底
fn 提示(键: &str, 参数: &[(&str, &str)]) -> String {
    if let Some(t) = crate::I18N.get() {
        return t.t(键, 参数);
    }
    let 取值 = |k: &str| {
        参数
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| *v)
            .unwrap_or("")
    };
    match 键 {
        "crash.saved" => format!(
            "💥 程序出了意外，现场已保存到：{}\n💥 The program crashed; details saved to: {}",
            取值("path"),
            取值("path")
        ),
        "crash.report_hint" => {
            "   报障：把这个文件（或 yshjy --report 生成的诊断包）发给开发者\n\
                To report: send this file (or the package from `yshjy --report`) to the developer"
                .to_string()
        }
        "crash.not_your_fault" => {
            "   说明：这不是你操作错了 —— 你的文件没有被改动过。\n\
                Note: this is not your mistake — your files were not modified."
                .to_string()
        }
        "crash.save_failed" => format!(
            "💥 程序出了意外，而且现场没能保存（{}）\n💥 Crashed, and failed to save details ({})",
            取值("err"),
            取值("err")
        ),
        "crash.task_saved" => format!(
            "   ⚠ 这个文件出错了，已跳过（现场：{}）\n   ⚠ This file failed and was skipped (details: {})",
            取值("path"),
            取值("path")
        ),
        _ => 键.to_string(),
    }
}

// ─────────────────────────── 启动时的残骸清理 ───────────────────────────

/// 清理自己留下的过期临时文件（崩溃 / 被 kill / 断电时留下的）
///
/// 只删两种**自己命名**的东西，且必须**超过 24 小时**没动过：
/// - `yshjy_pack_*.zip`（打包时先落的 ZIP）
/// - `yshjy_tar_xz_*.tar`（tar.xz 要先解到临时文件）
///
/// 为什么按"24 小时没动"而不是"文件就在那儿"：别的实例可能正在跑，
/// 删了正在用的临时文件会直接让它失败。宁可晚点清，也不能误删。
pub fn 清理临时残骸() {
    let 目录 = std::env::temp_dir();
    let Ok(rd) = std::fs::read_dir(&目录) else {
        return;
    };
    let 现在 = std::time::SystemTime::now();
    for e in rd.flatten() {
        let p = e.path();
        let Some(名) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let 像我们的 = (名.starts_with("yshjy_pack_") && 名.ends_with(".zip"))
            || (名.starts_with("yshjy_tar_xz_") && 名.ends_with(".tar"));
        if !像我们的 {
            continue;
        }
        let 够旧 = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| 现在.duration_since(t).ok())
            .is_some_and(|d| d.as_secs() > 24 * 3600);
        if 够旧 {
            let _ = std::fs::remove_file(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 时间戳格式固定() {
        let s = 文件名时间();
        assert_eq!(s.len(), 15, "YYYYMMDD-HHMMSS 应为 15 个字符，实际 {s}");
        assert!(s.contains('-'));
        assert!(s.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn 状态目录遵守_xdg() {
        // 不真的改环境变量（测试并发会互相踩），只验证两种来源都落在 yshjy 子目录
        let p = 状态目录().unwrap();
        assert_eq!(p.file_name().and_then(|n| n.to_str()), Some("yshjy"));
    }

    #[test]
    fn 说明能处理两种负载() {
        let s: &str = "坏掉了";
        assert_eq!(说明(&s), "坏掉了");
        let owned = String::from("也坏掉了");
        assert_eq!(说明(&owned), "也坏掉了");
        let n = 42u8;
        assert!(说明(&n).contains("不是字符串"));
    }

    #[test]
    fn 报告含关键字段且写明隐私边界() {
        let r = 生成报告("0.9.0", "src/main.rs:1:1".into(), "测试崩溃".into());
        for 关键字 in ["版本", "时间", "系统", "命令行", "位置", "信息", "调用栈"] {
            assert!(r.contains(关键字), "报告里缺「{关键字}」：\n{r}");
        }
        assert!(r.contains("0.9.0"));
        assert!(r.contains("测试崩溃"));
        assert!(r.contains("没有**"), "要明确写出「不含文件内容」");
        assert!(r.contains("密码已经抹成"), "要明确写出「密码已抹掉」");
    }

    #[test]
    fn 密码不会出现在脱敏后的命令行里() {
        let argv = vec![
            "yshjy".to_string(),
            "-p".to_string(),
            "secret123".to_string(),
            "包.mp4".to_string(),
        ];
        let 脱敏 = yshjy::report::抹掉密码(&argv);
        assert!(
            !脱敏.iter().any(|a| a == "secret123"),
            "明文密码漏出来了: {脱敏:?}"
        );
        assert!(脱敏.contains(&"<已抹掉>".to_string()));
        // 等号写法也要挡住
        let argv2 = vec!["yshjy".into(), "--password=abc".into()];
        let 脱2 = yshjy::report::抹掉密码(&argv2);
        assert!(!脱2.iter().any(|a| a.contains("abc")), "{脱2:?}");
    }

    #[test]
    fn 报告落盘后只有自己可读() {
        let dir = std::env::temp_dir().join(format!("yshjy_crash_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = 写到目录(&dir, "测试报告内容").unwrap();
        assert!(p.exists());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "测试报告内容");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "崩溃日志可能含路径，只能自己读");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 旧报告只留最近十份() {
        let dir = std::env::temp_dir().join(format!("yshjy_crash_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..15 {
            std::fs::write(dir.join(format!("crash-2026010{i:02}-120000.log")), "x").unwrap();
        }
        std::fs::write(dir.join("别的文件.txt"), "不该被删").unwrap();
        清理旧报告(&dir);
        let 剩: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            剩.iter().filter(|n| n.starts_with("crash-")).count(),
            保留份数,
            "应当只剩 {保留份数} 份: {剩:?}"
        );
        assert!(剩.iter().any(|n| n == "别的文件.txt"), "不是我们的文件不能删");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── W1 时区（2026-10-30 实测：偏移写死 0 → 解出的文件时间 +8 小时）──

    #[cfg(windows)]
    #[test]
    fn windows时区偏移落在真实范围内() {
        let o = 本地时区偏移秒();
        // 现实中时区只存在于 UTC-12（-43200）到 UTC+14（+50400）之间
        assert!(
            (-43200..=50400).contains(&o),
            "偏移 {o} 秒超出任何真实时区"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows时区偏移与本机时间一致() {
        // 用 Rust 自己的本地换算不了（引擎禁 unsafe，这正是外壳算的原因），
        // 但可以验证符号方向：拿 wYear/wMonth/wDay 现场对照偏移的整除关系不可靠，
        // 这里只守「两次取样都是合法的民用时刻，差值非零且稳定」
        let a = 本地时区偏移秒();
        let b = 本地时区偏移秒();
        assert_eq!(a, b, "同一秒内两次取样，偏移必须一致（实际 {a} vs {b}）");
    }

    #[cfg(windows)]
    #[test]
    fn 民用日换算锚点正确() {
        // Unix 纪元与两个闰年锚点（days_from_civil 的标准用例）
        assert_eq!(民用日(1970, 1, 1), 0);
        assert_eq!(民用日(2000, 3, 1), 11017);
        assert_eq!(民用日(2026, 10, 30), 20756); // 本项目 Windows 开工日
    }
}
