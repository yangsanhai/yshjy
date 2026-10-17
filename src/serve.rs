//! `--serve`：把引擎包成「一行一个 JSON」的双向管道
//!
//! 契约见 `docs/PROTOCOL.md`。这一层存在的唯一目的：
//! **让界面（Flutter / Tauri / 脚本）只依赖协议，不依赖引擎内部** ——
//! 引擎怎么重构、界面怎么换，只要协议不动，两边可以各自升级。
//!
//! # 线程模型
//! ```text
//! stdin → 读线程 → 命令队列 → 主线程（唯一写 stdout 的人）
//!                                 ↑ 事件（ChannelSink）/ 任务结果（mpsc）
//!                        任务线程（一次只跑一个，跑完才接下一个）
//! ```
//! 为什么只有一个写者：多个线程同时写一行 JSON 会互相插成半行，
//! 前端解析会随机炸 —— 这类 bug 极难复现，所以从结构上杜绝。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use yshjy::batch::{BatchOptions, Job, JobOutcome};
use yshjy::ctx::UnpackCtx;
use yshjy::event::{ChannelSink, EventSink};
use yshjy::progress::CancelToken;

/// 协议版本（**只有破坏性改动才加 1**，见 PROTOCOL.md 的兼容性承诺）
pub const 协议版本: u64 = 1;

/// 运行服务，返回进程退出码
pub fn 运行() -> u8 {
    let (命令发, 命令收) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for 行 in stdin.lock().lines() {
            match 行 {
                Ok(l) if !l.trim().is_empty() => {
                    if 命令发.send(l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    let (结果发, 结果收) = mpsc::channel::<Value>();
    let 事件 = ChannelSink::new(16_384);
    let mut 当前: Option<(u64, CancelToken, std::thread::JoinHandle<()>)> = None;

    let stdout = std::io::stdout();
    let mut 出 = stdout.lock();

    loop {
        // ① 先吐事件：进度事件在队列里堆着不输出，界面就会"卡住不动"
        for ev in 事件.drain() {
            写行(
                &mut 出,
                &json!({"type": "event", "kind": ev.kind(), "event": ev}),
            );
        }
        // ② 再吐任务结果
        while let Ok(v) = 结果收.try_recv() {
            写行(&mut 出, &v);
        }
        // ③ 收命令（30ms 一轮，保证事件输出及时）
        match 命令收.recv_timeout(Duration::from_millis(30)) {
            Ok(文本) => 处理命令(&文本, &mut 出, &事件, &结果发, &mut 当前),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        // ④ 任务跑完了就把句柄放掉（否则一直显示"忙"）
        if let Some((_, _, 句柄)) = &当前
            && 句柄.is_finished()
        {
            当前 = None;
        }
    }

    // 收尾：让还在跑的任务自己停下（最多等 3 秒，别把前端吊死）
    if let Some((_, 取消, 句柄)) = 当前 {
        取消.cancel_with("serve shutting down");
        let _ = 句柄.join();
    }
    0
}

fn 写行(出: &mut impl Write, v: &Value) {
    // 写失败基本只有一种原因：前端把管道关了。那就安静退出，别 panic。
    let _ = writeln!(出, "{v}");
    let _ = 出.flush();
}

fn 处理命令(
    文本: &str,
    出: &mut impl Write,
    事件: &Arc<ChannelSink>,
    结果发: &mpsc::Sender<Value>,
    当前: &mut Option<(u64, CancelToken, std::thread::JoinHandle<()>)>,
) {
    let 值: Value = match serde_json::from_str(文本) {
        Ok(v) => v,
        Err(e) => {
            写行(出, &错误响应(0, "BAD_COMMAND", &format!("JSON 解析失败: {e}")));
            return;
        }
    };
    let id = 值.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    // 转成 String：下面要把它 move 进任务线程，借用 `值` 会活不过 'static
    let cmd = 值.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // 一次只跑一个任务：并发解压同一个磁盘只会互相拖慢，且界面也只有一个进度面板
    let 忙 = 当前.as_ref().is_some_and(|(_, _, h)| !h.is_finished());
    if 忙 && !matches!(cmd.as_str(), "cancel" | "hello" | "report") {
        写行(
            出,
            &错误响应(id, "BUSY", "上一个任务还没结束：先发 cancel，或等它的结果"),
        );
        return;
    }

    match cmd.as_str() {
        "hello" => 写行(出, &ok(id, hello())),
        "cancel" => {
            if let Some((_, 取消, _)) = 当前 {
                取消.cancel_with("api cancel");
            }
            写行(出, &ok(id, json!({"cancelled": true})));
        }
        // 生成诊断包（报障用）：界面点一下就出 zip，不用教用户开终端
        "report" => {
            let opts = yshjy::report::ReportOptions {
                target: 值
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from),
                hide_paths: 值.get("no_paths").and_then(|v| v.as_bool()).unwrap_or(false),
                out_dir: 值
                    .get("out_dir")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    }),
                argv: std::env::args().collect(),
            };
            let sink: Arc<dyn EventSink> = 事件.clone();
            let ctx = UnpackCtx::new(1, sink);
            match yshjy::report::生成(&opts, &ctx) {
                Ok(p) => 写行(出, &ok(id, json!({"path": p.display().to_string()}))),
                Err(e) => 写行(出, &json!({"id": id, "ok": false, "error": 错误对象(e)})),
            }
        }
        // 扫一批路径（S13 队列 / S10 多候选的数据源）：
        // 展开目录 → 归并分卷 → 逐个认类型。只读文件头，不解压任何东西。
        "scan" => {
            let paths: Vec<PathBuf> = 值
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
                写行(出, &错误响应(id, "BAD_PARAM", "缺少 paths"));
                return;
            }
            let 展开 = yshjy::batch::expand_inputs(&paths);
            let mut items = Vec::new();
            for p in &展开 {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let 探 = yshjy::probe::probe_path(p).ok();
                let looks_like = 探
                    .as_ref()
                    .map(|x| format!("{:?}", x.kind))
                    .unwrap_or_else(|| "Unknown".to_string());
                let mime = 探.as_ref().map(|x| x.mime).unwrap_or("application/octet-stream");
                let sink: Arc<dyn EventSink> = Arc::new(yshjy::event::NullSink);
                let ctx = UnpackCtx::new(0, sink);
                let (kind, volumes) = match yshjy::job::identify(p, &ctx) {
                    Ok(t) => {
                        let n = match &t {
                            yshjy::job::Target::SevenZ { volumes } => volumes.len(),
                            yshjy::job::Target::ZipSplit { volumes } => volumes.len(),
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
            写行(
                出,
                &ok(
                    id,
                    json!({"items": items, "expanded": 展开.len(), "given": paths.len()}),
                ),
            );
        }
        "list" | "extract" | "pack" | "batch" => {
            let 参数 = 值.clone();
            let 事件 = 事件.clone();
            let 发 = 结果发.clone();
            let 取消 = CancelToken::new();
            let 取消给任务 = 取消.clone();
            let 句柄 = std::thread::spawn(move || {
                let 结果 = match cmd.as_str() {
                    "list" => 做list(&参数, &事件, &取消给任务),
                    "extract" => 做extract(&参数, &事件, &取消给任务),
                    "batch" => 做batch(&参数, &事件, &取消给任务),
                    _ => 做pack(&参数),
                };
                let 响应 = match 结果 {
                    Ok(v) => ok(id, v),
                    // 引擎的 detail 原样带出去：前端据此说"你给的是个 MP4"
                    Err(错误) => json!({"id": id, "ok": false, "error": 错误}),
                };
                let _ = 发.send(响应);
            });
            *当前 = Some((id, 取消, 句柄));
        }
        other => 写行(
            出,
            &错误响应(id, "UNKNOWN_COMMAND", &format!("不认识的 cmd: {other}")),
        ),
    }
}

// ───────────────────────── 各命令的实现 ─────────────────────────

fn hello() -> Value {
    json!({
        "protocol": 协议版本,
        "engine": yshjy::VERSION,
        "features": ["list", "extract", "pack", "cancel", "report", "scan", "batch"],
        "event_kinds": [
            "JOB_STARTED", "JOB_FINISHED", "STAGE_STARTED", "STAGE_FINISHED",
            "SCAN_PROGRESS", "PREVIEW_READY", "EXTRACT_PROGRESS", "ENTRY_FINISHED",
            "PASSWORD_NEEDED", "SPACE_SHORTAGE", "DIAGNOSTIC"
        ],
        "languages": ["zh-CN", "zh-TW", "en-US", "ja-JP", "ko-KR", "ru-RU", "es-ES"],
        // 前端据此决定要不要显示某些功能（新增能力只加不删）
        "caps": {
            "preview": true,
            "selective": true,
            "resume": false,
            "background": false
        }
    })
}

fn 做list(
    参数: &Value,
    事件: &Arc<ChannelSink>,
    取消: &CancelToken,
) -> Result<Value, Value> {
    let 路径 = 参数
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| 错误值("BAD_PARAM", "缺少 path"))?;
    let 密码 = 参数.get("password").and_then(|v| v.as_str());
    let sink: Arc<dyn EventSink> = 事件.clone();
    let ctx = UnpackCtx::new(1, sink).with_cancel(取消.clone());

    let (_目标, pv) = yshjy::preview::preview(Path::new(路径), &ctx, 密码).map_err(错误对象)?;
    Ok(json!({
        "format": pv.format,
        "complete": pv.complete,
        "total_size": pv.total_size,
        "encrypted": pv.encrypted_count,
        // 加密方式与强度：zipcrypto / aes / aes256 / unknown（DESIGN §17.4）
        "enc_kind": pv.enc_kind,
        // EntryView 原样序列化：字段名以 src/event.rs 为准（契约里写了）
        "entries": pv.entries,
        "hints": pv.hints,
    }))
}

fn 做extract(
    参数: &Value,
    事件: &Arc<ChannelSink>,
    取消: &CancelToken,
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

    let 大小 = std::fs::metadata(路径).map(|m| m.len()).unwrap_or(0);
    let mut job = Job::new(1, PathBuf::from(路径), 大小);
    if let Some(d) = 参数.get("dest").and_then(|v| v.as_str()) {
        job.dest = Some(PathBuf::from(d));
    }

    let opts = BatchOptions {
        jobs: 1, // 界面这边自己排队，引擎一次跑一个就够了
        continue_on_error: true,
        password: 参数
            .get("password")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        selection: yshjy::select::Selection::all(),
        dest: job.dest.clone(),
        smart_dir: true,
        force: 参数.get("force").and_then(|v| v.as_bool()).unwrap_or(false),
        limits: yshjy::safety::Limits::default(),
    };

    let sink: Arc<dyn EventSink> = 事件.clone();
    let out: JobOutcome = yshjy::job::run_job(&job, &opts, &sink, 取消);

    // 失败时返回 ok:false，并**带上结构化 detail**（如缺分卷的 missing/found）。
    //
    // 为什么重新识别一次：`JobOutcome` 只带 `Outcome::Failed(code)`，刻意不背 detail
    // （批量场景传不动）。identify 是纯读文件头，几毫秒，换来界面能说"缺 xxx.7z.003"。
    if let yshjy::error::Outcome::Failed(code) = &out.outcome {
        let sink2: Arc<dyn EventSink> = Arc::new(yshjy::event::NullSink);
        let ctx2 = UnpackCtx::new(1, sink2);
        if let Err(e) = yshjy::job::identify(Path::new(路径), &ctx2) {
            return Err(错误对象(e));
        }
        return Err(json!({
            "code": code.as_str(),
            "note": out.note,
        }));
    }
    let mut 结果 = 结果json(&out);

    // 封面分离（DESIGN §3 / §17.5）：解压成功后顺手把 `[0, 压缩层起点)` 切出来。
    // 默认开（参数 `cover: false` 可关）；失败一律忽略 —— 封面是附赠品，
    // 不能让主流程因为切不出封面就报错。
    if 参数.get("cover").and_then(|v| v.as_bool()).unwrap_or(true) {
        let sink0: Arc<dyn EventSink> = Arc::new(yshjy::event::NullSink);
        let ctx0 = UnpackCtx::new(1, sink0);
        if let Ok(yshjy::job::Target::Zip { base, .. }) =
            yshjy::job::identify(Path::new(路径), &ctx0)
            && let Ok(Some(c)) = yshjy::cover_extract::extract_zip_cover(Path::new(路径), base)
        {
            结果["cover"] = json!({
                "path": c.path.display().to_string(),
                "size": c.size,
                "kind": c.kind,
            });
        }
    }
    Ok(结果)
}

fn 做pack(参数: &Value) -> Result<Value, Value> {
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
    let 素材 = yshjy::pack::cover::BuiltinCover::for_extension(&扩展名);

    let mut opts = yshjy::pack::PackOptions {
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
        opts.layout = yshjy::pack::PackLayout::TailAppend;
    }

    let 文件 = std::fs::File::create(输出)
        .map_err(|e| 错误值("PERMISSION_DENIED", &format!("建不了输出文件: {e}")))?;
    let mut bw = std::io::BufWriter::new(文件);
    let rep = yshjy::pack::build(&opts, &mut bw).map_err(错误对象)?;
    use std::io::Write;
    let _ = bw.flush();

    Ok(json!({
        "total_size": rep.total_size,
        "zip_size": rep.zip_size,
        "cover_size": rep.cover_size,
        "entries": rep.entries,
        "layout": format!("{:?}", rep.layout),
        "warnings": rep.warnings,
    }))
}

// ───────────────────────── 小工具 ─────────────────────────

fn ok(id: u64, v: Value) -> Value {
    json!({"id": id, "ok": true, "result": v})
}

fn 错误响应(id: u64, 码: &str, 备注: &str) -> Value {
    json!({"id": id, "ok": false, "error": {"code": 码, "note": 备注}})
}

/// 参数类错误（没有引擎 detail 可带）
fn 错误值(码: &str, 备注: &str) -> Value {
    json!({"code": 码, "note": 备注, "detail": {}})
}

/// 引擎错误 → 协议错误对象
///
/// **`detail` 原样带出去**（`looks_like` / `ext_mismatch` / `size` …）：
/// 前端据此才能说"名字写的是 .mp4，内容其实是 ZIP —— 这很可能就是伪装包"。
/// 只给码 + 技术备注，**不给用户句子**（文案归前端查语言包）。
fn 错误对象(e: yshjy::Error) -> Value {
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

fn 结果json(out: &JobOutcome) -> Value {
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

/// 批量执行：展开目录/归并分卷 → 排队 → 逐个跑（S13）
///
/// 为什么放在**一个** `batch` 命令里而不是前端循环调 `extract`：
/// 1. 分卷归并、目录展开是引擎的知识（`expand_inputs`），前端不该自己猜；
/// 2. 一次任务里取消能立刻停掉整个队列，而不是前端逐个 cancel 竞态；
/// 3. 每个任务的 `JOB_STARTED` / `JOB_FINISHED` 事件天然带任务号，界面据此更新队列行。
fn 做batch(
    参数: &Value,
    事件: &Arc<ChannelSink>,
    取消: &CancelToken,
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
    let dest = 参数.get("dest").and_then(|v| v.as_str()).map(PathBuf::from);
    let password = 参数
        .get("password")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let smart_dir = 参数
        .get("smart_dir")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let 展开 = yshjy::batch::expand_inputs(&paths);
    let 任务 = yshjy::batch::make_jobs(&展开);
    let cfg = yshjy::config::全局配置();
    let opts = yshjy::batch::BatchOptions {
        jobs: 1, // 顺序跑：队列的进度才有意义，也不会把机械盘打成碎片
        continue_on_error: true,
        password,
        selection: yshjy::select::Selection::all(),
        dest,
        smart_dir,
        force: false,
        limits: yshjy::safety::Limits {
            max_ratio: cfg.safety.zip_bomb_ratio,
            warn_executables: cfg.safety.warn_executables,
            ..Default::default()
        },
    };
    let sink: Arc<dyn EventSink> = 事件.clone();
    let started = std::time::Instant::now();
    let report = yshjy::batch::run_batch(任务, &opts, &sink, 取消, |job, o, s, c| {
        yshjy::job::run_job(job, o, s, c)
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
                yshjy::error::Outcome::Success => "ok".to_string(),
                yshjy::error::Outcome::Partial { .. } => "partial".to_string(),
                yshjy::error::Outcome::Cancelled { .. } => "cancelled".to_string(),
                yshjy::error::Outcome::Failed(c) => format!("{c:?}"),
            },
            "bytes": j.stats.bytes_written,
            "entries": j.stats.entries_done,
            "elapsed_ms": j.stats.millis,
            "dest": j.stats.dest.as_ref().map(|d| d.display().to_string()),
        })).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 响应信封形状稳定() {
        let v = ok(7, json!({"x": 1}));
        assert_eq!(v["id"], 7);
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"]["x"], 1);

        let e = 错误响应(8, "NOT_ARCHIVE", "note");
        assert_eq!(e["id"], 8);
        assert_eq!(e["ok"], false);
        assert_eq!(e["error"]["code"], "NOT_ARCHIVE");
    }

    #[test]
    fn hello_带协议版本() {
        let h = hello();
        assert_eq!(h["protocol"], 协议版本);
        assert_eq!(h["engine"], yshjy::VERSION);
        assert!(h["features"].as_array().unwrap().contains(&json!("extract")));
    }

    #[test]
    fn 非压缩包的错误里带着真实类型() {
        // 造一个"名字是 .mp4、内容其实是 ZIP"的文件 —— 伪装包的典型样子
        let dir = std::env::temp_dir().join(format!("yshjy_serve_mis_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let 假 = dir.join("伪装.mp4");
        std::fs::write(&假, b"PK\x03\x04not a real zip but signature is enough").unwrap();

        let 事件 = ChannelSink::new(64);
        let 取消 = CancelToken::new();
        let e = 做list(&json!({"path": 假.to_str().unwrap()}), &事件, &取消).unwrap_err();
        assert_eq!(e["code"], "NOT_ARCHIVE", "{e}");
        let d = &e["detail"];
        assert_eq!(d["looks_like"], "zip", "要认出内容其实是 ZIP：{d}");
        assert_eq!(d["ext"], "mp4");
        assert_eq!(d["ext_mismatch"], "true", "名字与内容不符必须标出来：{d}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 缺参数报_bad_param_而不是崩() {
        let 事件 = ChannelSink::new(16);
        let 取消 = CancelToken::new();
        let e = 做list(&json!({}), &事件, &取消).unwrap_err();
        assert_eq!(e["code"], "BAD_PARAM");
    }
}

// ───────────────────────── 批量 ─────────────────────────
