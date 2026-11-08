// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 杨三海解压引擎的**稳定 C ABI**（v1）
//!
//! 为什么单独一个 crate 而不是塞进 `src/`：引擎本体是 `#![forbid(unsafe_code)]`，
//! 而 C ABI 必然要碰裸指针。把 unsafe 全部关在这一层，引擎的零 unsafe 承诺不被破坏。
//!
//! 设计原则（与 `docs/PROTOCOL.md` 同构）：
//! - **JSON 进、JSON 出**：命令语义只有一份（`yshjy::api`），这里不重复实现
//! - **事件走回调**：与管道模式的 NDJSON 行内容完全一样
//! - **跨线程安全**：`submit` 立即返回，`wait` 阻塞，`cancel` 可从任意线程调
//! - **绝不 unwind 穿过 FFI**：worker 里 `catch_unwind`，panic 变成 `PANIC` 错误
//!
//! 完整契约与调用示例见 `docs/FFI.md` + `ffi/include/yshjy.h`。

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use serde_json::{Value, json};

use yshjy::api;
use yshjy::event::EventSink;
use yshjy::progress::CancelToken;

/// C ABI 版本。**加函数不改它；改已有函数语义才 +1。**
pub const ABI_VERSION: u32 = 1;

/// 事件回调：`json/len` 是一条完整的 JSON（UTF-8，不是 C 字符串，**可能不含结尾 0**）
///
/// 回调在**引擎的工作线程**上被调用；宿主自己保证线程安全。
pub type YshjyEventCb = Option<unsafe extern "C" fn(json: *const c_char, len: usize, user: *mut c_void)>;

/// 把 C 回调包成引擎的 [`EventSink`]
struct CbSink {
    cb: unsafe extern "C" fn(*const c_char, usize, *mut c_void),
    user: *mut c_void,
}

// SAFETY: 回调指针与 user 由宿主提供并保证在 job 生命周期内有效；
// 真实调用发生在单个 worker 线程上（引擎一次只跑一个任务），
// 因此「跨线程 Send/Sync」在这里是宿主契约的一部分，已在 FFI.md 写明。
unsafe impl Send for CbSink {}
unsafe impl Sync for CbSink {}

impl EventSink for CbSink {
    fn emit(&self, ev: &yshjy::event::Event) {
        let 行 = json!({"type": "event", "kind": ev.kind(), "event": ev}).to_string();
        // 事件行里不会有 NUL；万一有（自定义路径），截断也比 panic 好
        let Ok(c) = CString::new(行) else {
            return;
        };
        // SAFETY: c 在调用期间有效；回调按契约不得长期持有该指针。
        unsafe { (self.cb)(c.as_ptr(), c.as_bytes().len(), self.user) };
    }
}

/// 一个后台任务
pub struct Job {
    取消: CancelToken,
    结果: Mutex<Option<mpsc::Receiver<String>>>,
    缓存: Mutex<Option<String>>,
    句柄: Mutex<Option<JoinHandle<()>>>,
}

impl Job {
    fn 完成(响应: String) -> *mut Job {
        let (_tx, rx) = mpsc::channel();
        // 直接塞进缓存，等待时立刻返回
        Box::into_raw(Box::new(Job {
            取消: CancelToken::new(),
            结果: Mutex::new(Some(rx)),
            缓存: Mutex::new(Some(响应)),
            句柄: Mutex::new(None),
        }))
    }
}

/// ABI 版本（宿主启动时先问这个，不匹配就别调后面的）
///
/// # Safety
/// 无参数、无副作用，任何线程可调。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_abi_version() -> u32 {
    ABI_VERSION
}

/// 引擎版本（如 `0.9.20-c`）。返回的字符串用 [`yshjy_string_free`] 释放。
///
/// # Safety
/// 返回值可能为 NULL（内存耗尽）；调用方必须检查。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_version() -> *mut c_char {
    转_c字符串(yshjy::VERSION.to_string())
}

/// `hello`：协议版本 / 命令表 / 事件种类 / 能力（与管道模式同一份实现）
///
/// # Safety
/// 返回值用 [`yshjy_string_free`] 释放。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_hello() -> *mut c_char {
    转_c字符串(api::hello().to_string())
}

/// 同步执行一条请求（内部会阻塞到任务结束）。**不需要**取消就用这个。
///
/// 入参是 `{"cmd":"list","path":"..."}` 这样的 JSON；
/// 返回 `{"ok":true,"result":...}` 或 `{"ok":false,"error":{...}}`。
///
/// # Safety
/// - `json` 必须是有效的、以 0 结尾的 UTF-8 字符串
/// - 返回值用 [`yshjy_string_free`] 释放
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_request(json: *const c_char) -> *mut c_char {
    let Some(文本) = 读_c字符串(json) else {
        return 转_c字符串(错误响应("BAD_PARAM", "json 指针为空或不是 UTF-8"));
    };
    let 值: Value = match serde_json::from_str(&文本) {
        Ok(v) => v,
        Err(e) => return 转_c字符串(错误响应("BAD_COMMAND", &format!("JSON 解析失败: {e}"))),
    };
    let 事件: Arc<dyn EventSink> = Arc::new(yshjy::event::NullSink);
    let 取消 = CancelToken::new();
    let 响应 = 跑一条(&值, &事件, &取消);
    转_c字符串(响应.to_string())
}

/// 提交一条请求到后台线程。立即返回任务句柄（NULL = 内存不足）。
///
/// 事件通过 `cb` 实时回调；结果用 [`yshjy_job_wait`] 取。
///
/// # Safety
/// - `json` 同 [`yshjy_request`]
/// - `cb` 可以为 NULL（不关心进度）
/// - `user` 原样传回回调；生命周期由宿主保证到 `yshjy_job_free` 为止
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_job_submit(
    json: *const c_char,
    cb: YshjyEventCb,
    user: *mut c_void,
) -> *mut Job {
    let Some(文本) = 读_c字符串(json) else {
        return Job::完成(错误响应("BAD_PARAM", "json 指针为空或不是 UTF-8"));
    };
    let 值: Value = match serde_json::from_str(&文本) {
        Ok(v) => v,
        Err(e) => return Job::完成(错误响应("BAD_COMMAND", &format!("JSON 解析失败: {e}"))),
    };

    let 取消 = CancelToken::new();
    let (发, 收) = mpsc::channel::<String>();
    let sink: Arc<dyn EventSink> = match cb {
        Some(f) => Arc::new(CbSink {
            cb: f,
            user,
        }),
        None => Arc::new(yshjy::event::NullSink),
    };
    let 取消给线程 = 取消.clone();
    let 句柄 = std::thread::spawn(move || {
        // 绝不把 panic 抛回宿主：跨 FFI unwind 是未定义行为
        let 响应 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            跑一条(&值, &sink, &取消给线程).to_string()
        }))
        .unwrap_or_else(|_| 错误响应("PANIC", "引擎内部 panic（已拦截，未影响宿主）"));
        let _ = 发.send(响应);
    });

    Box::into_raw(Box::new(Job {
        取消,
        结果: Mutex::new(Some(收)),
        缓存: Mutex::new(None),
        句柄: Mutex::new(Some(句柄)),
    }))
}

/// 请求取消（可从任意线程调；重复调用无害）
///
/// # Safety
/// `job` 必须是 [`yshjy_job_submit`] 返回且**尚未 free** 的指针。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_job_cancel(job: *mut Job) -> i32 {
    if job.is_null() {
        return -1;
    }
    // SAFETY: 调用方契约保证指针有效
    let j = unsafe { &*job };
    j.取消.cancel_with("ffi cancel");
    0
}

/// 等任务结束并取结果（阻塞）。可重复调用：第二次返回同一份结果。
///
/// # Safety
/// 同 [`yshjy_job_cancel`]；返回值用 [`yshjy_string_free`] 释放。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_job_wait(job: *mut Job) -> *mut c_char {
    if job.is_null() {
        return 转_c字符串(错误响应("BAD_PARAM", "job 为空"));
    }
    // SAFETY: 调用方契约保证指针有效
    let j = unsafe { &*job };
    if let Some(s) = j.缓存.lock().ok().and_then(|g| g.clone()) {
        return 转_c字符串(s);
    }
    let 收 = j.结果.lock().ok().and_then(|mut g| g.take());
    let Some(收) = 收 else {
        return 转_c字符串(错误响应("INTERNAL", "结果已被取走且没有缓存"));
    };
    let 响应 = 收
        .recv()
        .unwrap_or_else(|_| 错误响应("ENGINE_EXITED", "worker 线程未返回结果"));
    if let Ok(mut g) = j.缓存.lock() {
        *g = Some(响应.clone());
    }
    if let Ok(mut g) = j.句柄.lock()
        && let Some(h) = g.take()
    {
        let _ = h.join();
    }
    转_c字符串(响应)
}

/// 释放任务句柄。**必须先把结果取走**（或接受结果丢失）。
///
/// # Safety
/// 同一指针只能 free 一次；free 后不得再使用。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_job_free(job: *mut Job) {
    if job.is_null() {
        return;
    }
    // SAFETY: 调用方契约保证是 Box::into_raw 出来的指针
    let j = unsafe { Box::from_raw(job) };
    // 任务还在跑：先取消再等它退出，避免线程引用已释放的宿主回调
    j.取消.cancel_with("ffi job freed");
    if let Ok(mut g) = j.句柄.lock()
        && let Some(h) = g.take()
    {
        let _ = h.join();
    }
}

/// 释放本库返回的所有字符串
///
/// # Safety
/// 只能传本库返回的指针，且只释放一次。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yshjy_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: 指针来自 CString::into_raw
    drop(unsafe { CString::from_raw(s) });
}

// ───────────────────────── 内部 ─────────────────────────

fn 跑一条(值: &Value, 事件: &Arc<dyn EventSink>, 取消: &CancelToken) -> Value {
    let id = 值.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let cmd = 值.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    if cmd == "cancel" {
        取消.cancel_with("ffi cancel");
        return json!({"id": id, "ok": true, "result": {"cancelled": true}});
    }
    // C ABI v1 不做交互式询问：超过限额按硬失败处理（宿主可自行取消后重试）。
    // 要「问用户」请用 --serve（ASK_USER/answer）。
    match api::执行(cmd, 值, 事件, 取消, None) {
        Ok(v) => json!({"id": id, "ok": true, "result": v}),
        Err(e) => json!({"id": id, "ok": false, "error": e}),
    }
}

fn 错误响应(码: &str, 备注: &str) -> String {
    json!({"id": 0, "ok": false, "error": {"code": 码, "note": 备注}}).to_string()
}

fn 读_c字符串(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: 调用方契约保证是以 0 结尾的有效字符串
    let s = unsafe { CStr::from_ptr(p) };
    s.to_str().ok().map(|x| x.to_string())
}

fn 转_c字符串(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        // 内容里有 NUL：退化成一条合法错误，绝不返回野指针
        Err(_) => CString::new(r#"{"ok":false,"error":{"code":"BAD_UTF8","note":"结果包含 NUL"}}"#)
            .map(|c| c.into_raw())
            .unwrap_or(std::ptr::null_mut()),
    }
}
