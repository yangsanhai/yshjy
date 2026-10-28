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

use std::path::PathBuf;
use std::sync::Arc;

use yshjy::ctx::UnpackCtx;
use yshjy::error::{Error, ErrorCode, Result};
use yshjy::event::{EventSink, NullSink};
use yshjy::select::Selection;

use crate::{人类可读, 文案};

/// 最多列多少条给用户勾（再多翻页也没人看；`--select` 仍能精确取）
const 最多候选: usize = 500;

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

    let 勾选 = dialoguer::MultiSelect::new()
        .with_prompt(提示)
        .items(&显示)
        .defaults(&vec![false; 显示.len()])
        .interact_opt()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("交互勾选出错：{e}")))?;

    match 勾选 {
        None => Ok(None), // 用户 Esc / q
        Some(位置) => {
            // dialoguer 给的是 0-based 下标，转成条目自己的 1-based 序号
            let 序号: Vec<u64> = 位置.iter().map(|&i| 候选[i].index).collect();
            Ok(Some(Selection::only(序号)))
        }
    }
}
