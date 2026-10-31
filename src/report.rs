//! 诊断包 —— `--report` 一键收集报障信息（BACKEND-SPEC §4 的落地）
//!
//! # 为什么需要它
//! 用户说"我这个包解不开"，开发这边手上什么都没有：看不到他的文件、他的系统、
//! 他识别出来的偏移。靠来回问"你什么系统""它报什么错"效率极低 ——
//! 所以给一条命令，生成一个**可以直接贴出去**的小 zip。
//!
//! # 隐私红线（BACKEND-SPEC §4.3，必须守住；改动前先读这一节）
//! - **绝不包含**：文件内容（一个字节都不读）、任何密码、密码本内容、加密数据
//! - **路径默认包含**（便于定位问题）；`--no-paths` 时换成 `<path-1>` 这类占位符
//! - **README.txt 必须写清"包含什么 / 不包含什么"** —— 用户要敢把它贴到公开论坛
//!
//! # 与设计文档的一处偏差（设计这里有漏洞，已修正）
//! BACKEND-SPEC §4.2 写的是「meta.json 含**完整命令行**」。这条**不能照做**：
//! 用户完全可能用 `yshjy -p 我的解压码 包.mp4` 调用 —— 完整命令行就是明文密码。
//! 所以这里只记录**脱敏后的命令行**（`-p` / `--password` 的值一律换成
//! `<已抹掉>`），并在 README 里写明。**任何"收集环境信息"的功能都要先想一遍
//! 这件事**：命令行、环境变量、配置里都可能藏着凭据。

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 生成诊断包的参数
#[derive(Debug, Clone)]
pub struct ReportOptions {
    /// 要分析的包（`None` = 只收环境信息）
    pub target: Option<PathBuf>,
    /// 路径脱敏（要贴到公开的地方就用它）
    pub hide_paths: bool,
    /// 输出目录
    pub out_dir: PathBuf,
    /// 本次运行的原始命令行（**函数内部会脱敏后再写进包里**）
    pub argv: Vec<String>,
}

/// 生成诊断包，返回 zip 的完整路径
pub fn 生成(opts: &ReportOptions, ctx: &UnpackCtx) -> Result<PathBuf> {
    let mut 脱敏 = 脱敏器::new(opts.hide_paths);
    let 时间 = 时间戳();

    let 名字 = format!("yshjy-report-{时间}.zip");
    let 目标路径 = opts.out_dir.join(&名字);
    let 文件 = std::fs::File::create(&目标路径).map_err(|e| {
        Error::new(ErrorCode::PermissionDenied).context(format!("创建诊断包失败: {e}"))
    })?;
    let mut zip = zip::ZipWriter::new(文件);
    let 选项 = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        // 顺手写上时间戳：不写的话 zip 里条目全是 1980-01-01 00:00，
        // 别人打开会以为"这包从哪台古董机器上来的"（v0.8 打包侧踩过同款坑）
        .last_modified_time(zip时间());

    // ① README.txt —— **第一个写**：用户打开包最该先看到的就是"里面有什么"
   写一个(
        &mut zip,
        "README.txt",
        &readme(&时间, opts.hide_paths),
        选项,
    )?;

    // ② meta.json —— 环境与这次运行（命令行已脱敏）
    写一个(
        &mut zip,
        "meta.json",
        &meta_json(opts, &mut 脱敏)?.to_string(),
        选项,
    )?;

    // ③ findings.json —— 识别结果（没给目标就如实写"没给"）
    写一个(
        &mut zip,
        "findings.json",
        &findings_json(opts, ctx, &mut 脱敏)?.to_string(),
        选项,
    )?;

    // ④ config.snapshot.toml —— 当时生效的配置（**不含密码本内容**，只有行数）
    写一个(
        &mut zip,
        "config.snapshot.toml",
        &配置快照(&mut 脱敏),
        选项,
    )?;

    // ⑤ 最近的崩溃记录 —— 报障时**最有用的一件东西**，别让用户自己去找
    //
    // 为什么放在诊断包里：崩溃提示只能给一个绝对路径，普通用户要么找不到、
    // 要么复制粘贴丢细节。跟着 --report 一起走，一条命令就能把现场交出来。
    // --no-paths 时把家目录换成 <home>（日志里的路径是自由文本，套不了逐条映射）。
    let 崩溃 = 最近崩溃日志();
    if let Some((文件名, 内容)) = &崩溃 {
        let 内容 = if opts.hide_paths {
            抹掉家目录(内容)
        } else {
            内容.clone()
        };
        写一个(&mut zip, &format!("crash/{文件名}"), &内容, 选项)?;
    }

    zip.finish()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("收尾失败: {e}")))?;
    Ok(目标路径)
}

/// 状态目录（与崩溃日志一致：见 [`crate::paths::state_dir`]）
fn 状态目录() -> Option<PathBuf> {
    crate::paths::state_dir()
}

/// 最近一份崩溃日志（`crash-*.log`），返回（文件名, 内容）
///
/// 找不到就返回 `None` —— 诊断包照样生成，不因为"没有崩溃记录"而失败。
fn 最近崩溃日志() -> Option<(String, String)> {
    let dir = 状态目录()?.join("crash");
    let mut 候选: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".log"))
        })
        .collect();
    // 文件名带时间戳 → 排序即时间顺序，最后一份最新
    候选.sort();
    let 最新 = 候选.pop()?;
    let 内容 = std::fs::read_to_string(&最新).ok()?;
    let 名 = 最新.file_name()?.to_string_lossy().to_string();
    Some((名, 内容))
}

/// 把家目录换成 `<home>`（`--no-paths` 时用）
fn 抹掉家目录(文本: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => 文本.replace(&home, "<home>"),
        _ => 文本.to_string(),
    }
}

/// 往包里塞一个文本文件（统一入口：自动处理错误信息）
fn 写一个(
    zip: &mut zip::ZipWriter<std::fs::File>,
    名字: &str,
    内容: &str,
    选项: zip::write::SimpleFileOptions,
) -> Result<()> {
    zip.start_file(名字, 选项)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("写 {名字} 失败: {e}")))?;
    zip.write_all(内容.as_bytes())
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("写 {名字} 失败: {e}")))?;
    Ok(())
}

/// 路径脱敏器：同一个原始路径总是映射到同一个占位符
///
/// 为什么要映射表而不是"打码成 ****"：报障时开发者需要知道
/// "这两个路径其实是同一个"（比如包和它的说明文件在同一目录）——
/// 全打码就把这个信息也抹掉了。
struct 脱敏器 {
    开: bool,
    表: HashMap<String, String>,
    下一个: usize,
}

impl 脱敏器 {
    fn new(开: bool) -> Self {
        Self {
            开,
            表: HashMap::new(),
            下一个: 1,
        }
    }

    fn 路径(&mut self, p: &Path) -> String {
        let 原文 = p.display().to_string();
        if !self.开 {
            return 原文;
        }
        if let Some(占位) = self.表.get(&原文) {
            return 占位.clone();
        }
        let 占位 = format!("<path-{}>", self.下一个);
        self.下一个 += 1;
        self.表.insert(原文, 占位.clone());
        占位
    }

    /// 任意文本里的路径脱敏：映射表里的原路径换成占位符，家目录兜底换成 `<home>`。
    ///
    /// 为什么需要它：`findings.error` 是引擎的**错误原文**（`path=/home/…/坏文件.bin`），
    /// 不走 [`脱敏器::路径`] 那条路 —— 只脱 `target` 会让界面那句「路径已脱敏」变成
    /// 假话（2026-10-31 真机验收诊断包时撞到：包里明晃晃一串 /home/用户名）。
    fn 文本(&mut self, s: &str) -> String {
        if !self.开 {
            return s.to_string();
        }
        let mut 出 = s.to_string();
        // 长的先换：否则 `/home/x/包.mp4` 可能被 `/home/x/包` 先咬掉一半
        let mut 键: Vec<(&String, &String)> = self.表.iter().collect();
        键.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        for (原文, 占位) in 键 {
            出 = 出.replace(原文.as_str(), 占位.as_str());
        }
        // 表里没有的（比如错误原文里只出现了一部分路径）→ 至少把家目录抹掉
        抹掉家目录(&出)
    }

    /// 命令行脱敏：`-p xxx` / `--password xxx` / `--password=xxx` 的值一律抹掉
    ///
    /// 这是**最关键的一条**：完整命令行可能就是明文密码（见模块头那段说明）。
    /// 密码规则走 [`抹掉密码`]（与崩溃报告共用一份），这里只补上路径脱敏。
    fn 命令行(&mut self, argv: &[String]) -> Vec<String> {
        let 已抹密码 = 抹掉密码(argv);
        let mut 出 = Vec::with_capacity(已抹密码.len());
        for a in 已抹密码 {
            // 这几种是占位符 / 选项名 / 语言代码，原样保留（它们不敏感）
            if a == "<已抹掉>" || a.starts_with('-') || a.starts_with('<') || a == "已抹掉" {
                出.push(a);
                continue;
            }
            // 看着像路径的（含分隔符或带扩展名）→ 走脱敏；
            // 其余（语言代码 `zh-CN`、序号 `1,3`）原样保留。
            if a.contains('/') || a.contains('\\') || a.contains('.') {
                出.push(self.路径(Path::new(&a)));
                continue;
            }
            出.push(a);
        }
        出
    }
}

/// 只做「密码抹掉」的命令行脱敏 —— 崩溃报告与诊断包共用同一条红线
///
/// 与 [`脱敏器::命令行`] 的区别：这里**不脱路径**（崩溃报告要能看出是在哪个包上崩的），
/// 但 `-p` / `--password` 的值一律换成 `<已抹掉>`。
/// `脱敏器::命令行` 内部也调它 —— 密码规则只有一份，改一次两边都生效。
pub fn 抹掉密码(argv: &[String]) -> Vec<String> {
    let mut 出 = Vec::with_capacity(argv.len());
    let mut 吃掉下一个 = false;
    for a in argv {
        if 吃掉下一个 {
            出.push("<已抹掉>".to_string());
            吃掉下一个 = false;
            continue;
        }
        if a == "-p" || a == "--password" {
            出.push(a.clone());
            吃掉下一个 = true;
            continue;
        }
        if a.starts_with("--password=") {
            出.push("--password=<已抹掉>".to_string());
            continue;
        }
        出.push(a.clone());
    }
    出
}

/// 生成时间戳（本地时间，`YYYYMMDD-HHMMSS`）
///
/// 用返回 `(年,月,日,时,分,秒)` 的算法（`restore::unix_to_civil`，
/// Howard Hinnant 的 `civil_from_days`）—— 不为一个时间戳引入日期库。
fn 时间戳() -> String {
    let 秒 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (年, 月, 日, 时, 分, 秒数) = crate::restore::unix_to_civil_本地(秒);
    format!("{年:04}{月:02}{日:02}-{时:02}{分:02}{秒数:02}")
}

/// 当前时间 → zip 的 `DateTime`
///
/// zip 默认给条目写 1980-01-01（就是那个"打包从不写时间戳"的老毛病），
/// 诊断包虽然主要信息在 JSON 里，但条目时间戳错了会让人以为包很旧 —— 顺手写对。
fn zip时间() -> zip::DateTime {
    let 秒 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (年, 月, 日, 时, 分, 秒数) = crate::restore::unix_to_civil_本地(秒);
    zip::DateTime::from_date_and_time(年 as u16, 月 as u8, 日 as u8, 时 as u8, 分 as u8, 秒数 as u8)
        .unwrap_or_default()
}

/// 人类可读的生成时间
fn 可读时间() -> String {
    let 秒 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (年, 月, 日, 时, 分, 秒数) = crate::restore::unix_to_civil_本地(秒);
    format!("{年:04}-{月:02}-{日:02} {时:02}:{分:02}:{秒数:02}")
}

/// README.txt：把"包含什么 / 不包含什么"写在最前面
fn readme(时间: &str, 已脱敏: bool) -> String {
    let 路径说明 = if 已脱敏 {
        "路径**已脱敏**（换成了 <path-1> 这类占位符）—— 可以直接贴到公开的地方。"
    } else {
        "⚠️ 这个包里**包含文件路径**（含你的用户名/目录名）。\n\
         要贴到公开论坛/群里之前，请改用 `yshjy --report --no-paths` 重新生成一份。"
    };
    format!(
        "yshjy 诊断包\n\
         ============\n\
         \n\
         这是什么：yshjy（杨三海解压）在你机器上生成的一份「报障信息」，\n\
         用来让别人帮你看出问题出在哪。它**不改变、不上传**任何东西 ——\n\
         生成完就躺在你指定的目录里，发不发、发给谁，都由你决定。\n\
         \n\
         ✅ 里面包含：\n\
         \x20 - meta.json            版本 / 操作系统 / CPU 线程 / 内存与磁盘余量 /\n\
         \x20                          这次运行的参数（**密码已抹掉**）/\n\
         \x20                          是否设了代理环境变量（**只记有没有设，不记值**）\n\
         \x20 - findings.json        如果给了包：识别结果（外壳类型、格式、偏移、\n\
         \x20                          条目数量、有几条加密）—— **不含文件名清单**\n\
         \x20 - config.snapshot.toml 当时生效的配置（**不含密码本内容**，只记几行）\n\
         \x20 - crash/               最近一次崩溃的现场（**只有崩溃过才有**；\n\
         \x20                          含崩溃位置与调用栈，密码已抹掉）\n\
         \x20 - 本文件\n\
         \n\
         ❌ 里面**不包含**：\n\
         \x20 - 你的文件内容（一个字节都没有读进来）\n\
         \x20 - 任何密码 / 密钥 / 加密数据（命令行里的 -p 值已抹成 <已抹掉>）\n\
         \x20 - 密码本内容（只记「有 N 行」）\n\
         \x20 - 网络请求、账号、cookie（本程序里也没有这些东西）\n\
         \n\
         ⚠️ 关于路径：\n\
         {路径说明}\n\
         \n\
         生成时间：{时间}\n\
         \n\
         怎么用：把整个 zip 发给帮你排查的人（或贴到 issue 里），说一句\n\
         「这是 yshjy 的诊断包」即可。\n",
    )
}

/// meta.json：环境 + 这次运行（命令行已脱敏）
fn meta_json(opts: &ReportOptions, 脱敏: &mut 脱敏器) -> Result<Value> {
    let 硬件 = crate::hardware::Hardware::detect();

    // 磁盘余量：直接问文件系统。
    // ⚠️ 别借 `safety::check_space(dir, 0)` —— 它对 `need == 0` 直接返回 `Unknown`
    //    （源码第一行就是 `if need == 0 { return SpaceCheck::Unknown }`）。
    //    实测踩过：诊断包里 `disk_available_bytes` 一直是 null。
    // 目标目录可能还不存在 → 往上找第一个存在的祖先（都在同一个磁盘上）。
    let 磁盘可用 = {
        let mut 探针 = opts.out_dir.clone();
        while !探针.exists() {
            match 探针.parent() {
                Some(p) if !p.as_os_str().is_empty() => 探针 = p.to_path_buf(),
                _ => break,
            }
        }
        fs4::available_space(&探针).ok()
    };

    // 代理环境变量：**只记"有没有设"，不记值** ——
    // `http://user:pass@host` 这种值本身就带凭据，记下来就是泄露
    let 代理: Vec<String> = ["http_proxy", "https_proxy", "all_proxy", "no_proxy"]
        .into_iter()
        .filter(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
        .map(|k| k.to_string())
        .collect();

    Ok(json!({
        "yshjy_version": crate::VERSION,
        "generated_at": 可读时间(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hardware": 硬件.summary(),
        "disk_available_bytes": 磁盘可用,
        "argv": 脱敏.命令行(&opts.argv),
        "proxy_env_set": 代理,
        "paths_hidden": opts.hide_paths,
        "privacy_note": "命令行里的密码值已抹成 <已抹掉>；代理变量只记名字、不记值",
    }))
}

/// findings.json：识别结果（没有给目标就如实写"没给"）
fn findings_json(opts: &ReportOptions, ctx: &UnpackCtx, 脱敏: &mut 脱敏器) -> Result<Value> {
    let Some(目标) = &opts.target else {
        return Ok(json!({
            "target": null,
            "note": "这次没给包（只收集了环境信息）。想分析某个包：yshjy --report 那个包.mp4",
        }));
    };

    let 大小 = std::fs::metadata(目标).map(|m| m.len()).ok();
    let 脱敏路径 = 脱敏.路径(目标);

    match crate::job::identify(目标, ctx) {
        Ok(target) => {
            let 识别 = 描述识别(&target, 脱敏);
            // 预览（列清单）失败不算错 —— 有些格式/损坏包本来就列不出来
            let 预览 = crate::preview::preview(目标, ctx, None).ok().map(|(_t, pv)| {
                json!({
                    "format": pv.format,
                    "entry_count": pv.entries.len(),
                    "file_count": pv.file_count(),
                    "encrypted_count": pv.encrypted_count,
                    "total_unpacked_bytes": pv.total_size,
                    "list_complete": pv.complete,
                    "hints": format!("{:?}", pv.hints),
                })
            });
            Ok(json!({
                "target": 脱敏路径,
                "size_bytes": 大小,
                "identified": 识别,
                "preview": 预览,
                "privacy_note": "文件名清单**故意不含**（只给数量）—— 免得泄露你下载了什么",
            }))
        }
        Err(e) => Ok(json!({
            "target": 脱敏路径,
            "size_bytes": 大小,
            "identified": null,
            "error": 脱敏.文本(&format!("{e}")),
            "privacy_note": "识别失败本身也是结果 —— 报障时通常正是要看这个",
        })),
    }
}

/// 把识别结果翻译成 JSON（偏移是关键信息：伪装包的问题几乎都在偏移上）
fn 描述识别(t: &crate::job::Target, 脱敏: &mut 脱敏器) -> Value {
    use crate::job::Target as T;
    match t {
        T::Zip { base, len } => json!({
            "kind": "zip",
            "base_offset": base,
            "data_len": len,
        }),
        T::ZipSplit { volumes } => json!({
            "kind": "zip_split",
            "volume_count": volumes.len(),
            "volumes": volumes.iter().map(|p| 脱敏.路径(p)).collect::<Vec<_>>(),
        }),
        T::SevenZ { volumes } => json!({
            "kind": "7z",
            "volume_count": volumes.len(),
            "volumes": volumes.iter().map(|p| 脱敏.路径(p)).collect::<Vec<_>>(),
        }),
        T::Rar { volumes } => json!({ "kind": "rar", "volumes": volumes.len() }),
        T::Stream {
            format,
            is_tar,
            packed,
        } => json!({
            "kind": "stream",
            "format": format.as_str(),
            "is_tar": is_tar,
            "packed_bytes": packed,
        }),
    }
}

/// 语言那行的值：`None` 要写成能读懂的东西
///
/// 直接 `{:?}` 会输出 Rust 的 `None`，那不是合法的 TOML 值（快照虽然只给人看，
/// 但眼睛扫到 `None` 会以为出错了）。
fn 语言行(cfg: &crate::config::Config) -> String {
    match &cfg.general.language {
        Some(l) => format!("{l:?}"),
        None => "\"(跟随系统)\"".to_string(),
    }
}

/// config.snapshot.toml：当时生效的配置（**不含密码本内容**，只记行数）
fn 配置快照(脱敏: &mut 脱敏器) -> String {
    let cfg = crate::config::全局配置();
    let 密码本行数 = cfg.passwords().len();
    let 配置目录 = cfg
        .passwords_path()
        .parent()
        .map(|p| 脱敏.路径(p))
        .unwrap_or_else(|| "<未知>".to_string());
    let 输出目录 = cfg
        .output_dir()
        .map(|p| format!("{:?}", 脱敏.路径(&p)))
        .unwrap_or_else(|| "null".to_string());

    format!(
        "# yshjy 配置快照（脱敏）\n\
         # ⚠️ 不含密码本内容 —— 只记「有几行」。密码本在：{配置目录}\n\
         \n\
         [general]\n\
         language = {}\n\
         jobs = {}\n\
         \n\
         [password]\n\
         file = {:?}\n\
         auto_from_notes = {}\n\
         max_candidates = {}\n\
         password_book_lines = {}   # 只记行数，不记内容\n\
         \n\
         [output]\n\
         dir = {}\n\
         conflict = {:?}\n\
         \n\
         [safety]\n\
         zip_bomb_ratio = {}\n\
         warn_executables = {}\n",
        语言行(cfg),
        cfg.general.jobs,
        cfg.password.file,
        cfg.password.auto_from_notes,
        cfg.password.max_candidates,
        密码本行数,
        输出目录,
        cfg.output.conflict,
        cfg.safety.zip_bomb_ratio,
        cfg.safety.warn_executables,
    )
}

#[cfg(test)]
mod 脱敏测试 {
    use super::脱敏器;

    /// 2026-10-31 真机验收诊断包时撞出来的真 bug：
    /// `findings.json` 里 `target` 脱成了 `<path-2>`，`error` 却是完整的
    /// `/home/用户名/…/坏文件.bin` —— 界面写着「路径已脱敏」，包里明晃晃一串，
    /// 而这包正是要贴到群里/发给别人的。
    #[test]
    fn 错误原文里的家目录要脱掉() {
        let home = std::env::var("HOME").expect("测试机总有 HOME");
        let mut 脱敏 = 脱敏器::new(true);
        let 出 = 脱敏.文本(&format!(
            "[NOT_ARCHIVE] path={home}/下载/坏文件.bin looks_like=text"
        ));
        assert!(!出.contains(&home), "错误原文里泄漏了家目录: {出}");
        assert!(出.contains("<home>"), "应当换成 <home>: {出}");
    }

    /// 同一个路径在 `target` 和 `error` 里必须映射到**同一个**占位符 ——
    /// 否则报障时看不出「这两个其实是同一个文件」
    #[test]
    fn 同一个路径在不同字段里的占位符一致() {
        let mut 脱敏 = 脱敏器::new(true);
        let 路径 = std::path::Path::new("/tmp/我的包.mp4");
        let 占位 = 脱敏.路径(路径);
        let 文案 = 脱敏.文本("失败位置: /tmp/我的包.mp4");
        assert!(文案.contains(&占位), "应当复用 {占位}: {文案}");
        assert!(!文案.contains("/tmp/我的包.mp4"), "原文不该原样留下: {文案}");
    }

    /// 关闭脱敏（不带 `--no-paths`）时必须原样保留 —— 别帮倒忙，
    /// 开发者要能看出问题出在哪个具体文件上
    #[test]
    fn 关掉脱敏时原样保留() {
        let mut 脱敏 = 脱敏器::new(false);
        let home = std::env::var("HOME").unwrap_or_default();
        let 原文 = format!("path={home}/x.bin");
        assert_eq!(脱敏.文本(&原文), 原文);
    }
}
