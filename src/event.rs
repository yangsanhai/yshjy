//! 引擎事件流（引擎 → 外壳）
//!
//! # 为什么要有事件（见 `docs/V0.5-ENGINE.md` §2）
//!
//! v0.4 的引擎是「一次调用、一个文件、结束时才返回」：解 8 GB 的包要等几分钟，
//! 这期间外壳什么都看不到、也没法反悔。GUI 的 S3（预览挑选）、S6（进度）、
//! S13（批量队列）都要求「看得见中间过程 + 能中途取消」。
//!
//! # 三条纪律
//!
//! 1. **事件是纯数据**，`serde` 可序列化 —— FFI 侧原样转 JSON 给 Dart `Stream`（BACKEND-SPEC §1.3）
//! 2. **不含面向用户的中文文案** —— 只带码 + 参数，渲染归外壳（三层审查修正 1）
//! 3. **进度事件可合并、结果事件不丢** —— 解 351 个文件会产生几万条进度，
//!    全塞进队列会淹掉外壳（台账 `M1`：有界队列 + 合并推送）
//!
//! 进度本身由 [`crate::progress`] 负责；这里只负责「什么时候告诉外壳什么」。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{ArchiveFlags, ItemResult, Outcome};
use crate::progress::ProgressView;

/// 处理阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// 定位（在文件里找出压缩包藏在哪）
    Locate,
    /// 列内容（预览）
    List,
    /// 解压
    Extract,
    /// 校验
    Verify,
    /// 打包
    Pack,
}

impl Stage {
    /// 稳定标识（日志/JSON 用）
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Locate => "LOCATE",
            Stage::List => "LIST",
            Stage::Extract => "EXTRACT",
            Stage::Verify => "VERIFY",
            Stage::Pack => "PACK",
        }
    }
}

/// 诊断信息码（归档级，不打断流程，只提醒）
///
/// 对应 7-Zip 的 `reportInterestingFileWithReason:` 埋点思路（XADMaster 分析，
/// 见 `refs/analysis/00-总览与关键发现.md` §11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCode {
    /// 包藏在文件中间（前面有一大段外壳）
    HiddenAtOffset,
    /// 中央目录坏了，已按文件头重建清单
    RebuiltIndex,
    /// 索引里的偏移是错的，已在内存里修正（不动源文件）
    OffsetCorrected,
    /// 分卷已自动拼接
    VolumesMerged,
    /// 布局靠嗅探猜出来的，没十足把握
    LayoutGuessed,
    /// 封面视频本身损坏（不影响解压）
    VideoUnplayable,
    /// 解出可执行文件/脚本（安全提醒）
    RiskyEntry,
    /// 走了慢路径（全文件扫描）
    SlowPath,
    /// 解压码是从旁边的说明文件抓到的
    PasswordFromNotes,
    /// 数据只有一部分（残包，只能救多少算多少）
    PartialData,
}

impl DiagnosticCode {
    /// 稳定标识（JSON/日志用）
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::HiddenAtOffset => "HIDDEN_AT_OFFSET",
            DiagnosticCode::RebuiltIndex => "REBUILT_INDEX",
            DiagnosticCode::OffsetCorrected => "OFFSET_CORRECTED",
            DiagnosticCode::VolumesMerged => "VOLUMES_MERGED",
            DiagnosticCode::LayoutGuessed => "LAYOUT_GUESSED",
            DiagnosticCode::VideoUnplayable => "VIDEO_UNPLAYABLE",
            DiagnosticCode::RiskyEntry => "RISKY_ENTRY",
            DiagnosticCode::SlowPath => "SLOW_PATH",
            DiagnosticCode::PasswordFromNotes => "PASSWORD_FROM_NOTES",
            DiagnosticCode::PartialData => "PARTIAL_DATA",
        }
    }
}

/// 预览条目（S3「列出内容」的数据源）
///
/// 只有元数据，不含内容 —— 所以 8 GB 的包也能秒级列出来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryView {
    /// 条目序号（**预览到解压全程不变**，GUI 勾选就靠它）
    pub index: u64,
    /// 归档内路径
    pub path: String,
    /// 原始（解压后）大小
    pub size: u64,
    /// 压缩后大小（未知则 `None`）
    pub packed: Option<u64>,
    /// 是否加密（界面上的 🔒 就是读它）
    pub encrypted: bool,
    /// 是否目录
    pub is_dir: bool,
    /// 修改时间（Unix 秒，未知则 `None`）
    pub modified: Option<i64>,
    /// 是否可执行/脚本（安全提醒：解出来别乱点）
    pub is_risky: bool,
}

/// 预览提示码（外壳渲染成 UI-SPEC 里那几句「ℹ️ 检测到 2 个分卷，会自动合并」）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewHint {
    /// 检测到 n 个分卷，会自动合并
    VolumesMerged {
        /// 分卷数
        count: u64,
    },
    /// 里面还套了一层压缩包
    NestedArchive {
        /// 内层的格式名（如 `7z`）
        inner: String,
    },
    /// 需要密码才能解开
    NeedsPassword {
        /// 带锁的条目数
        entries: u64,
    },
    /// 看着像下载没完成（尾部缺数据）
    LooksTruncated,
    /// 包藏在文件中间
    HiddenAt {
        /// 偏移
        offset: u64,
    },
    /// 清单是按文件头重建的（原索引坏了）
    RebuiltIndex,
    /// 封面视频本身坏了，但里面的文件还能解
    VideoUnplayable,
}

/// 引擎事件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// 一个任务开始
    JobStarted {
        /// 任务号
        job: u64,
        /// 输入路径
        path: String,
        /// 文件大小
        size: u64,
    },
    /// 一个任务结束
    JobFinished {
        /// 任务号
        job: u64,
        /// 结论
        outcome: Outcome,
    },
    /// 进入某个阶段
    StageStarted {
        /// 任务号
        job: u64,
        /// 阶段
        stage: Stage,
    },
    /// 某个阶段结束（带耗时，`-v` 与诊断包用）
    StageFinished {
        /// 任务号
        job: u64,
        /// 阶段
        stage: Stage,
        /// 耗时（毫秒）
        millis: u64,
    },
    /// 扫描进度（**双维度**：文件数 / 字节数，两者都可空，台账 §3.3）
    ScanProgress {
        /// 任务号
        job: u64,
        /// (已完成, 总数) 文件数
        files: Option<(u64, u64)>,
        /// (已完成, 总数) 字节数
        bytes: Option<(u64, u64)>,
    },
    /// 预览就绪（S3 的数据源）
    PreviewReady {
        /// 任务号
        job: u64,
        /// 条目清单
        entries: Vec<EntryView>,
        /// 提示（可能多条）
        hints: Vec<PreviewHint>,
    },
    /// 解压进度
    ExtractProgress {
        /// 任务号
        job: u64,
        /// 进度读数
        progress: ProgressView,
        /// 当前正在处理的条目（可能没有）
        current: Option<String>,
    },
    /// 某个条目处理完了（S7/S8 的明细，层3 结果）
    EntryFinished {
        /// 任务号
        job: u64,
        /// 条目序号
        index: u64,
        /// 条目路径
        path: String,
        /// 条目结果
        result: ItemResult,
        /// 写出的字节数
        bytes: u64,
    },
    /// 需要密码（S4）
    PasswordNeeded {
        /// 任务号
        job: u64,
        /// 已经试过几次
        attempts: u32,
        /// 还剩几个候选可以试
        candidates: u32,
    },
    /// 正在试第 N 个密码候选（S4：让用户看见「在动」，不是卡死）
    PasswordTry {
        /// 任务号
        job: u64,
        /// 第几个（1-based）
        index: u32,
        /// 一共几个
        total: u32,
    },
    /// 空间可能不够（S5）
    SpaceShortage {
        /// 任务号
        job: u64,
        /// 需要多少字节
        needed: u64,
        /// 可用多少字节
        available: u64,
    },
    /// 诊断提醒（不打断流程）
    Diagnostic {
        /// 任务号
        job: u64,
        /// 诊断码
        code: DiagnosticCode,
        /// 归档级问题位图
        flags: ArchiveFlags,
        /// 给开发者的备注（进日志，不进用户文案）
        note: String,
    },
}

impl Event {
    /// 属于哪个任务
    pub fn job(&self) -> u64 {
        match self {
            Event::JobStarted { job, .. }
            | Event::JobFinished { job, .. }
            | Event::StageStarted { job, .. }
            | Event::StageFinished { job, .. }
            | Event::ScanProgress { job, .. }
            | Event::PreviewReady { job, .. }
            | Event::ExtractProgress { job, .. }
            | Event::EntryFinished { job, .. }
            | Event::PasswordNeeded { job, .. }
            | Event::PasswordTry { job, .. }
            | Event::SpaceShortage { job, .. }
            | Event::Diagnostic { job, .. } => *job,
        }
    }

    /// 是不是「高频进度」事件（可以被合并、可以丢）
    ///
    /// 结果类事件（`EntryFinished` / `PreviewReady` / `JobFinished`…）**永不丢弃**。
    pub fn is_progress(&self) -> bool {
        matches!(
            self,
            Event::ScanProgress { .. } | Event::ExtractProgress { .. }
        )
    }

    /// 事件类型标识（日志用）
    pub fn kind(&self) -> &'static str {
        match self {
            Event::JobStarted { .. } => "JOB_STARTED",
            Event::JobFinished { .. } => "JOB_FINISHED",
            Event::StageStarted { .. } => "STAGE_STARTED",
            Event::StageFinished { .. } => "STAGE_FINISHED",
            Event::ScanProgress { .. } => "SCAN_PROGRESS",
            Event::PreviewReady { .. } => "PREVIEW_READY",
            Event::ExtractProgress { .. } => "EXTRACT_PROGRESS",
            Event::EntryFinished { .. } => "ENTRY_FINISHED",
            Event::PasswordNeeded { .. } => "PASSWORD_NEEDED",
            Event::PasswordTry { .. } => "PASSWORD_TRY",
            Event::SpaceShortage { .. } => "SPACE_SHORTAGE",
            Event::Diagnostic { .. } => "DIAGNOSTIC",
        }
    }
}

// ───────────────────────────── 事件出口 ─────────────────────────────

/// 事件出口（外壳实现）
///
/// 实现方必须**非阻塞**：引擎在工作线程里调用它，卡住就等于卡住解压。
pub trait EventSink: Send + Sync {
    /// 送出一条事件
    fn emit(&self, ev: &Event);

    /// 是否想要进度事件（管道/脚本模式可以关掉，省掉格式化开销）
    fn wants_progress(&self) -> bool {
        true
    }
}

/// 什么都不做的出口（测试、纯脚本模式）
#[derive(Debug, Default)]
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _ev: &Event) {}
}

/// 有界事件队列（CLI 主线程按需抽取）
///
/// 规则（台账 `M1` + `F2`）：
/// - **进度事件合并**：同一任务连续的进度只保留最后一条
/// - **队列满时先丢最旧的进度事件**，结果事件不丢
/// - 丢弃与合并都计数，便于诊断「是不是丢过头了」
#[derive(Debug)]
pub struct ChannelSink {
    inner: Mutex<Queue>,
    capacity: usize,
}

#[derive(Debug, Default)]
struct Queue {
    events: VecDeque<Event>,
    dropped: u64,
    merged: u64,
}

impl ChannelSink {
    /// 新建（`capacity` 是队列上限，建议 1024~8192）
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Queue::default()),
            capacity: capacity.max(1),
        })
    }

    /// 取出目前积累的所有事件（先进先出）
    pub fn drain(&self) -> Vec<Event> {
        match self.inner.lock() {
            Ok(mut q) => q.events.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 当前积压数量
    pub fn len(&self) -> usize {
        self.inner.lock().map(|q| q.events.len()).unwrap_or(0)
    }

    /// 是否没有积压
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 被丢弃的事件数（队列满导致）
    pub fn dropped(&self) -> u64 {
        self.inner.lock().map(|q| q.dropped).unwrap_or(0)
    }

    /// 被合并掉的进度事件数
    pub fn merged(&self) -> u64 {
        self.inner.lock().map(|q| q.merged).unwrap_or(0)
    }

    /// 看一眼最后一条事件（不取走）
    pub fn last(&self) -> Option<Event> {
        self.inner.lock().ok().and_then(|q| q.events.back().cloned())
    }

    fn push(&self, ev: Event) {
        let Ok(mut q) = self.inner.lock() else {
            return;
        };

        // ① 进度事件：同一任务只保留最后一条
        if ev.is_progress()
            && let Some(idx) = q
                .events
                .iter()
                .rposition(|e| e.is_progress() && e.job() == ev.job())
        {
            q.events[idx] = ev;
            q.merged += 1;
            return;
        }

        // ② 队列满：先丢最旧的进度事件腾位置
        if q.events.len() >= self.capacity {
            match q.events.iter().position(|e| e.is_progress()) {
                Some(idx) => {
                    q.events.remove(idx);
                    q.dropped += 1;
                }
                None => {
                    // 全是结果事件 —— 绝不静默丢，记账
                    q.dropped += 1;
                    return;
                }
            }
        }

        q.events.push_back(ev);
    }
}

impl EventSink for ChannelSink {
    fn emit(&self, ev: &Event) {
        self.push(ev.clone());
    }
}

/// 把事件同时送给多个出口（如 CLI 打印 + 队列留档）
pub struct TeeSink {
    sinks: Vec<Box<dyn EventSink>>,
}

impl TeeSink {
    /// 组合多个出口
    pub fn new(sinks: Vec<Box<dyn EventSink>>) -> Self {
        Self { sinks }
    }
}

impl EventSink for TeeSink {
    fn emit(&self, ev: &Event) {
        for s in &self.sinks {
            s.emit(ev);
        }
    }

    fn wants_progress(&self) -> bool {
        self.sinks.iter().any(|s| s.wants_progress())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::ProgressView;

    fn 进度事件(job: u64, done: u64) -> Event {
        Event::ExtractProgress {
            job,
            progress: ProgressView {
                done_bytes: done,
                total_bytes: 1000,
                done_files: 0,
                total_files: 1,
            },
            current: None,
        }
    }

    #[test]
    fn 进度事件被合并成最后一条() {
        let sink = ChannelSink::new(64);
        sink.emit(&进度事件(1, 10));
        sink.emit(&进度事件(1, 20));
        sink.emit(&进度事件(1, 30));

        let got = sink.drain();
        assert_eq!(got.len(), 1, "同一任务的连续进度应当只剩最后一条");
        match &got[0] {
            Event::ExtractProgress { progress, .. } => assert_eq!(progress.done_bytes, 30),
            other => panic!("事件类型不对：{other:?}"),
        }
        assert_eq!(sink.merged(), 2);
        assert_eq!(sink.dropped(), 0);
    }

    #[test]
    fn 不同任务的进度不互相合并() {
        let sink = ChannelSink::new(64);
        sink.emit(&进度事件(1, 10));
        sink.emit(&进度事件(2, 20));
        assert_eq!(sink.len(), 2, "两个任务各自保留自己的进度");
    }

    #[test]
    fn 结果事件不丢且保序() {
        let sink = ChannelSink::new(64);
        for i in 0..3 {
            sink.emit(&Event::EntryFinished {
                job: 1,
                index: i,
                path: format!("文件{i}.txt"),
                result: ItemResult::Ok,
                bytes: 10,
            });
        }
        let got = sink.drain();
        assert_eq!(got.len(), 3);
        let idx: Vec<u64> = got
            .iter()
            .map(|e| match e {
                Event::EntryFinished { index, .. } => *index,
                _ => panic!("类型不对"),
            })
            .collect();
        assert_eq!(idx, vec![0, 1, 2], "顺序必须保持");
    }

    #[test]
    fn 队列满时先丢最旧的进度事件() {
        let sink = ChannelSink::new(3);
        // 先塞两条结果事件 + 一条进度
        sink.emit(&Event::EntryFinished {
            job: 1,
            index: 0,
            path: "a".into(),
            result: ItemResult::Ok,
            bytes: 1,
        });
        sink.emit(&进度事件(1, 5));
        sink.emit(&Event::EntryFinished {
            job: 1,
            index: 1,
            path: "b".into(),
            result: ItemResult::Ok,
            bytes: 1,
        });
        // 再来一条别的任务的进度事件 —— 队列满，应当挤掉那条进度
        sink.emit(&进度事件(2, 7));

        let got = sink.drain();
        assert_eq!(got.len(), 3);
        assert!(
            got.iter().all(|e| !matches!(e, Event::ExtractProgress { job, .. } if *job == 1)),
            "最旧的进度事件应当被挤掉"
        );
        assert_eq!(sink.dropped(), 1);
    }

    #[test]
    fn 全是结果事件时丢弃会被计数() {
        let sink = ChannelSink::new(2);
        for i in 0..4 {
            sink.emit(&Event::EntryFinished {
                job: 1,
                index: i,
                path: format!("f{i}"),
                result: ItemResult::Ok,
                bytes: 1,
            });
        }
        assert_eq!(sink.len(), 2, "上限就是上限");
        assert_eq!(sink.dropped(), 2, "丢弃必须记账，绝不静默");
    }

    #[test]
    fn 事件可序列化交给_ffi() {
        let ev = Event::PreviewReady {
            job: 3,
            entries: vec![EntryView {
                index: 0,
                path: "游戏数据/pak.dat".into(),
                size: 12345,
                packed: Some(6000),
                encrypted: false,
                is_dir: false,
                modified: None,
                is_risky: false,
            }],
            hints: vec![PreviewHint::VolumesMerged { count: 2 }],
        };
        let j = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&j).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn 事件分类与任务号可读() {
        let ev = Event::Diagnostic {
            job: 9,
            code: DiagnosticCode::RebuiltIndex,
            flags: ArchiveFlags::empty(),
            note: "no central directory".into(),
        };
        assert_eq!(ev.job(), 9);
        assert!(!ev.is_progress());
        assert_eq!(ev.kind(), "DIAGNOSTIC");
        assert!(进度事件(1, 1).is_progress());
        assert_eq!(Stage::Extract.as_str(), "EXTRACT");
    }

    #[test]
    fn 空出口什么都不做() {
        let sink = NullSink;
        sink.emit(&进度事件(1, 1));
        assert!(sink.wants_progress());
    }

    #[test]
    fn 组合出口分发给每个子出口() {
        let a = ChannelSink::new(8);
        let b = ChannelSink::new(8);
        // TeeSink 持有内部出口；这里用 Arc 克隆验证两边都收到
        struct Pair(Arc<ChannelSink>, Arc<ChannelSink>);
        impl EventSink for Pair {
            fn emit(&self, ev: &Event) {
                self.0.emit(ev);
                self.1.emit(ev);
            }
        }
        let tee = TeeSink::new(vec![Box::new(Pair(a.clone(), b.clone()))]);
        tee.emit(&进度事件(1, 42));
        assert_eq!(a.drain().len(), 1);
        assert_eq!(b.drain().len(), 1);
    }
}
