//! 解压上下文：事件 / 进度 / 取消 三件套
//!
//! # 出处
//! BACKEND-SPEC §5.1 里设计的 `UnpackCtx`（「含取消标志、内存上限、进度回调」）。
//! v0.4 的解压函数只收 `&ExtractOptions`，所以「解到一半想报进度」「用户点取消」
//! 这两件事都无处安放 —— 这是 GUI 做不出来的直接原因。
//!
//! # 用法
//! ```no_run
//! use std::sync::Arc;
//! use yshjy::ctx::UnpackCtx;
//! use yshjy::event::NullSink;
//!
//! let ctx = UnpackCtx::quiet(1);          // 什么都不报告
//! let ctx = ctx.with_sink(Arc::new(NullSink));  // 换成真的出口
//! ```
//!
//! # 进度为什么要节流
//! 解 8 GB 要循环 13 万次（每次 64 KiB）。每次都发事件会把外壳淹掉，
//! 所以这里做**合并推送**：100 ms 内多次变化只发最后一条（三层审查修正 F2）。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{Error, ItemResult, Result};
use crate::event::{Event, EventSink, NullSink, Stage};
use crate::progress::{CancelToken, ProgressSnapshot};

/// 进度事件的最小间隔（短于此间隔的变化会被合并掉）
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// 一次解压/打包的上下文
#[derive(Clone)]
pub struct UnpackCtx {
    /// 任务号（事件里用它区分批量中的哪个任务）
    pub job: u64,
    /// 事件出口
    pub sink: Arc<dyn EventSink>,
    /// 进度快照（无锁读）
    pub progress: Arc<ProgressSnapshot>,
    /// 取消令牌
    pub cancel: CancelToken,
    /// 上次发进度事件的时间（节流用）
    last_emit: Arc<Mutex<Instant>>,
}

impl UnpackCtx {
    /// 新建
    pub fn new(job: u64, sink: Arc<dyn EventSink>) -> Self {
        Self {
            job,
            sink,
            progress: ProgressSnapshot::new(),
            cancel: CancelToken::new(),
            // 初始时间戳往前推一个间隔，保证**第一条进度就能发出去**
            // （否则小任务会一条进度事件都没有，界面看起来像卡住了）
            last_emit: Arc::new(Mutex::new(
                Instant::now()
                    .checked_sub(PROGRESS_INTERVAL)
                    .unwrap_or_else(Instant::now),
            )),
        }
    }

    /// 什么都不报告的上下文（测试、内部调用、纯批量模式）
    pub fn quiet(job: u64) -> Self {
        Self::new(job, Arc::new(NullSink))
    }

    /// 换一个事件出口
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = sink;
        self
    }

    /// 共用一个进度快照（多任务聚合时用）
    pub fn with_progress(mut self, progress: Arc<ProgressSnapshot>) -> Self {
        self.progress = progress;
        self
    }

    /// 共用一个取消令牌（批量取消时用）
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// 发一条事件
    pub fn emit(&self, ev: &Event) {
        self.sink.emit(ev);
    }

    /// 是否已被要求取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// 取消检查：已取消就返回 `Err(Cancelled)`
    pub fn check_cancel(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::cancelled().with("job", self.job))
        } else {
            Ok(())
        }
    }

    /// 设定总量（解压前由元数据算出）
    pub fn set_total(&self, bytes: u64, files: u64) {
        self.progress.set_total_bytes(bytes);
        self.progress.set_total_files(files);
    }

    /// 上报「又写出去了 n 字节」
    pub fn advance(&self, bytes: u64, current: Option<&str>) {
        self.progress.add_bytes(bytes);
        self.maybe_progress(current);
    }

    /// 强制发一条进度（阶段结束时用，保证界面看到的是最新值）
    pub fn force_progress(&self, current: Option<&str>) {
        if let Ok(mut t) = self.last_emit.lock() {
            *t = Instant::now();
        }
        self.emit(&Event::ExtractProgress {
            job: self.job,
            progress: self.progress.view(),
            current: current.map(|s| s.to_string()),
        });
    }

    /// 节流版的进度上报
    fn maybe_progress(&self, current: Option<&str>) {
        let now = Instant::now();
        let due = match self.last_emit.lock() {
            Ok(mut last) => {
                if now.duration_since(*last) >= PROGRESS_INTERVAL {
                    *last = now;
                    true
                } else {
                    false
                }
            }
            // 锁坏了也要能继续干活
            Err(_) => true,
        };
        if due {
            self.emit(&Event::ExtractProgress {
                job: self.job,
                progress: self.progress.view(),
                current: current.map(|s| s.to_string()),
            });
        }
    }

    /// 一个条目处理完了
    pub fn entry_done(&self, index: u64, path: &str, result: ItemResult, bytes: u64) {
        self.progress.add_file();
        self.emit(&Event::EntryFinished {
            job: self.job,
            index,
            path: path.to_string(),
            result,
            bytes,
        });
    }

    /// 进入某个阶段
    pub fn stage_started(&self, stage: Stage) {
        self.emit(&Event::StageStarted {
            job: self.job,
            stage,
        });
    }

    /// 某个阶段结束
    pub fn stage_finished(&self, stage: Stage, millis: u64) {
        self.emit(&Event::StageFinished {
            job: self.job,
            stage,
            millis,
        });
    }

    /// 一条诊断提醒
    pub fn diagnostic(&self, code: crate::event::DiagnosticCode, note: impl Into<String>) {
        self.emit(&Event::Diagnostic {
            job: self.job,
            code,
            flags: crate::error::ArchiveFlags::empty(),
            note: note.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ChannelSink;

    #[test]
    fn 安静上下文不产生事件() {
        let ctx = UnpackCtx::quiet(1);
        ctx.advance(1024, Some("a.txt"));
        ctx.entry_done(0, "a.txt", ItemResult::Ok, 1024);
        assert_eq!(ctx.progress.view().done_bytes, 1024);
        assert_eq!(ctx.progress.view().done_files, 1);
        assert!(!ctx.is_cancelled());
    }

    #[test]
    fn 进度事件被节流但数字不丢() {
        let sink = ChannelSink::new(64);
        let ctx = UnpackCtx::quiet(1).with_sink(sink.clone());
        // 连续 1000 次上报：事件远少于上报次数，但字节数一个不少
        for _ in 0..1000 {
            ctx.advance(100, Some("大文件"));
        }
        assert_eq!(ctx.progress.view().done_bytes, 100_000);
        assert!(
            sink.len() < 1000,
            "节流应当生效，实际事件数 {}",
            sink.len()
        );
    }

    #[test]
    fn 强制进度一定发一条() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// 只数进度事件的假出口（队列会合并同任务的进度，数长度会看错）
        struct 只数进度(AtomicUsize);
        impl EventSink for 只数进度 {
            fn emit(&self, ev: &Event) {
                if matches!(ev, Event::ExtractProgress { .. }) {
                    self.0.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        let counter = Arc::new(只数进度(AtomicUsize::new(0)));
        let ctx = UnpackCtx::quiet(1).with_sink(counter.clone());

        let n0 = counter.0.load(Ordering::Relaxed);
        ctx.force_progress(Some("最后的文件"));
        assert_eq!(
            counter.0.load(Ordering::Relaxed),
            n0 + 1,
            "force_progress 必须绕过节流，一定发出去"
        );
    }

    #[test]
    fn 取消后检查会报取消() {
        let ctx = UnpackCtx::quiet(7);
        ctx.cancel.cancel_with("用户点了取消");
        let e = ctx.check_cancel().unwrap_err();
        assert!(e.is_cancelled());
        assert_eq!(e.param("job"), Some("7"));
    }

    #[test]
    fn 阶段与条目事件带任务号() {
        let sink = ChannelSink::new(64);
        let ctx = UnpackCtx::quiet(5).with_sink(sink.clone());
        ctx.stage_started(Stage::Extract);
        ctx.entry_done(3, "游戏/a.pak", ItemResult::Ok, 999);
        ctx.stage_finished(Stage::Extract, 1234);
        let evs = sink.drain();
        assert_eq!(evs.len(), 3);
        assert!(evs.iter().all(|e| e.job() == 5));
    }
}
