//! 引擎 API 层：一次请求 → 一次结果
//!
//! 为什么单独一层：`--serve`（管道）、C ABI（iOS/Android）、将来的其它外壳只是
//! **不同的传输**；命令语义必须只有一份。传输层负责信封、线程、忙/取消，
//! 这里只负责「这条命令干什么」。
//!
//! 约定（与 PROTOCOL.md 一致）：
//! - 入参是已经解析好的 JSON 对象（`参数`）
//! - 成功返回 `Ok(Value)`，失败返回 `Err(错误对象)` —— **错误对象不带用户文案**，
//!   只有码 + 结构化 `detail`，句子归外壳查语言包
//! - 这里**不写 stdout、不碰进程**，事件只经 [`EventSink`] 出去

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::batch::BatchOptions;
use crate::ctx::UnpackCtx;
use crate::event::EventSink;
use crate::progress::CancelToken;

/// 协议版本（**只有破坏性改动才加 1**，见 PROTOCOL.md 的兼容性承诺）
pub const 协议版本: u64 = 1;

/// 引擎认识的全部命令（`hello.features` 与 PROTOCOL 命令表都从这里取）
pub const 命令列表: &[&str] = &[
    "hello", "cancel", "report", "scan", "list", "extract", "pack", "batch", "rescue", "answer",
    "skip",
];

/// 引擎会发出的事件种类（`hello.event_kinds` 从这里取）
pub const 事件种类: &[&str] = &[
    "JOB_STARTED",
    "JOB_FINISHED",
    "STAGE_STARTED",
    "STAGE_FINISHED",
    "SCAN_PROGRESS",
    "PREVIEW_READY",
    "EXTRACT_PROGRESS",
    "PACK_PROGRESS",
    "ENTRY_FINISHED",
    "PASSWORD_NEEDED",
    "PASSWORD_TRY",
    "SPACE_SHORTAGE",
    "ASK_USER",
    "RESUMED",
    "DIAGNOSTIC",
];

/// 统一入口：给 FFI 用（管道模式走 `serve.rs` 自己的忙/取消调度，但命令实现相同）
pub fn 执行(
    命令: &str,
    参数: &Value,
    事件: &Arc<dyn EventSink>,
    取消: &CancelToken,
    ask: Option<&Arc<crate::ask::AskGate>>,
) -> Result<Value, Value> {
    match 命令 {
        "hello" => Ok(hello()),
        "scan" => 扫描(参数),
        "list" => 列表(参数, 事件, 取消),
        "extract" => 解压(参数, 事件, 取消, ask),
        "pack" => 打包(参数, 事件),
        "batch" => 批量(参数, 事件, 取消, ask),
        "rescue" => 救援(参数, 事件, 取消),
        "report" => 诊断(参数, 事件),
        // 这两条是**传输层状态机**（要挂起等待 / 改当前任务），不属于单次命令
        "answer" | "skip" => Err(错误值(
            "UNSUPPORTED",
            "answer / skip 需要交互式传输（--serve），C ABI 请直接取消",
        )),
        other => Err(错误值("UNKNOWN_COMMAND", &format!("不认识的 cmd: {other}"))),
    }
}

/// 能力声明：前端据此决定显不显示某些功能（新增能力只加不删）
pub fn hello() -> Value {
    json!({
        "protocol": 协议版本,
        "engine": crate::VERSION,
        "features": 命令列表,
        "event_kinds": 事件种类,
        "languages": ["zh-CN", "zh-TW", "en-US", "ja-JP", "ko-KR", "ru-RU", "es-ES"],
        "caps": {
            "preview": true,
            "selective": true,
            // 断点续传做出来了，但**粒度是「条目之间」**：解到一半被打断，
            // 已完整解出的文件不再重解；正在写的那个文件要重来。
            // 这个词是从 itchio/savior 的三级定义借的（none / entry / block），
            // 用字符串而不是布尔，是因为将来真做了块级续传（peel 那种 frame checkpoint）
            // 只要把值改成 "block"，前端不用改代码就知道能力升级了。
            "resume": true,
            "resume_level": "entry",
            // 关掉窗口后引擎还在后台跑（藏到托盘继续解），这是真做出来了，
            // 所以能力位如实写 true —— 前端可以据此显示「后台继续」。
            "background": true
        }
    })
}

/// 扫一批路径（S13 队列 / S10 多候选的数据源）：
/// 展开目录 → 归并分卷 → 逐个认类型。只读文件头，不解压任何东西。
pub fn 扫描(参数: &Value) -> Result<Value, Value> {
    let paths: Vec<PathBuf> = 参数
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Err(错误值("BAD_PARAM", "缺少 paths"));
    }
    let 展开 = crate::batch::expand_inputs(&paths);
    let mut items = Vec::new();
    for p in &展开 {
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let 探 = crate::probe::probe_path(p).ok();
        let looks_like = 探
            .as_ref()
            .map(|x| format!("{:?}", x.kind))
            .unwrap_or_else(|| "Unknown".to_string());
        let mime = 探.as_ref().map(|x| x.mime).unwrap_or("application/octet-stream");
        let sink: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
        let ctx = UnpackCtx::new(0, sink);
        let (kind, volumes) = match crate::job::identify(p, &ctx) {
            Ok(t) => {
                let n = match &t {
                    crate::job::Target::SevenZ { volumes } => volumes.len(),
                    crate::job::Target::ZipSplit { volumes } => volumes.len(),
                    _ => 1,
                };
                (t.as_str().to_string(), n)
            }
            Err(_) => ("UNKNOWN".to_string(), 1),
        };
        items.push(json!({
            "path": p.display().to_string(),
            "name": p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            "size": size,
            "kind": kind,
            "volumes": volumes,
            "looks_like": looks_like,
            "mime": mime,
            "archive": kind != "UNKNOWN",
        }));
    }
    Ok(json!({"items": items, "expanded": 展开.len(), "given": paths.len()}))
}

/// 预览：只读索引，不解压
pub fn 列表(
    参数: &Value,
    事件: &Arc<dyn EventSink>,
    取消: &CancelToken,
) -> Result<Value, Value> {
    let 路径 = 参数
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| 错误值("BAD_PARAM", "缺少 path"))?;
    let 密码 = 参数.get("password").and_then(|v| v.as_str());
    let sink: Arc<dyn EventSink> = 事件.clone();
    let ctx = UnpackCtx::new(1, sink)
        .with_cancel(取消.clone())
        .with_encoding(
            参数
                .get("encoding")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        );

    let (_目标, pv) = crate::preview::preview(Path::new(路径), &ctx, 密码).map_err(错误对象)?;
    let mut 结果 = json!({
        "format": pv.format,
        "complete": pv.complete,
        "total_size": pv.total_size,
        "encrypted": pv.encrypted_count,
        // 加密方式与强度：zipcrypto / aes / aes256 / unknown（DESIGN §17.4）
        "enc_kind": pv.enc_kind,
        // EntryView 原样序列化：字段名以 src/event.rs 为准（契约里写了）
        "entries": pv.entries,
        "hints": pv.hints,
    });
    // 封面区大小（A② / §5.2①）：界面要把「伪装封面」当成清单里可勾选的一行，
    // 就必须在预览阶段知道它有多大 —— 否则勾选框只能写死一个占位。
    if let Ok(crate::job::Target::Zip { base, .. }) = crate::job::identify(Path::new(路径), &ctx)
        && let Some(cover) = crate::cover_extract::probe_zip_cover(Path::new(路径), base)
    {
        结果["cover"] = json!({
            "size": cover.size,
            "kind": cover.kind,
            "ext": cover.ext,
        });
    }
    Ok(结果)
}

/// 解压一个包
pub fn 解压(
    参数: &Value,
    事件: &Arc<dyn EventSink>,
    取消: &CancelToken,
    ask: Option<&Arc<crate::ask::AskGate>>,
) -> Result<Value, Value> {
    let 路径 = 参数
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| 错误值("BAD_PARAM", "缺少 path"))?;

    // 挑选（P23-1）：与 CLI `--select` 同一套语法，字符串或序号数组都认。
    // 非法表达式**报错**而不是静默全解 —— 用户以为只解了主程序、实际解了 16 GB（D20 同类教训）。
    let selection = 挑选条件(参数)?;

    // 「只提封面、不解压」：§5.2① 的「只勾封面」就是这条路径。
    // 必须**在建输出目录之前**返回 —— 用户只想要封面，不该留下空文件夹。
    let 要解压 = 参数.get("extract").and_then(|v| v.as_bool()).unwrap_or(true);
    let 要封面 = 参数.get("cover").and_then(|v| v.as_bool()).unwrap_or(true);

    let 大小 = std::fs::metadata(路径).map(|m| m.len()).unwrap_or(0);
    let mut job = crate::batch::Job::new(1, PathBuf::from(路径), 大小);
    if let Some(d) = 参数.get("dest").and_then(|v| v.as_str()) {
        job.dest = Some(PathBuf::from(d));
    }

    let opts = BatchOptions {
        encoding: 参数
            .get("encoding")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        jobs: 1, // 界面这边自己排队，引擎一次跑一个就够了
        continue_on_error: true,
        password: 参数
            .get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        selection,
        dest: job.dest.clone(),
        smart_dir: true,
        force: 参数.get("force").and_then(|v| v.as_bool()).unwrap_or(false),
        // 只测不写：界面/脚本可以只要「这个包完好么」而不要产物
        verify_only: 参数
            .get("verify_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        limits: crate::safety::Limits {
            ask: ask.cloned(),
            ..Default::default()
        },
    };

    let mut 结果 = if 要解压 {
        let sink: Arc<dyn EventSink> = 事件.clone();
        let out: crate::batch::JobOutcome = crate::job::run_job(&job, &opts, &sink, 取消);

        // 失败时返回 ok:false，并**带上结构化 detail**（如缺分卷的 missing/found）。
        //
        // 为什么重新识别一次：`JobOutcome` 只带 `Outcome::Failed(code)`，刻意不背 detail
        // （批量场景传不动）。identify 是纯读文件头，几毫秒，换来界面能说"缺 xxx.7z.003"。
        if let crate::error::Outcome::Failed(code) = &out.outcome {
            let sink2: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
            let ctx2 = UnpackCtx::new(1, sink2);
            if let Err(e) = crate::job::identify(Path::new(路径), &ctx2) {
                return Err(错误对象(e));
            }
            return Err(json!({
                "code": code.as_str(),
                "note": out.note,
            }));
        }
        // 取消 / 跳过不是失败：如实回「Cancelled」，让外壳显示"已停止"而不是"出错"
        if matches!(out.outcome, crate::error::Outcome::Cancelled { .. }) {
            let mut r = 结果json(&out);
            r["note"] = json!(out.note);
            return Ok(r);
        }
        结果json(&out)
    } else {
        // 只提封面：没有解压统计，给一个空壳，字段形状保持一致
        json!({
            "outcome": "Success",
            "entries_done": 0,
            "entries_total": 0,
            "bytes_written": 0,
            "dest": null,
            "millis": 0,
            "failed": [],
        })
    };

    // 封面分离（DESIGN §3 / §17.5）：解压成功后顺手把 `[0, 压缩层起点)` 切出来。
    // 失败一律忽略 —— 封面是附赠品，不能让主流程因为切不出封面就报错。
    if 要封面 {
        let sink0: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
        let ctx0 = UnpackCtx::new(1, sink0);
        // 封面也是产物：请求里给了 `dest` 就放那儿 —— 界面「只提取封面」正是这条路径，
        // 不给 dest 才退回「包旁边」（历史行为）。不认 dest 会让副本撒在源包旁边：
        // 只读目录直接失败，可写目录则越跑越多 `封面 (N).mp4`。
        let 封面目录: Option<PathBuf> = 参数
            .get("dest")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        if let Ok(crate::job::Target::Zip { base, .. }) =
            crate::job::identify(Path::new(路径), &ctx0)
            && let Ok(Some(c)) = crate::cover_extract::extract_zip_cover_to(
                Path::new(路径),
                base,
                封面目录.as_deref(),
            )
        {
            结果["cover"] = json!({
                "path": c.path.display().to_string(),
                "size": c.size,
                "kind": c.kind,
            });
        }
    }
    // 嵌套包自动下钻（组 C，只一层）：默认**关**，参数 `drill: true` 才做。
    // 为什么默认关：多解一层就是多写一倍的盘；用户没要求时不该替他做主。
    // 界面侧用「☑ 解完后自动继续解里面的包」控制。
    if 要解压
        && 参数.get("drill").and_then(|v| v.as_bool()).unwrap_or(false)
        && !取消.is_cancelled()
        && let Some(root) = 结果
            .get("dest")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
    {
        let sink: Arc<dyn EventSink> = 事件.clone();
        let 内层 = crate::drill::drill_once(
            &root,
            Some(Path::new(路径)),
            opts.password.as_deref(),
            opts.encoding.as_deref(),
            &sink,
            取消,
        );
        结果["nested"] = json!(内层
            .iter()
            .map(|d| json!({
                "path": d.path.display().to_string(),
                "dest": d.dest.as_ref().map(|p| p.display().to_string()),
                "outcome": d.outcome,
            }))
            .collect::<Vec<_>>());
    }
    Ok(结果)
}

/// 比较「是不是同一个文件」用的路径规范化
///
/// 输出文件通常还不存在，`canonicalize` 会失败 —— 那就把父目录规范化再拼文件名，
/// 这样 `./a.mp4` 和 `/home/me/a.mp4` 也能比出来。
fn 规范路径(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    let parent = p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = p.file_name().unwrap_or_default();
    match std::fs::canonicalize(parent) {
        Ok(c) => c.join(name),
        Err(_) => p.to_path_buf(),
    }
}

/// 打包（伪装 / 普通压缩共用一条实现）
/// 预扫输入：算出打包进度的分母（总字节 / 总文件数）
///
/// 打包本来就要把每个文件读一遍，这里多走一次目录代价很小；
/// 但能让界面从第一秒就有正确的百分比，而不是「写了 3 MB / 未知」。
fn 预扫输入(inputs: &[PathBuf]) -> (u64, u64) {
    fn 走(p: &Path, 字节: &mut u64, 文件: &mut u64) {
        let Ok(m) = std::fs::metadata(p) else {
            return;
        };
        if m.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    走(&e.path(), 字节, 文件);
                }
            }
        } else {
            *字节 = 字节.saturating_add(m.len());
            *文件 += 1;
        }
    }
    let (mut b, mut f) = (0u64, 0u64);
    for i in inputs {
        走(i, &mut b, &mut f);
    }
    (b, f.max(1))
}

/// 打包命令（协议 `pack`）
///
/// 三条交付路径：普通压缩（`.zip`）/ 伪装（外壳 + 压缩数据）/ 自解压（exe 或 .sh）。
/// 普通压缩这条会发 `PACK_PROGRESS`（N2），其余两条目前只报阶段事件。
pub fn 打包(参数: &Value, 事件: &Arc<dyn EventSink>) -> Result<Value, Value> {
    let 输入: Vec<PathBuf> = 参数
        .get("inputs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    if 输入.is_empty() {
        return Err(错误值("BAD_PARAM", "inputs 不能为空"));
    }
    let 输出 = 参数
        .get("out")
        .and_then(|v| v.as_str())
        .ok_or_else(|| 错误值("BAD_PARAM", "缺少 out"))?;

    // 打包模式（GUI-PLAN §5.2②）：普通压缩是默认语义，伪装是特殊功能。
    // 协议默认仍是 disguise —— 「只加不删」：老前端不传 mode，行为不能变。
    let 普通压缩 = 参数.get("mode").and_then(|v| v.as_str()) == Some("plain");
    // 自解压（第 23 批）：`sfx: true` / `"windows"` → exe；`"linux"` / `"sh"` → .sh 脚本。
    // 与伪装互斥；分卷、cover 也互斥（双击要是一个完整文件、外壳就是存根本身）。
    let 自解压: Option<&'static str> = match 参数.get("sfx") {
        Some(Value::Bool(true)) => Some("windows"),
        Some(Value::String(s)) if s == "windows" => Some("windows"),
        Some(Value::String(s)) if s == "linux" || s == "sh" => Some("linux"),
        _ => None,
    };
    if let Some(种类) = 自解压 {
        if 普通压缩 {
            return Err(错误值("BAD_PARAM", "普通压缩和自解压只能选一个"));
        }
        if 参数.get("split").and_then(|v| v.as_u64()).is_some() {
            return Err(错误值("BAD_PARAM", "自解压包不能分卷（双击要是一个完整文件）"));
        }
        if 参数.get("cover").and_then(|v| v.as_str()).is_some() {
            return Err(错误值("BAD_PARAM", "自解压不需要 cover（外壳就是存根）"));
        }
        let ext = Path::new(输出)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if 种类 == "windows" && ext != "exe" {
            return Err(错误值("BAD_PARAM", "Windows 自解压的输出文件必须以 .exe 结尾"));
        }
        if 种类 == "linux" && ext != "sh" {
            return Err(错误值(
                "BAD_PARAM",
                "Linux 自解压的输出文件必须以 .sh 结尾（才好双击/直接执行）",
            ));
        }
    }

    let 规范输出 = 规范路径(Path::new(输出));
    for i in &输入 {
        if 规范路径(i) == 规范输出 {
            return Err(错误值("BAD_PARAM", "输出文件不能覆盖要打包的源文件"));
        }
    }

    // 外壳文件（伪装模式）：任意类型，用户选什么就是什么。
    //
    // 之前协议没这个参数，界面只能拿三个内置小素材 —— 用户想伪装成 PDF / 安装包 /
    // 自己的猫图都没门。现在 `cover` 传路径，引擎原样读进来当外壳；
    // 布局由 `pack::build` 按文件头（魔数）决定，非 MP4 自动降到「尾部追加」。
    let mut 扩展名 = Path::new(输出)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    let mut 外壳字节: Option<Vec<u8>> = None;
    let mut 外壳路径: Option<String> = None;
    if !普通压缩 && 自解压.is_none() {
        let 外壳 = 参数
            .get("cover")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| 错误值("BAD_PARAM", "伪装打包要先选一个伪装文件"))?;
        let 外壳文件 = Path::new(外壳);
        if 规范路径(外壳文件) == 规范输出 {
            return Err(错误值(
                "BAD_PARAM",
                "输出文件不能就是伪装文件本身（会把外壳覆盖掉）",
            ));
        }
        let 数据 = std::fs::read(外壳文件)
            .map_err(|e| 错误值("PATH_NOT_FOUND", &format!("伪装文件读不了: {e}")))?;
        if 数据.is_empty() {
            return Err(错误值("BAD_PARAM", "伪装文件是空文件，没法当外壳"));
        }
        扩展名 = 外壳文件
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4")
            .to_ascii_lowercase();
        外壳字节 = Some(数据);
        外壳路径 = Some(外壳.to_string());
    }
    let 素材 = crate::pack::cover::BuiltinCover::for_extension(&扩展名);

    let mut opts = crate::pack::PackOptions {
        inputs: 输入,
        password: 参数
            .get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        cover: 外壳字节,
        cover_ext: 扩展名.clone(),
        ..Default::default()
    };
    // 压缩级别（P23-4）：0=仅存储（最快）/ 1-9 越大越小越慢，默认 6。
    // 越界明确报错 —— 静默忽略会让用户以为「选了最大压缩」其实没生效（D20 教训）。
    if let Some(v) = 参数.get("level") {
        let n = v
            .as_u64()
            .ok_or_else(|| 错误值("BAD_PARAM", "level 只能是 0-9 的整数"))?;
        if n > 9 {
            return Err(错误值("BAD_PARAM", &format!("level 只能是 0-9（收到 {n}）")));
        }
        opts.level = n as u32;
    }
    // 图片/文本内置素材不能用 MP4 的 free 原子（会写坏封面）—— 与 CLI 同一条规则。
    // 用户自带外壳不在这里判断：`pack::build` 会按魔数兜底降级。
    if 参数.get("tail").and_then(|v| v.as_bool()) == Some(true)
        || (opts.cover.is_none() && !素材.is_video())
    {
        opts.layout = crate::pack::PackLayout::TailAppend;
    }

    let 文件 = std::fs::File::create(输出)
        .map_err(|e| 错误值("PERMISSION_DENIED", &format!("建不了输出文件: {e}")))?;
    let mut bw = std::io::BufWriter::new(文件);
    // 打包进度（PACK_PROGRESS）：先按输入大小算分母，再逐块上报，结束补一条终态
    let ctx = crate::ctx::UnpackCtx::new(1, 事件.clone());
    let (总字节, 总文件) = 预扫输入(&opts.inputs);
    ctx.set_total(总字节, 总文件);

    // 三条打包路径共用同一个上报闭包。
    //
    // 以前只有「普通压缩」传了回调，伪装 / 自解压走的是空回调 ——
    // 打 2 GB 伪装包时界面进度条一动不动，用户以为卡死了（梯队四第 11 项）。
    // - `Wrote`：压缩阶段报「已读输入字节」，拼装阶段报「已写输出字节」
    // - `TotalHint`：压缩结束，把分母放大成「输入总字节 + 中间 ZIP」，
    //   进度条才能接着走而不是停在压缩阶段的百分比上
    let mut 报 = |t: crate::pack::PackTick| match t {
        crate::pack::PackTick::Wrote(n, 名字) => ctx.advance_pack(n, 名字.as_deref()),
        crate::pack::PackTick::TotalHint(额外) => ctx.set_total(总字节 + 额外, 总文件),
    };

    let rep = if 普通压缩 {
        crate::pack::build_plain_ctx(&opts, &mut bw, &mut 报)
    } else if 自解压 == Some("linux") {
        // Linux 自解压：shell 脚本 + ZIP（偏移写死在脚本里，不靠尾标）
        crate::pack::build_sfx_sh_ctx(&opts, &mut bw, &mut 报)
    } else if 自解压.is_some() {
        crate::pack::build_sfx_ctx(&opts, &mut bw, &mut 报)
    } else {
        crate::pack::build_ctx(&opts, &mut bw, &mut 报)
    }
    .map_err(错误对象)?;
    // 收尾补一条终态，保证界面看到的进度是最终值
    ctx.force_pack_progress(None);
    use std::io::Write;
    let _ = bw.flush();
    // Linux 自解压产物要能直接双击/执行
    #[cfg(unix)]
    if 自解压 == Some("linux") {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(输出, std::fs::Permissions::from_mode(0o755));
    }

    // 交付形态（DESIGN §14.2 / §17.8）：分卷 / 校验和 / 说明书。
    // 失败不改判 —— 包已经打好了。
    // 普通压缩默认不写说明书（用户验收 2026-10-17：普通压缩不写视频封面、不写说明书），
    // 但调用方显式要 readme:true 时照做；校验和默认开（这是通用的，不是伪装包专属）。
    let post = crate::pack_post::PostOptions {
        split: 参数.get("split").and_then(|v| v.as_u64()),
        checksum: 参数
            .get("checksum")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        readme: 参数
            .get("readme")
            .and_then(|v| v.as_bool())
            .unwrap_or(!普通压缩),
        password: 参数
            .get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let post_rep = crate::pack_post::postprocess(Path::new(输出), &post).ok();

    Ok(json!({
        "mode": if 普通压缩 { "plain" } else { "disguise" },
        "sfx": 自解压.is_some(),
        "sfx_kind": 自解压,
        "cover": 外壳路径,
        "total_size": rep.total_size,
        "zip_size": rep.zip_size,
        "cover_size": rep.cover_size,
        "entries": rep.entries,
        "level": opts.level,
        "layout": format!("{:?}", rep.layout),
        "warnings": rep.warnings,
        "volumes": post_rep.as_ref().map(|p| p.volumes.iter().map(|x| x.display().to_string()).collect::<Vec<_>>()).unwrap_or_default(),
        "checksum": post_rep.as_ref().and_then(|p| p.checksum.as_ref()).map(|x| x.display().to_string()),
        "readme": post_rep.as_ref().and_then(|p| p.readme.as_ref()).map(|x| x.display().to_string()),
    }))
}

/// 生成诊断包（报障用）
pub fn 诊断(参数: &Value, 事件: &Arc<dyn EventSink>) -> Result<Value, Value> {
    let opts = crate::report::ReportOptions {
        target: 参数
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from),
        out_dir: 参数
            .get("out_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            }),
        argv: std::env::args().collect(),
        hide_paths: 参数
            .get("no_paths")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };
    let sink: Arc<dyn EventSink> = 事件.clone();
    let ctx = UnpackCtx::new(1, sink);
    let p = crate::report::生成(&opts, &ctx).map_err(错误对象)?;
    Ok(json!({"path": p.display().to_string()}))
}

/// 批量执行：展开目录/归并分卷 → 排队 → 逐个跑（S13）
///
/// 为什么放在**一个** `batch` 命令里而不是前端循环调 `extract`：
/// 1. 分卷归并、目录展开是引擎的知识（`expand_inputs`），前端不该自己猜；
/// 2. 一次任务里取消能立刻停掉整个队列，而不是前端逐个 cancel 竞态；
/// 3. 每个任务的 `JOB_STARTED` / `JOB_FINISHED` 事件天然带任务号，界面据此更新队列行。
pub fn 批量(
    参数: &Value,
    事件: &Arc<dyn EventSink>,
    取消: &CancelToken,
    ask: Option<&Arc<crate::ask::AskGate>>,
) -> Result<Value, Value> {
    let paths: Vec<PathBuf> = 参数
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Err(错误值("BAD_PARAM", "缺少 paths"));
    }
    let dest = 参数
        .get("dest")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let password = 参数
        .get("password")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // 挑选（P23-1）：与单任务同一条规则 —— 界面选「只解这几个」时批量也照办
    let selection = 挑选条件(参数)?;
    let smart_dir = 参数
        .get("smart_dir")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let 展开 = crate::batch::expand_inputs(&paths);
    let 任务 = crate::batch::make_jobs(&展开);
    let cfg = crate::config::全局配置();
    let opts = BatchOptions {
        encoding: 参数
            .get("encoding")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        jobs: 1, // 顺序跑：队列的进度才有意义，也不会把机械盘打成碎片
        continue_on_error: true,
        password,
        selection,
        dest,
        smart_dir,
        force: false,
        // 批量也支持只测不写（协议 batch 命令）
        verify_only: 参数
            .get("verify_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        limits: crate::safety::Limits {
            max_ratio: cfg.safety.zip_bomb_ratio,
            warn_executables: cfg.safety.warn_executables,
            ask: ask.cloned(),
            ..Default::default()
        },
    };
    let sink: Arc<dyn EventSink> = 事件.clone();
    let started = std::time::Instant::now();
    let report = crate::batch::run_batch(任务, &opts, &sink, 取消, |job, o, s, c| {
        crate::job::run_job(job, o, s, c)
    });
    Ok(json!({
        "total": report.len(),
        "ok": report.succeeded(),
        "partial": report.partial(),
        "failed": report.failed(),
        "cancelled": report.cancelled(),
        "bytes": report.bytes_written(),
        "elapsed_ms": started.elapsed().as_millis() as u64,
        "jobs": report.jobs.iter().map(|j| json!({
            "id": j.id,
            "path": j.input.display().to_string(),
            "outcome": match &j.outcome {
                crate::error::Outcome::Success => "ok".to_string(),
                crate::error::Outcome::Partial { .. } => "partial".to_string(),
                crate::error::Outcome::Cancelled { .. } => "cancelled".to_string(),
                crate::error::Outcome::Failed(c) => format!("{c:?}"),
            },
            "bytes": j.stats.bytes_written,
            "entries": j.stats.entries_done,
            "elapsed_ms": j.stats.millis,
            "dest": j.stats.dest.as_ref().map(|d| d.display().to_string()),
        })).collect::<Vec<_>>(),
    }))
}

// ───────────────────────── 小工具 ─────────────────────────

/// 解析协议里的 `select`（P23-1 文件级挑选）
///
/// 两种情况都认：
/// - 字符串：与 CLI `--select` 完全同一套语法（`all` / `1,3,5` / `*.exe` / `目录/**` / `required`）
/// - 数组：界面勾选更顺手的写法（`[1, 3, 5]`，序号从 1 起，与 `PREVIEW_READY` 的 `index` 一致）
///
/// 缺省 / null / 空串 = 全都要（「只加不删」：老前端不传这个参数，行为不能变）。
fn 挑选条件(参数: &Value) -> Result<crate::select::Selection, Value> {
    match 参数.get("select") {
        None | Some(Value::Null) => Ok(crate::select::Selection::all()),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(crate::select::Selection::all()),
        Some(Value::String(s)) => crate::select::Selection::parse(s).map_err(|_| {
            错误值(
                "BAD_PARAM",
                &format!(
                    "select 表达式看不懂（{s}）；写法举例：all / 1,3,5 / *.exe / 目录/** / required"
                ),
            )
        }),
        Some(Value::Array(arr)) => {
            let mut ids: Vec<u64> = Vec::with_capacity(arr.len());
            for v in arr {
                match v.as_u64() {
                    Some(n) if n > 0 => ids.push(n),
                    _ => {
                        return Err(错误值(
                            "BAD_PARAM",
                            "select 数组只能放正整数序号（从 1 开始，与列表里的 index 一致）",
                        ));
                    }
                }
            }
            if ids.is_empty() {
                Ok(crate::select::Selection::all())
            } else {
                Ok(crate::select::Selection::only(ids))
            }
        }
        Some(_) => Err(错误值(
            "BAD_PARAM",
            "select 只接受字符串（表达式）或正整数数组（序号）",
        )),
    }
}

/// 参数类错误（没有引擎 detail 可带）
pub fn 错误值(码: &str, 备注: &str) -> Value {
    json!({"code": 码, "note": 备注, "detail": {}})
}

/// 引擎错误 → 协议错误对象
///
/// **`detail` 原样带出去**（`looks_like` / `ext_mismatch` / `size` …）：
/// 前端据此才能说"名字写的是 .mp4，内容其实是 ZIP —— 这很可能就是伪装包"。
/// 只给码 + 技术备注，**不给用户句子**（文案归前端查语言包）。
pub fn 错误对象(e: crate::Error) -> Value {
    let mut detail = serde_json::Map::new();
    for (k, v) in &e.detail {
        detail.insert(k.clone(), json!(v));
    }
    json!({
        "code": e.code.as_str(),
        "note": format!("{e:?}"),
        "detail": detail,
    })
}

/// 救援：索引（中央目录）坏了 / 被改掉时，按每个条目的 local header 重建清单并抢救数据。
///
/// 与 CLI `--rescue` 走同一批引擎函数（`scan_whole_file` → `rescue_by_locals` →
/// `extract_by_locals_ctx`），区别只是把「打印」换成「发事件 + 返回 JSON」。
///
/// 参数：
/// - `path`（必填）
/// - `dest`：解到哪；不给就 `<包名>.rescue`（与 CLI 一致，落在包旁边）
/// - `list_only: true`：只重建清单、不落盘（界面可以先说「能救回 N 个」再动手）
/// - `encoding`：文件名编码（同 `extract`）
///
/// 为什么单独一条命令、不让 `extract` 自动兜底：救援是**慢且可能缺东西**的路径，
/// 用户得知道自己在用它。界面把它放在失败页，意思是「已经解不开了，再试一把」。
pub fn 救援(参数: &Value, 事件: &Arc<dyn EventSink>, 取消: &CancelToken) -> Result<Value, Value> {
    let 路径 = 参数
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| 错误值("BAD_PARAM", "缺少 path"))?;
    let p = Path::new(路径);
    let 只列 = 参数.get("list_only").and_then(|v| v.as_bool()).unwrap_or(false);

    let dest: PathBuf = match 参数.get("dest").and_then(|v| v.as_str()) {
        Some(d) => PathBuf::from(d),
        None => {
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "救援结果".into());
            p.with_file_name(format!("{name}.rescue"))
        }
    };

    let mut f =
        std::fs::File::open(p).map_err(|e| 错误值("PATH_NOT_FOUND", &format!("打不开：{e}")))?;
    let size = f
        .metadata()
        .map_err(|e| 错误值("INTERNAL", &format!("读不到文件大小：{e}")))?
        .len();

    let sink: Arc<dyn EventSink> = 事件.clone();
    let ctx = UnpackCtx::new(1, sink)
        .with_cancel(取消.clone())
        .with_encoding(
            参数
                .get("encoding")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        );

    // ① 先找压缩层真正的起点（伪装包前面可能压着几十 MB 视频）
    let hits = crate::scan::scan_whole_file(&mut f, size, &crate::scan::ScanOptions::strict())
        .map_err(|e| 错误值("INTERNAL", &format!("扫描失败：{e}")))?;
    let start = hits
        .iter()
        .find(|h| h.format == crate::layout::ArchiveFormat::Zip)
        .map(|h| h.offset)
        .ok_or_else(|| {
            错误值(
                "NOT_ARCHIVE",
                "文件里找不到 ZIP 的痕迹 —— 救援只认得 ZIP 家族（7z/RAR 索引坏了暂时救不了）",
            )
        })?;

    // ② 按 local header 重建清单
    let items = crate::rescue::rescue_by_locals(
        &mut f,
        start,
        size,
        &crate::rescue::RescueOptions::default(),
    )
    .map_err(错误对象)?;

    if items.is_empty() {
        return Err(错误值(
            "DATA_LOST",
            "按文件头重建不出任何条目（数据区可能已经没了）",
        ));
    }

    let 清单: Vec<Value> = items
        .iter()
        .map(|e| {
            json!({
                "path": e.path,
                "size": e.size,
                "pack_size": e.pack_size,
                "method": e.method,
                // 通用标志位 bit0 = 加密；有的条目大小来自数据描述符（流式写入）
                "encrypted": e.flags & 1 != 0,
                "size_from_descriptor": e.size_from_descriptor,
            })
        })
        .collect();

    if 只列 {
        let 总数: u64 = items.iter().map(|e| e.size).sum();
        return Ok(json!({
            "list_only": true,
            "outcome": "Success",
            "entries_total": items.len(),
            "bytes_total": 总数,
            "dest": Value::Null,
            "files": [],
            "rebuilt": 清单,
            "skipped": [],
            "warnings": [],
            "cancelled": false,
        }));
    }

    // ③ 真把数据救出来（走与解压同一套安全限额 / 原子落盘）
    let eopts = crate::extract::ExtractOptions::new(&dest);
    let rep = crate::rescue::extract_by_locals_ctx(&mut f, start, size, &eopts, &ctx)
        .map_err(错误对象)?;

    if rep.files.is_empty() {
        return Err(错误值(
            "DATA_LOST",
            &format!("重建了 {} 个条目，但一个都没救出来", items.len()),
        ));
    }

    Ok(json!({
        "outcome": if rep.cancelled { "Cancelled" } else { "Success" },
        "entries_total": items.len(),
        "entries_done": rep.files.len(),
        "bytes_written": rep.bytes_written,
        "dest": dest.display().to_string(),
        "files": rep.files.iter().map(|f| json!({
            "path": f.out_path.display().to_string(),
            "name": f.name,
            "size": f.size,
        })).collect::<Vec<_>>(),
        "skipped": rep.skipped.iter()
            .map(|(n, why)| json!({"name": n, "reason": why}))
            .collect::<Vec<_>>(),
        "warnings": rep.warnings,
        "rebuilt": 清单,
        "cancelled": rep.cancelled,
    }))
}

/// 解压结果 → JSON（字段名是协议契约的一部分，别随手改）
pub fn 结果json(out: &crate::batch::JobOutcome) -> Value {
    json!({
        "outcome": out.outcome,
        "entries_done": out.stats.entries_done,
        "entries_total": out.stats.entries_total,
        "bytes_written": out.stats.bytes_written,
        "dest": out.stats.dest.as_ref().map(|p| p.display().to_string()),
        "millis": out.stats.millis,
        "failed": out.stats.failed.iter()
            .map(|(n, r, why)| json!({
                "path": n,
                "result": r.as_str(),
                "human": r.人话(),
                "reason": why,
            }))
            .collect::<Vec<_>>(),
    })
}
