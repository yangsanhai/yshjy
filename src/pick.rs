// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! `--pick`：全屏勾选要解哪些条目（2026-10-28）
//!
//! ## 为什么不手写
//!
//! raw mode、分页、非 TTY 降级、退出复原 —— 这些边界 `dialoguer` 全处理过了。
//! 键位就在 `refs/upstream/dialoguer/src/prompts/multi_select.rs:245-305`：
//!
//! ```text
//! ↓ / Tab / j      下移        ↑ / BackTab / k   上移
//! ← / h            上一页      → / l             下一页
//! 空格             勾选 / 取消  a                 全选 / 全不选
//! Esc / q          取消        Enter             确认
//! ```
//!
//! 手写 60 行只会把这些坑推迟到用户机器上（台账 `docs/REFERENCE-INDEX.md` §8 的教训）。
//!
//! ## 语义
//!
//! `--pick` 只负责「选出哪些序号」，选完仍走 `--select` 那套（`Selection::only`），
//! 所以 `--pick` 勾中第 2、5、9 项与 `--select "2,5,9"` 的结果**逐字节一致**（验收标准）。
//! 序号一律用 `EntryView::index`（1-based，与 `--select` 用号相同）。

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use yshjy::ctx::UnpackCtx;
use yshjy::error::{Error, ErrorCode, Result};
use yshjy::event::{EventSink, NullSink};
use yshjy::select::Selection;

use crate::{人类可读, 文案};

/// 最多列多少条给用户勾（再多翻页也没人看；`--select` 仍能精确取）
const 最多候选: usize = 500;

// ───────────────────────── Ctrl-C 兜底（2026-10-29）─────────────────────────

/// 是否正停在勾选界面里 —— `main::装中断接管` 的 handler 靠它分派
static 在屏中: AtomicBool = AtomicBool::new(false);

/// 这一次 `--pick` 有没有被 Ctrl-C 打断过（主线程与 handler 线程都要看）
static 已中断: AtomicBool = AtomicBool::new(false);

/// 「在屏」标记的守卫：构造时置位，离开函数（正常返回 / Esc / 出错）时复位
struct 屏幕守卫;

impl 屏幕守卫 {
    fn 进() -> Self {
        在屏中.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for 屏幕守卫 {
    fn drop(&mut self) {
        在屏中.store(false, Ordering::SeqCst);
    }
}

/// 把光标显示回来。**重复写无害**（ANSI 序列幂等），
/// 所以 handler 与主线程两边都可以调，不用抢。
fn 显光标() {
    // 跟 dialoguer 同一条流（它默认画在 stderr）；换行让 shell 提示符回到行首
    let mut 流 = std::io::stderr();
    let _ = 流.write_all(b"\x1b[?25h\r\n");
    let _ = 流.flush();
}

/// Ctrl-C 落下时由 `main::装中断接管` 调用。
///
/// 正在勾选界面里 → 把光标显示回来，返回 `true`（调用方随即 `exit(130)`）；
/// 不在界面里 → 返回 `false`，让 handler 交给别的阶段处理。
///
/// 为什么只补光标：走到这里说明 `console` 后端把 Ctrl-C 变成了 SIGINT，而它的 raw mode
/// 是 RAII 的、被信号打断时自己会复原（2026-10-29 pty 实测 lflag / ICANON / ECHO / ISIG
/// 前后完全一致）。**只有光标是 `dialoguer` 画出来的**，复原代码在信号路径上没有机会跑，
/// 于是它一直不回来，用户得 `reset` 才看得见 —— 这里补的就是这一项。
pub fn 中断复原() -> bool {
    已中断.store(true, Ordering::SeqCst);
    if !在屏中.swap(false, Ordering::SeqCst) {
        return false;
    }
    显光标();
    true
}

/// `dialoguer` 报的错是不是「用户按了 Ctrl-C」。
///
/// 有了 SIGINT 接管之后，信号不再直接杀进程：`console` 后端的阻塞读被 `EINTR` 打断，
/// 于是 `interact_opt()` 返回 `IO error: read interrupted`。**这不是出错，是用户取消**，
/// 不能拿它当内部错误报给用户（实测过：不拦的话屏幕上会冒出「❌ 交互勾选出错」）。
///
/// 用「标志 或 错误文本」双保险：两个线程（ctrlc 的等待线程 / 主线程）谁先醒没保证，
/// 只靠标志会有窗口期漏判。
fn 是中断(e: &str) -> bool {
    已中断.load(Ordering::SeqCst) || e.to_ascii_lowercase().contains("interrupt")
}

/// 弹出全屏勾选。
///
/// - `Ok(Some(sel))`：用户选完了（可能一个都没勾 → `Selection::only(vec![])`）
/// - `Ok(None)`：用户按 Esc / q 取消
pub fn 交互勾选(目标: &str, encoding: Option<&str>) -> Result<Option<Selection>> {
    // 预览用静默出口：勾选界面里不该混进进度行
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let ctx = UnpackCtx::new(1, sink).with_encoding(encoding.map(str::to_string));

    let (_t, pv) = yshjy::preview::preview(&PathBuf::from(目标), &ctx, None)?;

    if pv.entries.is_empty() {
        // 列不出内容（比如单流 gz）：交给上层按"全部"处理
        return Ok(Some(Selection::all()));
    }

    let 语 = 文案();
    let 候选: Vec<&yshjy::event::EntryView> = pv.entries.iter().take(最多候选).collect();
    let 显示: Vec<String> = 候选
        .iter()
        .map(|e| {
            let 大小 = if e.size > 0 {
                人类可读(e.size)
            } else {
                语.t("preview.unknown_size", &[])
            };
            let 锁 = if e.encrypted { "🔒 " } else { "" };
            let 目录 = if e.is_dir { "📁 " } else { "" };
            format!("{:>5}.  {:>10}  {锁}{目录}{}", e.index, 大小, e.path)
        })
        .collect();

    let 提示 = 语.t("cli.pick_prompt", &[("input", 目标)]);

    // 进屏：从这里到函数返回，Ctrl-C 走「复原屏幕 + 退 130」那条路（见 `中断复原`）
    let _在屏 = 屏幕守卫::进();

    let 勾选 = match dialoguer::MultiSelect::new()
        .with_prompt(提示)
        .items(&显示)
        .defaults(&vec![false; 显示.len()])
        .interact_opt()
    {
        Ok(位置) => 位置,
        Err(e) => {
            let 文 = e.to_string();
            // Ctrl-C：`console` 后端的读被 EINTR 打断 → 走到这里。
            // 复原光标（handler 那边可能已经抢先写过，幂等）后按 SIGINT 惯例退 130，
            // **不要**当成「交互勾选出错」报给用户。
            if 是中断(&文) {
                在屏中.store(false, Ordering::SeqCst);
                显光标();
                std::process::exit(crate::中断退出码);
            }
            return Err(Error::new(ErrorCode::Internal).context(format!("交互勾选出错：{文}")));
        }
    };

    match 勾选 {
        None => Ok(None), // 用户 Esc / q
        Some(位置) => {
            // dialoguer 给的是 0-based 下标，转成条目自己的 1-based 序号
            let 序号: Vec<u64> = 位置.iter().map(|&i| 候选[i].index).collect();
            Ok(Some(Selection::only(序号)))
        }
    }
}
