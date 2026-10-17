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
    "hello", "cancel", "report", "scan", "list", "extract", "pack", "batch", "answer", "skip",
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
    "ENTRY_FINISHED",
    "PASSWORD_NEEDED",
    "PASSWORD_TRY",
    "SPACE_SHORTAGE",
    "ASK_USER",
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
        "pack" => 打包(参数),
        "batch" => 批量(参数, 事件, 取消, ask),
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
            "resume": false,
            "background": false
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

    // 挑选表达式暂时只支持「序号列表」；通配还没接进协议 —— 宁可报不支持，也不静默忽略
    if let Some(s) = 参数.get("select").and_then(|v| v.as_str())
        && !s.trim().is_empty()
    {
        return Err(错误值(
            "UNSUPPORTED",
            &format!("协议 v1 暂不支持 select（收到 {s}）"),
        ));
    }

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
        selection: crate::select::Selection::all(),
        dest: job.dest.clone(),
        smart_dir: true,
        force: 参数.get("force").and_then(|v| v.as_bool()).unwrap_or(false),
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
        if let Ok(crate::job::Target::Zip { base, .. }) =
            crate::job::identify(Path::new(路径), &ctx0)
            && let Ok(Some(c)) = crate::cover_extract::extract_zip_cover(Path::new(路径), base)
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

/// 打包（伪装 / 普通压缩共用一条实现）
pub fn 打包(参数: &Value) -> Result<Value, Value> {
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

    let 扩展名 = Path::new(输出)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    // 打包模式（GUI-PLAN §5.2②）：普通压缩是默认语义，伪装是特殊功能。
    // 协议默认仍是 disguise —— 「只加不删」：老前端不传 mode，行为不能变。
    let 普通压缩 = 参数.get("mode").and_then(|v| v.as_str()) == Some("plain");
    let 素材 = crate::pack::cover::BuiltinCover::for_extension(&扩展名);

    let mut opts = crate::pack::PackOptions {
        inputs: 输入,
        password: 参数
            .get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        cover_ext: 扩展名,
        ..Default::default()
    };
    // 图片/文本封面不能用 MP4 的 free 原子（会写坏封面）—— 与 CLI 同一条规则
    if 参数.get("tail").and_then(|v| v.as_bool()) == Some(true) || !素材.is_video() {
        opts.layout = crate::pack::PackLayout::TailAppend;
    }

    let 文件 = std::fs::File::create(输出)
        .map_err(|e| 错误值("PERMISSION_DENIED", &format!("建不了输出文件: {e}")))?;
    let mut bw = std::io::BufWriter::new(文件);
    let rep = if 普通压缩 {
        crate::pack::build_plain(&opts, &mut bw)
    } else {
        crate::pack::build(&opts, &mut bw)
    }
    .map_err(错误对象)?;
    use std::io::Write;
    let _ = bw.flush();

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
        "total_size": rep.total_size,
        "zip_size": rep.zip_size,
        "cover_size": rep.cover_size,
        "entries": rep.entries,
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
        selection: crate::select::Selection::all(),
        dest,
        smart_dir,
        force: false,
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
            .map(|(n, r)| json!({"path": n, "result": r.as_str()}))
            .collect::<Vec<_>>(),
    })
}
