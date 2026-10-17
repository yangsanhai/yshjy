//! `--serve`：把引擎包成「一行一个 JSON」的双向管道
//!
//! 契约见 `docs/PROTOCOL.md`。这一层存在的唯一目的：
//! **让界面（Flutter / Tauri / 脚本）只依赖协议，不依赖引擎内部** ——
//! 引擎怎么重构、界面怎么换，只要协议不动，两边可以各自升级。
//!
//! 命令**语义**在 [`yshjy::api`]（C ABI 也调同一份）；这里只管传输：
//! 信封、线程、忙/取消、事件出队。
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
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use yshjy::api;
use yshjy::event::{ChannelSink, EventSink};
use yshjy::progress::CancelToken;

/// 协议版本（**只有破坏性改动才加 1**，见 PROTOCOL.md 的兼容性承诺）
///
/// 真身在 `yshjy::api`（C ABI 也用它），这里只是给同 crate 的测试一个短名字。
#[cfg(test)]
const 协议版本: u64 = api::协议版本;

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
        "hello" => 写行(出, &ok(id, api::hello())),
        "cancel" => {
            if let Some((_, 取消, _)) = 当前 {
                取消.cancel_with("api cancel");
            }
            写行(出, &ok(id, json!({"cancelled": true})));
        }
        // 生成诊断包（报障用）：界面点一下就出 zip，不用教用户开终端
        "report" => {
            let sink: Arc<dyn EventSink> = 事件.clone();
            let 结果 = api::诊断(&值, &sink);
            写行(出, &结果响应(id, 结果));
        }
        // 扫一批路径（S13 队列 / S10 多候选的数据源）
        "scan" => {
            let 结果 = api::扫描(&值);
            写行(出, &结果响应(id, 结果));
        }
        "list" | "extract" | "pack" | "batch" => {
            let 参数 = 值.clone();
            let 事件: Arc<dyn EventSink> = 事件.clone();
            let 发 = 结果发.clone();
            let 取消 = CancelToken::new();
            let 取消给任务 = 取消.clone();
            let 句柄 = std::thread::spawn(move || {
                let 结果 = api::执行(&cmd, &参数, &事件, &取消给任务);
                let _ = 发.send(结果响应(id, 结果));
            });
            *当前 = Some((id, 取消, 句柄));
        }
        other => 写行(
            出,
            &错误响应(id, "UNKNOWN_COMMAND", &format!("不认识的 cmd: {other}")),
        ),
    }
}

// ───────────────────────── 信封 ─────────────────────────

fn ok(id: u64, v: Value) -> Value {
    json!({"id": id, "ok": true, "result": v})
}

fn 错误响应(id: u64, 码: &str, 备注: &str) -> Value {
    json!({"id": id, "ok": false, "error": {"code": 码, "note": 备注}})
}

fn 结果响应(id: u64, r: Result<Value, Value>) -> Value {
    match r {
        Ok(v) => ok(id, v),
        Err(e) => json!({"id": id, "ok": false, "error": e}),
    }
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
    fn hello_带协议版本与完整命令表() {
        let h = api::hello();
        assert_eq!(h["protocol"], 协议版本);
        assert_eq!(h["engine"], yshjy::VERSION);
        // PROTOCOL 表 / 前端能力判断都吃这个数组：漏一项就等于"功能不存在"
        for c in ["hello", "cancel", "report", "scan", "list", "extract", "pack", "batch"] {
            assert!(
                h["features"].as_array().unwrap().contains(&json!(c)),
                "hello.features 少了 {c}"
            );
        }
    }

    #[test]
    fn 非压缩包的错误里带着真实类型() {
        // 造一个"名字是 .mp4、内容其实是 ZIP"的文件 —— 伪装包的典型样子
        let dir = std::env::temp_dir().join(format!("yshjy_serve_mis_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let 假 = dir.join("伪装.mp4");
        std::fs::write(&假, b"PK\x03\x04not a real zip but signature is enough").unwrap();

        let 事件: Arc<dyn EventSink> = ChannelSink::new(64);
        let 取消 = CancelToken::new();
        let e = api::列表(&json!({"path": 假.to_str().unwrap()}), &事件, &取消).unwrap_err();
        assert_eq!(e["code"], "NOT_ARCHIVE", "{e}");
        let d = &e["detail"];
        assert_eq!(d["looks_like"], "zip", "要认出内容其实是 ZIP：{d}");
        assert_eq!(d["ext"], "mp4");
        assert_eq!(d["ext_mismatch"], "true", "名字与内容不符必须标出来：{d}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 缺参数报_bad_param_而不是崩() {
        let 事件: Arc<dyn EventSink> = ChannelSink::new(16);
        let 取消 = CancelToken::new();
        let e = api::列表(&json!({}), &事件, &取消).unwrap_err();
        assert_eq!(e["code"], "BAD_PARAM");
    }
}
