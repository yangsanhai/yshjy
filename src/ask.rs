//! 双向询问：引擎问一句、用户答一句（限额超过时用）
//!
//! 为什么需要它：安全限额（压缩比 / 累计写出量）命中时，直接失败会毁掉一次
//! 已经解了半天的任务，直接放行又违背"安全默认"。正确做法是**停下来问**：
//!
//! ```text
//! 任务线程 ──emit ASK_USER──▶ 外壳弹窗 ──answer──▶ 主循环 ──▶ 这里
//!    └──────── 阻塞等待（超时默认「跳过」）────────┘
//! ```
//!
//! 三条纪律：
//! 1. **超时 = 跳过**（默认 60 秒）：用户不在、前端崩了，任务不能永远吊着
//! 2. **一次任务只问一次**：用户说继续就别每 1 MB 再问一遍（`ExtractBudget` 里记着）
//! 3. **答案只有三种**：继续 / 跳过这一个 / 取消 —— 不提供"从此都不问"，
//!    那是设置里的事（`zip_bomb_ratio`），不藏在弹窗里

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::event::{Event, EventSink};

/// 用户对询问的答复
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum 答案 {
    /// 继续解（本次任务不再检查这条限额）
    继续,
    /// 只跳过当前这一个任务
    跳过,
    /// 取消（当前任务中止，批量里也不再继续由调用方决定）
    取消,
}

/// 把协议里的字符串解析成答案（不认识的返回 `None`，由调用方报 BAD_PARAM）
pub fn 解析答案(s: &str) -> Option<答案> {
    match s {
        "continue" | "yes" => Some(答案::继续),
        "skip" | "no" => Some(答案::跳过),
        "cancel" | "abort" => Some(答案::取消),
        _ => None,
    }
}

/// 询问闸门：任务线程在这里阻塞，主循环把答案送进来
pub struct AskGate {
    发: mpsc::Sender<答案>,
    收: Mutex<mpsc::Receiver<答案>>,
    超时: Duration,
    事件: std::sync::Arc<dyn EventSink>,
    下一个: AtomicU64,
    /// 此刻有没有一个「问」正卡在 `recv_timeout` 上
    ///
    /// 唯一用途是**让测试确定性地等**「新问题已经开始等」。
    /// 为什么必须要有：原来的测试靠 `sleep(10ms)` 猜，全量并行跑（8 个测试线程）时
    /// 线程调度一延迟，「继续」就落在新问题清空缓冲之前被丢掉 → 新问题超时返回「跳过」，
    /// 测试偶发红（2026-11-02 全量跑时撞到）。靠时间猜顺序的测试永远会 flaky。
    等待中: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for AskGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 事件出口（dyn EventSink）不实现 Debug，这里只交代超时，够排查了
        f.debug_struct("AskGate").field("超时", &self.超时).finish()
    }
}

impl AskGate {
    /// `超时` 到点没人回答 → 按「跳过」处理
    pub fn new(事件: std::sync::Arc<dyn EventSink>, 超时: Duration) -> std::sync::Arc<Self> {
        let (发, 收) = mpsc::channel();
        std::sync::Arc::new(Self {
            发,
            收: Mutex::new(收),
            超时,
            事件,
            下一个: AtomicU64::new(1),
            等待中: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// 问一句（**阻塞**），返回用户答案；超时返回 `跳过`
    ///
    /// `原因`: `possible_zip_bomb` / `extract_size_limit_exceeded`
    pub fn 问(&self, 原因: &str, 已写: u64, 上限: u64) -> 答案 {
        let id = self.下一个.fetch_add(1, Ordering::Relaxed);
        self.事件.emit(&Event::AskUser {
            job: 0,
            id,
            reason: 原因.to_string(),
            written: 已写,
            limit: 上限,
        });
        // 先清掉可能残留的旧答案（上一次超时后才到的答复，不能算这一次）
        if let Ok(收) = self.收.lock() {
            while 收.try_recv().is_ok() {}
            // ★ 只在**真要开始等**的这一刻置位，返回前复位。
            //   测试靠它确定「新问题已经在等了」，不再用 sleep 猜顺序。
            self.等待中.store(true, Ordering::SeqCst);
            let 结果 = 收.recv_timeout(self.超时);
            self.等待中.store(false, Ordering::SeqCst);
            return 结果.unwrap_or(答案::跳过);
        }
        答案::跳过
    }

    /// 从主循环投递答案；没有任务在等待也返回 `true`（答案被丢弃即可）
    pub fn 回答(&self, 答案: 答案) -> bool {
        self.发.send(答案).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::NullSink;

    /// 等有一句「问」真的卡在等答案上（最多 2 秒）
    ///
    /// 不用 `sleep`：并行跑测试时线程调度延迟随时超过固定 sleep，
    /// 那样答案会落在「清空残留」之前被丢掉，测试偶发红。
    fn 等进入等待(gate: &AskGate) {
        let 截止 = std::time::Instant::now() + Duration::from_secs(2);
        while !gate.等待中.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < 截止,
                "2 秒内都没有「问」进入等待，实现或线程启动有问题"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn 收到答案就返回它() {
        let gate = AskGate::new(std::sync::Arc::new(NullSink), Duration::from_millis(200));
        let g2 = gate.clone();
        let t = std::thread::spawn(move || g2.问("possible_zip_bomb", 100, 10));
        等进入等待(&gate);
        assert!(gate.回答(答案::继续));
        assert_eq!(t.join().unwrap(), 答案::继续);
    }

    #[test]
    fn 超时默认跳过() {
        let gate = AskGate::new(std::sync::Arc::new(NullSink), Duration::from_millis(50));
        assert_eq!(gate.问("extract_size_limit_exceeded", 1, 1), 答案::跳过);
    }

    #[test]
    fn 迟到答案不会污染下一次询问() {
        let gate = AskGate::new(std::sync::Arc::new(NullSink), Duration::from_millis(40));
        assert_eq!(gate.问("x", 1, 1), 答案::跳过); // 超时
        gate.回答(答案::取消); // 迟到
        let g2 = gate.clone();
        let t = std::thread::spawn(move || g2.问("x", 1, 1));
        等进入等待(&gate);
        gate.回答(答案::继续);
        assert_eq!(t.join().unwrap(), 答案::继续, "迟到的 cancel 不能算到下一次");
    }

    #[test]
    fn 字符串解析() {
        assert_eq!(解析答案("continue"), Some(答案::继续));
        assert_eq!(解析答案("skip"), Some(答案::跳过));
        assert_eq!(解析答案("cancel"), Some(答案::取消));
        assert_eq!(解析答案("whatever"), None);
    }
}
