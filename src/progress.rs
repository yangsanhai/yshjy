//! 进度、取消与剩余时间估算
//!
//! # 设计（见 `docs/V0.5-ENGINE.md` §3）
//!
//! 拆成**两个对象**，而不是 7-Zip 那样一个加锁的大结构体（台账 `7zip-工程层.md` §6.4）：
//!
//! - [`ProgressSnapshot`] —— 全是 `AtomicU64`，读不加锁；进度是单调累加量，
//!   用 `fetch_add` 比 `Mutex` 更合适（台账 §2.6）
//! - [`CancelToken`] —— 只管取消，与进度数据解耦
//!
//! 另外三件配套：
//! - [`ProgressAggregator`] —— 多任务各自的增量聚合成一条单调的全局进度
//! - [`LocalProgress`] —— 嵌套解压（ZIP 里套 7z）的内层进度折算到外层（RAII，台账 §3.5）
//! - [`EtaEstimator`] —— 剩余时间估算，指数滑动平均，样本太少时**不给估算**（宁可不显示也不乱跳）

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ───────────────────────────── 取消 ─────────────────────────────

/// 取消令牌
///
/// 外壳持有它来请求取消，引擎（或其读取层）持克隆来检查。
/// **取消不是错误**：它对应 `Outcome::Cancelled`，退出码 130（台账 §5.8）。
#[derive(Debug, Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    /// 「只跳过当前这个」—— 与取消分开：跳过后批量继续跑下一个
    skip: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    /// 新建一个未取消的令牌
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            skip: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    /// 请求取消
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// 带原因请求取消（原因只进日志，不进用户文案）
    pub fn cancel_with(&self, reason: impl Into<String>) {
        if let Ok(mut r) = self.reason.lock() {
            *r = Some(reason.into());
        }
        self.cancel();
    }

    /// 请求「只跳过当前任务」（批量里继续下一个）
    ///
    /// 与 [`Self::cancel`] 的区别：取消是整批停下，跳过只影响正在跑的这个。
    /// 生效点：下一次 `is_cancelled()` 检查（每个条目/数据块一次）。
    pub fn skip(&self) {
        self.skip.store(true, Ordering::Relaxed);
    }

    /// 取走并清掉「跳过」标记（批量循环在每个任务结束时调一次）
    pub fn take_skip(&self) -> bool {
        self.skip.swap(false, Ordering::Relaxed)
    }

    /// 是否已被请求停下（**包含**「跳过当前」——对解压循环来说都是停下）
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed) || self.skip.load(Ordering::Relaxed)
    }

    /// 取消原因（如果有）
    pub fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|r| r.clone())
    }

    /// 裸标志 —— 交给 `io::interrupt::InterruptibleReader` 使用
    ///
    /// 这样取消检查能下沉到读取层：大文件解压时每 1 MiB 检查一次，
    /// 用户点取消后不会「卡在那里没反应」（三层审查 B1）。
    pub fn raw_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }
}

// ───────────────────────────── 进度快照 ─────────────────────────────

/// 某一时刻的进度读数（普通数据，可跨线程/跨语言传递）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressView {
    /// 已完成字节
    pub done_bytes: u64,
    /// 总字节（0 = 还不知道，界面应显示「不确定进度」）
    pub total_bytes: u64,
    /// 已完成条目数
    pub done_files: u64,
    /// 总条目数（0 = 还不知道）
    pub total_files: u64,
}

impl ProgressView {
    /// 完成比例（0.0~1.0）；总字节未知时返回 `None`
    pub fn fraction(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            return None;
        }
        Some((self.done_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0))
    }

    /// 是否已完成
    pub fn is_done(&self) -> bool {
        self.total_bytes > 0 && self.done_bytes >= self.total_bytes
    }
}

/// 无锁进度快照（跨线程共享）
///
/// 所有字段都是 `AtomicU64`：写入方 `fetch_add` 累加，读取方随时拿到一份一致的读数，
/// **双方都不需要加锁**。
#[derive(Debug, Default)]
pub struct ProgressSnapshot {
    done_bytes: AtomicU64,
    total_bytes: AtomicU64,
    done_files: AtomicU64,
    total_files: AtomicU64,
}

impl ProgressSnapshot {
    /// 新建并放入 `Arc`（引擎各处共享同一个实例）
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 设置总字节（解压前由元数据算出；未知则保持 0）
    pub fn set_total_bytes(&self, n: u64) {
        self.total_bytes.store(n, Ordering::Relaxed);
    }

    /// 设置总条目数
    pub fn set_total_files(&self, n: u64) {
        self.total_files.store(n, Ordering::Relaxed);
    }

    /// 累加已写出字节
    pub fn add_bytes(&self, n: u64) {
        self.done_bytes.fetch_add(n, Ordering::Relaxed);
    }

    /// 累加已完成条目
    pub fn add_file(&self) {
        self.done_files.fetch_add(1, Ordering::Relaxed);
    }

    /// 读取一份一致的读数
    pub fn view(&self) -> ProgressView {
        ProgressView {
            done_bytes: self.done_bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            done_files: self.done_files.load(Ordering::Relaxed),
            total_files: self.total_files.load(Ordering::Relaxed),
        }
    }

    /// 完成比例快捷读取
    pub fn fraction(&self) -> Option<f64> {
        self.view().fraction()
    }

    /// 直接写入已完成字节（用于「段结束补齐」，不常用）
    pub fn set_done_bytes(&self, n: u64) {
        self.done_bytes.store(n, Ordering::Relaxed);
    }
}

// ───────────────────────────── 多任务聚合 ─────────────────────────────

#[derive(Debug, Clone, Copy)]
struct JobSlot {
    id: u64,
    last_bytes: u64,
    last_files: u64,
}

/// 多任务进度聚合器
///
/// 每个任务各自上报「自己的累计完成量」，聚合器算出**增量**累加到全局。
/// 上层因此只见到一条单调递增的进度流，任务切换时不会跳变或回退（台账 §2.6）。
#[derive(Debug, Default)]
pub struct ProgressAggregator {
    totals: ProgressSnapshot,
    slots: Mutex<Vec<JobSlot>>,
}

impl ProgressAggregator {
    /// 新建并放入 `Arc`
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 登记一个任务（总字节/总条目未知就传 0）
    pub fn register(&self, job: u64, total_bytes: u64, total_files: u64) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.push(JobSlot {
                id: job,
                last_bytes: 0,
                last_files: 0,
            });
        }
        self.totals
            .total_bytes
            .fetch_add(total_bytes, Ordering::Relaxed);
        self.totals
            .total_files
            .fetch_add(total_files, Ordering::Relaxed);
    }

    /// 上报某任务的累计完成量，返回聚合后的全局读数
    pub fn report(&self, job: u64, done_bytes: u64, done_files: u64) -> ProgressView {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.iter_mut().find(|s| s.id == job)
        {
            let db = done_bytes.saturating_sub(slot.last_bytes);
            let df = done_files.saturating_sub(slot.last_files);
            slot.last_bytes = done_bytes.max(slot.last_bytes);
            slot.last_files = done_files.max(slot.last_files);
            drop(slots);
            if db > 0 {
                self.totals.add_bytes(db);
            }
            if df > 0 {
                self.totals.done_files.fetch_add(df, Ordering::Relaxed);
            }
        }
        self.totals.view()
    }

    /// 任务结束（从聚合中移除，避免内存里堆积已完成任务）
    pub fn unregister(&self, job: u64) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.retain(|s| s.id != job);
        }
    }

    /// 当前全局读数
    pub fn view(&self) -> ProgressView {
        self.totals.view()
    }
}

// ───────────────────────────── 嵌套进度折算 ─────────────────────────────

/// 嵌套进度的折算器（RAII）
///
/// 伪装包常见「ZIP 里套 7z」：内层解压的进度必须折算成**外层段内**的进度，
/// 否则界面会看到进度条跑到一半又归零（台账 §3.5，7-Zip 用 `CLocalProgress` 解决）。
///
/// 与 7-Zip 的区别：它靠 `Init/Clear` 手动配对（漏一个就错），
/// 这里靠 `Drop` 自动收尾 —— 段结束时会**把本段剩余补齐**，进度条不会卡在 99%。
pub struct LocalProgress {
    base: Arc<ProgressSnapshot>,
    offset: u64,
    span: u64,
    last_local: AtomicU64,
}

impl LocalProgress {
    /// 在全局进度上划出一段 `[offset, offset+span)` 给内层使用
    pub fn new(base: Arc<ProgressSnapshot>, offset: u64, span: u64) -> Self {
        Self {
            base,
            offset,
            span,
            last_local: AtomicU64::new(0),
        }
    }

    /// 上报段内已完成量（折算成全局**绝对**进度；重复/回退的上报会被忽略）
    ///
    /// 全局进度取绝对口径：`offset + local`。这样「外层已经推进到 offset」与
    /// 「本段从 offset 开始」两种情形都自洽 —— 若 base 里已有独立贡献者，
    /// 差值为 0，不会重复计数。
    pub fn report(&self, local_done: u64) {
        let local = local_done.min(self.span);
        let last = self.last_local.load(Ordering::Relaxed);
        if local <= last {
            return;
        }
        self.last_local.store(local, Ordering::Relaxed);
        self.advance_to(self.offset + local);
    }

    /// 段结束：把剩余部分补齐（`Drop` 时自动调用）
    pub fn finish(&self) {
        let last = self.last_local.load(Ordering::Relaxed);
        if last < self.span {
            self.last_local.store(self.span, Ordering::Relaxed);
            self.advance_to(self.offset + self.span);
        }
    }

    /// 把 base 推进到 `target`（只增不减，避免覆盖其他贡献者的进度）
    fn advance_to(&self, target: u64) {
        let cur = self.base.view().done_bytes;
        if target > cur {
            self.base.add_bytes(target - cur);
        }
    }

    /// 段在全局中的起点
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// 段长度
    pub fn span(&self) -> u64 {
        self.span
    }
}

impl Drop for LocalProgress {
    fn drop(&mut self) {
        self.finish();
    }
}

// ───────────────────────────── 剩余时间估算 ─────────────────────────────

/// 一次估算的结果
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Eta {
    /// 速度（字节/秒），样本不足时为 `None`
    pub speed_bps: Option<f64>,
    /// 预计剩余时间，样本不足或总量未知时为 `None`
    pub remaining: Option<Duration>,
}

/// 剩余时间估算器
///
/// 7-Zip 的平滑算法和 UI 代码混在一起、不可复用（台账 §6.7），这里单独写一个干净的：
/// - 指数滑动平均（`α = 0.3`），数字不会乱跳
/// - **前 1 秒 / 少于 3 个样本不给估算** —— 估出来是错的，不如不显示（UI-SPEC 原则 5）
#[derive(Debug)]
pub struct EtaEstimator {
    last_t: Instant,
    last_done: u64,
    speed: f64,
    samples: u32,
    alpha: f64,
}

impl Default for EtaEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl EtaEstimator {
    /// 新建（`α = 0.3` 的指数滑动平均）
    pub fn new() -> Self {
        Self {
            last_t: Instant::now(),
            last_done: 0,
            speed: 0.0,
            samples: 0,
            alpha: 0.3,
        }
    }

    /// 自定义平滑系数（测试用）
    pub fn with_alpha(alpha: f64) -> Self {
        let mut s = Self::new();
        s.alpha = alpha.clamp(0.01, 1.0);
        s
    }

    /// 喂一次读数，返回本次估算
    pub fn observe(&mut self, done_bytes: u64, total_bytes: u64) -> Eta {
        let now = Instant::now();
        let dt = now.duration_since(self.last_t).as_secs_f64();

        // 采样窗口太短（<100ms）不算速度，避免 dt→0 导致速度爆表
        if dt >= 0.1 {
            let db = done_bytes.saturating_sub(self.last_done) as f64;
            let inst = db / dt;
            self.speed = if self.samples == 0 {
                inst
            } else {
                self.alpha * inst + (1.0 - self.alpha) * self.speed
            };
            self.samples += 1;
            self.last_t = now;
            self.last_done = done_bytes;
        }

        let enough = self.samples >= 3 && self.speed > 0.0;
        let speed_bps = enough.then_some(self.speed);
        let remaining = if enough && total_bytes > done_bytes {
            let left = (total_bytes - done_bytes) as f64 / self.speed;
            // 超过 24 小时的估算没有意义（说明速度几乎为 0），不显示
            (left.is_finite() && left < 86_400.0).then(|| Duration::from_secs_f64(left))
        } else {
            None
        };

        Eta {
            speed_bps,
            remaining,
        }
    }

    /// 已知速度时的剩余时间（不采样，仅供测试/快捷使用）
    pub fn remaining_for(speed_bps: f64, done: u64, total: u64) -> Option<Duration> {
        if speed_bps <= 0.0 || total <= done {
            return None;
        }
        Some(Duration::from_secs_f64((total - done) as f64 / speed_bps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn 取消令牌默认未取消() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        assert!(t.reason().is_none());
    }

    #[test]
    fn 取消后带原因可读回() {
        let t = CancelToken::new();
        t.cancel_with("user pressed cancel");
        assert!(t.is_cancelled());
        assert_eq!(t.reason().as_deref(), Some("user pressed cancel"));
    }

    #[test]
    fn 克隆的令牌共享同一个标志() {
        let a = CancelToken::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled(), "克隆体取消后原件也应当看到");
    }

    #[test]
    fn 裸标志可交给读取层() {
        let t = CancelToken::new();
        let flag = t.raw_flag();
        assert!(!flag.load(Ordering::Relaxed));
        t.cancel();
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn 进度快照累加与比例() {
        let s = ProgressSnapshot::new();
        s.set_total_bytes(1000);
        s.add_bytes(250);
        s.add_bytes(250);
        assert_eq!(s.view().done_bytes, 500);
        assert_eq!(s.fraction(), Some(0.5));
        assert!(!s.view().is_done());
        s.add_bytes(500);
        assert!(s.view().is_done());
    }

    #[test]
    fn 总量未知时不给比例() {
        let s = ProgressSnapshot::new();
        s.add_bytes(10);
        assert_eq!(s.fraction(), None, "总量未知 → 界面显示不确定进度");
    }

    #[test]
    fn 进度快照可并发累加不丢数() {
        let s = ProgressSnapshot::new();
        let mut hs = Vec::new();
        for _ in 0..4 {
            let s = Arc::clone(&s);
            hs.push(thread::spawn(move || {
                for _ in 0..1000 {
                    s.add_bytes(1);
                    s.add_file();
                }
            }));
        }
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(s.view().done_bytes, 4000);
        assert_eq!(s.view().done_files, 4000);
    }

    #[test]
    fn 多任务聚合单调且不重复计数() {
        let agg = ProgressAggregator::new();
        agg.register(1, 100, 1);
        agg.register(2, 100, 1);

        let v = agg.report(1, 50, 0);
        assert_eq!(v.done_bytes, 50);
        // 同一任务重复上报同一个值 → 不应重复累加
        let v = agg.report(1, 50, 0);
        assert_eq!(v.done_bytes, 50);
        // 第二个任务推进 → 总和继续增长，第一个任务的贡献保留
        let v = agg.report(2, 70, 0);
        assert_eq!(v.done_bytes, 120);
        assert_eq!(v.total_bytes, 200);
    }

    #[test]
    fn 任务注销后不再影响总量() {
        let agg = ProgressAggregator::new();
        agg.register(1, 10, 0);
        agg.register(2, 10, 0);
        agg.report(1, 10, 0);
        agg.unregister(1);
        let v = agg.report(2, 4, 0);
        assert_eq!(v.done_bytes, 14);
    }

    #[test]
    fn 嵌套进度折算到全局() {
        let base = ProgressSnapshot::new();
        base.set_total_bytes(1000);
        let seg = LocalProgress::new(Arc::clone(&base), 200, 400);

        seg.report(0);
        assert_eq!(base.view().done_bytes, 0, "无进展时不应当冒进到 offset");
        seg.report(200); // 段内 50% → 全局 = offset(200) + 200
        assert_eq!(base.view().done_bytes, 400);
        seg.report(150); // 回退的上报被忽略
        assert_eq!(base.view().done_bytes, 400);

        drop(seg); // RAII：段结束自动补齐剩余
        assert_eq!(base.view().done_bytes, 600);
    }

    #[test]
    fn 段内超出长度被截断() {
        let base = ProgressSnapshot::new();
        let seg = LocalProgress::new(Arc::clone(&base), 0, 100);
        seg.report(500);
        assert_eq!(base.view().done_bytes, 100);
    }

    #[test]
    fn 剩余时间样本不足时不给估算() {
        let mut e = EtaEstimator::new();
        let r = e.observe(100, 1000);
        assert!(r.remaining.is_none(), "只有 1 个样本时不应当给估算");
        assert!(r.speed_bps.is_none());
    }

    #[test]
    fn 稳定速度后给出合理估算() {
        let mut e = EtaEstimator::with_alpha(1.0);
        // 模拟「每秒 100 字节」
        let mut done = 0u64;
        let mut last = None;
        for _ in 0..5 {
            thread::sleep(Duration::from_millis(120));
            done += 12;
            last = Some(e.observe(done, 1200));
        }
        let r = last.unwrap();
        let speed = r.speed_bps.expect("样本够了应当有速度");
        assert!(speed > 10.0, "速度估算应当接近真实值，实测 {speed}");
        assert!(r.remaining.is_some(), "总量已知时应当给出剩余时间");
    }

    #[test]
    fn 剩余时间快捷计算() {
        assert_eq!(
            EtaEstimator::remaining_for(100.0, 200, 1200),
            Some(Duration::from_secs(10))
        );
        assert_eq!(EtaEstimator::remaining_for(0.0, 0, 100), None);
        assert_eq!(EtaEstimator::remaining_for(10.0, 100, 100), None);
    }

    #[test]
    fn 进度读数可序列化() {
        let v = ProgressView {
            done_bytes: 1,
            total_bytes: 2,
            done_files: 3,
            total_files: 4,
        };
        let j = serde_json::to_string(&v).unwrap();
        let back: ProgressView = serde_json::from_str(&j).unwrap();
        assert_eq!(v, back);
    }
}
