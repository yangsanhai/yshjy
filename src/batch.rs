//! 批量队列：给一堆文件排队处理，一个坏了不影响其余
//!
//! # 为什么这一层必须单独做（见 `docs/V0.5-ENGINE.md` §6）
//!
//! 用户在网盘上经常是「一次下 8 个包」。逐个手敲命令太笨，
//! 而界面 S13 的批量队列要求：每个任务独立开跑、独立成败、独立可取消。
//!
//! 特别注意：**批量模式的「成功率」必须是真的**。隐写者的原版在这里有个知名 bug
//! ——`reveal_file` 没有返回值，批量成功率恒为 100%（见 `refs/analysis/00-总览与关键发现.md` §3）。
//! 所以这里每个任务都必须带回真实 [`Outcome`]。
//!
//! # 并行的纪律（台账 `7zip-工程层.md` §2.5）
//!
//! 并行路径**必须有无条件退回串行的能力**：`jobs` 默认就是 1（顺序，对磁盘最友好），
//! 只有明确要求并行时才开线程。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::error::{exit_code, ArchiveFlags, ItemResult, Outcome};
use crate::event::{Event, EventSink};
use crate::progress::CancelToken;
use crate::select::Selection;

/// 单个任务的状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// 排队中
    Queued,
    /// 正在跑（带当前阶段名）
    Running(String),
    /// 有结论了
    Done(Outcome),
    /// 没跑（原因：取消 / 前一个失败后停止 / 输入重复 …）
    Skipped(String),
}

/// 一个待处理的任务
#[derive(Debug, Clone)]
pub struct Job {
    /// 任务号（事件里用它区分）
    pub id: u64,
    /// 输入文件
    pub input: PathBuf,
    /// 文件大小（0 = 未知）
    pub size: u64,
    /// 输出目录（`None` = 用默认策略）
    pub dest: Option<PathBuf>,
    /// 状态
    pub state: JobState,
}

impl Job {
    /// 新建一个排队中的任务
    pub fn new(id: u64, input: impl Into<PathBuf>, size: u64) -> Self {
        Self {
            id,
            input: input.into(),
            size,
            dest: None,
            state: JobState::Queued,
        }
    }

    /// 指定输出目录
    pub fn with_dest(mut self, dest: impl Into<PathBuf>) -> Self {
        self.dest = Some(dest.into());
        self
    }
}

/// 单个任务的统计
#[derive(Debug, Clone, Default)]
pub struct JobStats {
    /// 成功条目数
    pub entries_done: u64,
    /// 总条目数
    pub entries_total: u64,
    /// 写出的字节数
    pub bytes_written: u64,
    /// 失败条目（路径 + 结果 + **人话原因**）
    ///
    /// 第三项就是 D8 要的 `detail`：界面/脚本不必再自己猜「为什么失败」。
    pub failed: Vec<(String, ItemResult, Option<String>)>,
    /// 实际输出目录
    pub dest: Option<PathBuf>,
    /// 归档级诊断位
    pub flags: ArchiveFlags,
    /// 耗时（毫秒）
    pub millis: u64,
}

/// 单个任务的结论 + 统计
#[derive(Debug, Clone)]
pub struct JobOutcome {
    /// 任务号
    pub id: u64,
    /// 输入文件
    pub input: PathBuf,
    /// 结论
    pub outcome: Outcome,
    /// 统计
    pub stats: JobStats,
    /// 中性说明（日志用，不是用户文案）
    pub note: Option<String>,
}

impl JobOutcome {
    /// 用结论构造一个最小结果（测试与快速路径用）
    pub fn minimal(id: u64, input: impl Into<PathBuf>, outcome: Outcome) -> Self {
        Self {
            id,
            input: input.into(),
            outcome,
            stats: JobStats::default(),
            note: None,
        }
    }
}

/// 批量选项
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// 并行任务数（默认 1：顺序跑，对磁盘最友好）
    pub jobs: usize,
    /// 一个失败后是否继续（默认继续）
    pub continue_on_error: bool,
    /// 解压码（所有任务共用；每个包不一样时用 `None` 让引擎自己找）
    pub password: Option<String>,
    /// 挑哪些条目
    pub selection: Selection,
    /// 统一输出目录（`None` = 每个包解到自己的同名目录）
    pub dest: Option<PathBuf>,
    /// 智能解压三规则（UI-SPEC §9.2）：单一顶层目录/单文件直接解到目标目录，
    /// 否则自动建同名子目录。默认开。
    pub smart_dir: bool,
    /// 跳过解压前的磁盘空间预检（`--force`）
    ///
    /// 预检只在**预估得出总大小时**才拦人（拿不准就放行，见 [`crate::safety::check_space`]）。
    pub force: bool,
    /// 安全限值（来自配置文件：压缩比上限、单次写出上限等）
    pub limits: crate::safety::Limits,
    /// 手动指定文件名编码（`gbk`/`big5`/`sjis`/`utf-8`；`None` = 自动链）
    pub encoding: Option<String>,
    /// 只测不写（`--test` / 协议 `verify_only`）：完整解码 + 校验，但**一个字节都不落盘**
    pub verify_only: bool,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            jobs: 1,
            continue_on_error: true,
            password: None,
            selection: Selection::all(),
            dest: None,
            smart_dir: true,
            force: false,
            limits: crate::safety::Limits::default(),
            encoding: None,
            verify_only: false,
        }
    }
}

/// 批量报告
#[derive(Debug, Clone, Default)]
pub struct BatchReport {
    /// 每个任务的结果（按任务号排序）
    pub jobs: Vec<JobOutcome>,
}

impl BatchReport {
    /// 任务总数
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// 是否没有任务
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// 完全成功的任务数
    pub fn succeeded(&self) -> usize {
        self.jobs.iter().filter(|j| j.outcome.is_success()).count()
    }

    /// 部分成功的任务数
    pub fn partial(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| matches!(j.outcome, Outcome::Partial { .. }))
            .count()
    }

    /// 失败的任务数
    pub fn failed(&self) -> usize {
        self.jobs.iter().filter(|j| j.outcome.is_failure()).count()
    }

    /// 被取消的任务数（用户取消 + 因取消而没跑的）
    pub fn cancelled(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| matches!(j.outcome, Outcome::Cancelled { .. }))
            .count()
    }

    /// 写出的总字节数
    pub fn bytes_written(&self) -> u64 {
        self.jobs.iter().map(|j| j.stats.bytes_written).sum()
    }

    /// 失败的任务清单（给用户看「哪几个没成」）
    pub fn failures(&self) -> Vec<&JobOutcome> {
        self.jobs.iter().filter(|j| j.outcome.is_failure()).collect()
    }

    /// 中性汇总（日志用；用户文案由外壳渲染）
    pub fn summary(&self) -> String {
        format!(
            "total={} ok={} partial={} failed={} cancelled={} bytes={}",
            self.len(),
            self.succeeded(),
            self.partial(),
            self.failed(),
            self.cancelled(),
            self.bytes_written()
        )
    }

    /// 对外退出码（引擎统一算，见 `crate::error::exit_code`）
    ///
    /// 规则：
    /// - 全成功 → 0
    /// - 有失败 → 若也有成功/部分成功则 `PARTIAL`(5)，否则取第一个失败的码
    /// - 没有失败但有取消 → 130
    pub fn exit_code(&self) -> i32 {
        if self.is_empty() {
            return exit_code::OK;
        }
        if self.failed() > 0 {
            let any_good = self.succeeded() + self.partial() + self.cancelled() > 0;
            if any_good {
                return exit_code::PARTIAL;
            }
            if let Some(first) = self.jobs.iter().find(|j| j.outcome.is_failure()) {
                return first.outcome.exit_code();
            }
        }
        if self.cancelled() > 0 {
            return exit_code::CANCELLED;
        }
        if self.partial() > 0 {
            return exit_code::PARTIAL;
        }
        exit_code::OK
    }
}

// ───────────────────────────── 输入展开 ─────────────────────────────

/// 目录递归的最大层数（防止在巨型目录树上跑太久）
pub const MAX_SCAN_DEPTH: usize = 4;

/// 判断一个文件值不值得处理（按扩展名）
pub fn looks_like_input(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    // 分卷：`x.7z.001` / `x.001` / `x.part1.rar`
    if name.ends_with(".001") || name.ends_with(".part1.rar") {
        return true;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v"
            | "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "zst" | "bz2" | "tgz" | "txz"
            | "jpg" | "png" | "gif" | "iso"
    )
}

/// 展开输入：文件原样、目录递归找可疑文件
///
/// - 目录按 `MAX_SCAN_DEPTH` 限深递归
/// - 结果**按体积倒序**（大文件先跑，总完成时间更短）
/// - 去重（同一个文件被给两次只算一次）
pub fn expand_inputs(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for p in paths {
        if p.is_dir() {
            collect_dir(p, 0, &mut out, &mut seen);
        } else if seen.insert(p.clone()) {
            out.push(p.clone());
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(file_size(p)));
    归并分卷(out)
}

/// 分卷归并：同一组 `x.7z.001/.002/…`、`x.z01/z02`、`x.part1.rar/part2` 只保留一块
///
/// 为什么必须做：目录里有 `.001/.002/.003` 时，逐个展开会让**同一个包被解三遍**
/// （每一块都能认出整组），用户白等三倍时间、还看到三份结果。
/// 归并后交给引擎的那一块，`as_7z_volumes` / `as_zip_volumes` 会自动把整组找齐。
fn 归并分卷(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut 已见组: HashSet<String> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        match 分卷组键(&p) {
            Some(组键) => {
                if 已见组.insert(组键) {
                    out.push(p);
                }
            }
            None => out.push(p),
        }
    }
    out
}

/// 若这个文件是「分卷命名」，返回整组的键（目录 + 主干名）
///
/// 认这三种写法（都是实际下载会遇到的）：
/// - `x.7z.001` / `x.zip.001` / `x.001`（三位以上数字）
/// - `x.z01` / `x.z02`（ZIP 分卷）
/// - `x.part1.rar` / `x.part01.rar`（RAR 分卷）
fn 分卷组键(p: &Path) -> Option<String> {
    let name = p.file_name()?.to_string_lossy().to_string();
    let lower = name.to_ascii_lowercase();
    let dir = p.parent().unwrap_or_else(|| Path::new("."));
    let 组 = |主干: &str| format!("{}/{}", dir.display(), 主干);

    // .partN.rar / .partN
    if let Some(pos) = lower.rfind(".part") {
        let rest = &lower[pos + 5..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num.is_empty() && rest[num.len()..].starts_with(".rar") {
            return Some(组(&name[..pos]));
        }
    }
    // .zNN（ZIP 分卷）
    if let Some(pos) = lower.rfind(".z") {
        let rest = &lower[pos + 2..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(组(&name[..pos]));
        }
    }
    // .NNN（三位以上数字；两位的 .01 也可能是分卷，但太容易误伤，不认）
    if let Some(pos) = lower.rfind('.') {
        let rest = &lower[pos + 1..];
        if rest.len() >= 3 && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(组(&name[..pos]));
        }
    }
    None
}

fn collect_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth + 1 < MAX_SCAN_DEPTH {
                collect_dir(&path, depth + 1, out, seen);
            }
            continue;
        }
        if looks_like_input(&path) && seen.insert(path.clone()) {
            out.push(path);
        }
    }
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

// ───────────────────────────── 执行 ─────────────────────────────

/// 跑一批任务
///
/// `run_one` 由调用方注入（引擎的单任务编排在 [`crate::job`]），这样批量层
/// 不需要知道「怎么解一个包」，只负责**排队、并行、汇总、取消**。
///
/// - `jobs = 1`：顺序跑
/// - `jobs > 1`：分片并行（结果按任务号排序，输出与顺序跑一致）
/// - 取消：正在跑的任务会自己中断；**还没开始的直接标记跳过**，不再启动
/// - `continue_on_error = false`：首个失败后，剩余任务标记跳过
pub fn run_batch<F>(
    mut tasks: Vec<Job>,
    opts: &BatchOptions,
    sink: &Arc<dyn EventSink>,
    cancel: &CancelToken,
    run_one: F,
) -> BatchReport
where
    F: Fn(&Job, &BatchOptions, &Arc<dyn EventSink>, &CancelToken) -> JobOutcome + Sync,
{
    tasks.sort_by_key(|t| t.id);
    let n = opts.jobs.max(1);

    let outcomes: Vec<JobOutcome> = if n == 1 || tasks.len() <= 1 {
        run_sequential(&tasks, opts, sink, cancel, &run_one)
    } else {
        run_parallel(&tasks, opts, sink, cancel, &run_one, n)
    };

    let mut report = BatchReport { jobs: outcomes };
    report.jobs.sort_by_key(|j| j.id);
    report
}

fn run_sequential<F>(
    tasks: &[Job],
    opts: &BatchOptions,
    sink: &Arc<dyn EventSink>,
    cancel: &CancelToken,
    run_one: &F,
) -> Vec<JobOutcome>
where
    F: Fn(&Job, &BatchOptions, &Arc<dyn EventSink>, &CancelToken) -> JobOutcome + Sync,
{
    let mut out = Vec::with_capacity(tasks.len());
    let mut stopped = false;
    for job in tasks {
        // D5：界面单独取消了这一行 → 只跳过它，其它任务照跑（不是整批取消）
        if cancel.take_job_skip(job.id) {
            out.push(skipped(job, "cancelled by user (queue)"));
            continue;
        }
        // 「跳过」在排队时按下 → 这一项直接跳过（还没开始跑）
        if cancel.take_skip() {
            out.push(skipped(job, "skipped by user (before start)"));
            continue;
        }
        if cancel.is_cancelled() {
            out.push(skipped(job, "cancelled before start"));
            continue;
        }
        if stopped {
            out.push(skipped(job, "stopped after previous failure"));
            continue;
        }

        sink.emit(&Event::JobStarted {
            job: job.id,
            path: job.input.to_string_lossy().to_string(),
            size: job.size,
        });
        // D5：记下「正在跑的是哪一项」—— 界面点这一行的取消才敢真打断它
        cancel.mark_running(job.id);
        let mut result = run_one(job, opts, sink, cancel);
        cancel.clear_running();
        // 「跳过当前」在跑的时候按下：这一项已按取消收尾，把标记清掉，
        // **不要让下一项也被跳过**（skip 只影响它按下的那一刻）
        if cancel.take_skip() {
            result.note = Some("skipped by user".to_string());
        }
        sink.emit(&Event::JobFinished {
            job: job.id,
            outcome: result.outcome.clone(),
        });

        if result.outcome.is_failure() && !opts.continue_on_error {
            stopped = true;
        }
        out.push(result);
    }
    out
}

fn run_parallel<F>(
    tasks: &[Job],
    opts: &BatchOptions,
    sink: &Arc<dyn EventSink>,
    cancel: &CancelToken,
    run_one: &F,
    jobs: usize,
) -> Vec<JobOutcome>
where
    F: Fn(&Job, &BatchOptions, &Arc<dyn EventSink>, &CancelToken) -> JobOutcome + Sync,
{
    // 简单分片：把任务按轮转分配到 N 个线程，线程内顺序跑。
    // 不做工作窃取 —— 任务数量级是「几十个包」，分片足够（简单胜过聪明）。
    let shards: Vec<Vec<Job>> = (0..jobs)
        .map(|k| {
            tasks
                .iter()
                .enumerate()
                .filter(|(i, _)| i % jobs == k)
                .map(|(_, j)| j.clone())
                .collect()
        })
        .collect();

    let results: Vec<Vec<JobOutcome>> = std::thread::scope(|scope| {
        let handles: Vec<_> = shards
            .into_iter()
            .map(|shard| {
                scope.spawn(move || {
                    let mut local = Vec::with_capacity(shard.len());
                    for job in &shard {
                        // D5：这一行被单独取消 → 只跳过它，本线程继续跑下一项
                        if cancel.take_job_skip(job.id) {
                            local.push(skipped(job, "cancelled by user (queue)"));
                            continue;
                        }
                        // 取消或已停止：本线程直接跳过剩余
                        if cancel.is_cancelled() {
                            local.push(skipped(job, "cancelled before start"));
                            continue;
                        }
                        if let Some(prev_fail) = local
                            .iter()
                            .find(|r: &&JobOutcome| r.outcome.is_failure())
                            .filter(|_| !opts.continue_on_error)
                        {
                            let _ = prev_fail;
                            local.push(skipped(job, "stopped after previous failure"));
                            continue;
                        }
                        sink.emit(&Event::JobStarted {
                            job: job.id,
                            path: job.input.to_string_lossy().to_string(),
                            size: job.size,
                        });
                        // D5：并行时这里只能记「最后一个开始的」——够用，
                        // 因为并行下用 skip 打断本来就只能打到一个线程的当前任务
                        cancel.mark_running(job.id);
                        let r = run_one(job, opts, sink, cancel);
                        cancel.clear_running();
                        sink.emit(&Event::JobFinished {
                            job: job.id,
                            outcome: r.outcome.clone(),
                        });
                        local.push(r);
                    }
                    local
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    results.into_iter().flatten().collect()
}

fn skipped(job: &Job, reason: &str) -> JobOutcome {
    JobOutcome {
        id: job.id,
        input: job.input.clone(),
        outcome: Outcome::Cancelled { done: 0, total: 0 },
        stats: JobStats::default(),
        note: Some(reason.to_string()),
    }
}

/// 把一批路径变成任务列表（`file_size` 顺带读出来，便于界面显示）
pub fn make_jobs(paths: &[PathBuf]) -> Vec<Job> {
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| Job::new(i as u64 + 1, p.clone(), file_size(p)))
        .collect()
}

/// 计时助手（统计耗时用）
pub fn elapsed_millis(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ErrorCode, Outcome};
    use crate::event::NullSink;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn 造任务(n: u64) -> Vec<Job> {
        (1..=n)
            .map(|i| Job::new(i, format!("/tmp/包{i}.mp4"), i * 100))
            .collect()
    }

    /// 批量测试用的空出口（Arc 形态，与 run_batch 的签名一致）
    fn 空出口() -> Arc<dyn EventSink> {
        Arc::new(NullSink)
    }

    #[test]
    fn 界面单独取消某一行只跳过那一条() {
        // D5：用户点第 2 行的「取消」→ 只跳过它，第 1、3 行照跑（不是整批停）
        let sink = 空出口();
        let token = CancelToken::new();
        token.cancel_job(2);
        let report = run_batch(
            造任务(3),
            &BatchOptions::default(),
            &sink,
            &token,
            |job, _, _, _| JobOutcome::minimal(job.id, job.input.clone(), Outcome::Success),
        );
        assert_eq!(report.len(), 3);
        assert_eq!(report.succeeded(), 2, "第 1、3 行应该照跑成功");
        assert_eq!(report.cancelled(), 1, "第 2 行按取消算（不是失败）");
        let 第二 = report.jobs.iter().find(|j| j.id == 2).expect("第 2 行");
        assert!(matches!(第二.outcome, Outcome::Cancelled { .. }));
        assert_eq!(token.pending_job_skips(), 0, "标记用过就该清掉");
    }

    #[test]
    fn 运行中的任务号记得住也清得掉() {
        let token = CancelToken::new();
        assert_eq!(token.running_job(), None);
        token.mark_running(7);
        assert_eq!(token.running_job(), Some(7));
        token.clear_running();
        assert_eq!(token.running_job(), None);
    }

    #[test]
    fn 顺序跑全部成功() {
        let sink = 空出口();
        let token = CancelToken::new();
        let report = run_batch(
            造任务(3),
            &BatchOptions::default(),
            &sink,
            &token,
            |job, _, _, _| JobOutcome::minimal(job.id, job.input.clone(), Outcome::Success),
        );
        assert_eq!(report.len(), 3);
        assert_eq!(report.succeeded(), 3);
        assert_eq!(report.exit_code(), exit_code::OK);
        assert_eq!(report.summary(), "total=3 ok=3 partial=0 failed=0 cancelled=0 bytes=0");
    }

    #[test]
    fn 一个失败不影响其余() {
        let sink = 空出口();
        let token = CancelToken::new();
        let report = run_batch(
            造任务(3),
            &BatchOptions::default(),
            &sink,
            &token,
            |job, _, _, _| {
                let o = if job.id == 2 {
                    Outcome::Failed(ErrorCode::NotArchive)
                } else {
                    Outcome::Success
                };
                JobOutcome::minimal(job.id, job.input.clone(), o)
            },
        );
        assert_eq!(report.succeeded(), 2);
        assert_eq!(report.failed(), 1);
        assert_eq!(report.failures().len(), 1);
        assert_eq!(report.failures()[0].id, 2);
        assert_eq!(report.exit_code(), exit_code::PARTIAL, "有成功也有失败 → 部分成功");
    }

    #[test]
    fn 全部失败时取第一个失败码() {
        let sink = 空出口();
        let token = CancelToken::new();
        let report = run_batch(
            造任务(2),
            &BatchOptions::default(),
            &sink,
            &token,
            |job, _, _, _| {
                JobOutcome::minimal(job.id, job.input.clone(), Outcome::Failed(ErrorCode::NotArchive))
            },
        );
        assert_eq!(report.exit_code(), exit_code::NOT_ARCHIVE);
        assert_eq!(report.failed(), 2);
    }

    #[test]
    fn 停下来后剩余任务被跳过() {
        let sink = 空出口();
        let token = CancelToken::new();
        let opts = BatchOptions {
            encoding: None,
            continue_on_error: false,
            ..Default::default()
        };
        let report = run_batch(造任务(3), &opts, &sink, &token, |job, _, _, _| {
            let o = if job.id == 1 {
                Outcome::Failed(ErrorCode::NotArchive)
            } else {
                Outcome::Success
            };
            JobOutcome::minimal(job.id, job.input.clone(), o)
        });
        assert_eq!(report.succeeded(), 0, "第 2、3 个不该被跑");
        let skipped_note = report
            .jobs
            .iter()
            .find(|j| j.id == 2)
            .and_then(|j| j.note.clone())
            .unwrap_or_default();
        assert!(skipped_note.contains("stopped after previous failure"));
    }

    #[test]
    fn 取消后剩余任务不再启动() {
        let sink = 空出口();
        let token = CancelToken::new();
        let report = run_batch(
            造任务(4),
            &BatchOptions::default(),
            &sink,
            &token,
            |job, _, _, cancel: &CancelToken| {
                cancel.cancel_with("测试：第一个就取消");
                JobOutcome::minimal(
                    job.id,
                    job.input.clone(),
                    Outcome::Cancelled { done: 1, total: 9 },
                )
            },
        );
        assert_eq!(report.cancelled(), 4);
        assert_eq!(report.exit_code(), exit_code::CANCELLED);
        let notes: Vec<String> = report.jobs.iter().filter_map(|j| j.note.clone()).collect();
        assert_eq!(notes.len(), 3, "后 3 个应当是被跳过而不是被执行");
    }

    #[test]
    fn 并行结果与顺序一致() {
        let sink = 空出口();
        let token = CancelToken::new();
        let counter = AtomicU64::new(0);
        let opts = BatchOptions {
            encoding: None,
            jobs: 3,
            ..Default::default()
        };
        let report = run_batch(造任务(7), &opts, &sink, &token, |job, _, _, _| {
            counter.fetch_add(1, Ordering::Relaxed);
            JobOutcome::minimal(job.id, job.input.clone(), Outcome::Success)
        });
        assert_eq!(counter.load(Ordering::Relaxed), 7, "每个任务都必须跑到");
        assert_eq!(report.len(), 7);
        let ids: Vec<u64> = report.jobs.iter().map(|j| j.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7], "结果必须按任务号排序");
    }

    #[test]
    fn 批量按计数判断部分成功退出码() {
        let mut report = BatchReport::default();
        report.jobs.push(JobOutcome::minimal(
            1,
            "/tmp/a",
            Outcome::Partial {
                done: 340,
                total: 351,
            },
        ));
        assert_eq!(report.exit_code(), exit_code::PARTIAL);
        assert_eq!(report.partial(), 1);
    }

    #[test]
    fn 空报告退出码为成功() {
        let report = BatchReport::default();
        assert!(report.is_empty());
        assert_eq!(report.exit_code(), exit_code::OK);
    }

    #[test]
    fn 输入展开去重并按体积倒序() {
        let dir = std::env::temp_dir().join("yshjy_batch_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("子目录")).unwrap();
        std::fs::write(dir.join("小.mp4"), b"x").unwrap();
        std::fs::write(dir.join("大.mp4"), vec![0u8; 5000]).unwrap();
        std::fs::write(dir.join("子目录/中.7z.001"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("读我.txt"), "不是包".as_bytes()).unwrap();

        let got = expand_inputs(std::slice::from_ref(&dir));
        let names: Vec<String> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["大.mp4", "中.7z.001", "小.mp4"], "应当倒序且不含 txt");

        // 重复给两次只算一次
        let again = expand_inputs(&[dir.clone(), dir.join("大.mp4")]);
        assert_eq!(again.len(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 判断值得处理的输入() {
        assert!(looks_like_input(Path::new("/x/电影.mp4")));
        assert!(looks_like_input(Path::new("/x/包.7z.001")));
        assert!(looks_like_input(Path::new("/x/包.part1.rar")));
        assert!(looks_like_input(Path::new("/x/包.ZIP")));
        assert!(!looks_like_input(Path::new("/x/说明.txt")));
        assert!(!looks_like_input(Path::new("/x/目录")));
    }
}
