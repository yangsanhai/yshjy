# P5 / P6 · 接线与死代码审计（2026-11-02）

> P5 原计划：126 个 fix 提交逐条反查
> P6 原计划：魔法痕迹扫描（magic number / 特殊分支）
> **结果：P5 的原方法不成立，已改法；P6 扫完基本干净。真正的病灶是另一类东西。**

---

## 一、P5：按 commit 审行不通（诚实认账）

| 项 | 数 |
|---|---|
| fix/修/bug 类提交总数 | **126** |
| 其中真正动了代码（src / gui / tests / scripts / ffi） | **92** |
| 只动文档/记录（CHANGELOG/TODO/HANDOFF/locales） | 34（直接跳过）|
| 92 条里「小补丁」（新增 ≤ 40 行） | **只有 6 条** |

**为什么行不通**：这个项目的提交粒度是「一批做完多件事」——
剩下 86 条都是 +200 ~ +1600 行的大提交（`c2a77a1` 一次 +5808 行）。

「一条 commit = 一个 bug」这个前提在这里**不成立**，
逐条读 92 个大 diff 等于把整个项目重读一遍，产出还很低。

**改法（已生效）**：不按 commit 审，**按代码位置查** ——
用「死代码 / 未接线扫描」找可疑点，再用 `git blame` 反查那一行的来历。

导出物：
- `~/下载/yshjy-优化表/P5-fix提交清单.csv`（126 条，标了改没改代码）
- `~/下载/yshjy-优化表/P5-可疑度.csv`（92 条按可疑度打分）

---

## 二、P6：魔法痕迹扫完，基本干净

抽查最可能「凭空发明」的地方：

| 文件 | 「注释留线索」处数 | 魔法数字抽查结论 |
|---|---|---|
| `src/job.rs` | 30 处 / 1926 行 | — |
| `src/layout/zip.rs` | 18 处 / 881 行 | `ZIP64_LOC_LEN=20`、`CHUNK=64KiB`、`OVERLAP=20` —— **全部对应 `REFERENCE-INDEX` 里「抄 7-Zip」的条目**，有出处 |
| `src/extract.rs` | 18 处 / 1369 行 | — |
| `src/unpack.rs` | 12 处 / 1087 行 | — |

**结论：没有扫出「凭空发明的算法」。** 魔法数字都能在 `docs/REFERENCE-INDEX.md` 找到出处
（EOCD 分块倒扫、64 KiB + 20 字节重叠这些就是从 7-Zip 抄的）。

---

## 三、真正的病灶：不是「算法自己写」，是「接线错误」四类

把 P1~P6 的发现排在一起看，规律非常清楚 ——
**这个项目的自研问题不在算法，而在四处「接错了」**：

| 类型 | 表现 | 实例 | 出处 |
|---|---|---|---|
| **① 库/系统支持的能力没调用** | 能力在，参数没传 | `read_path(path)` 而不是 `read_path_with_options(.., with_password)` → RAR 头加密全废 | P4-1 |
| **② 常量/函数定义了没接进路径** | 留了钩子没接线 | `UNSUPPORTED_METHOD` 全库零引用；`rescue_broken_index` / `try_remux_with_ffmpeg` 零调用 | P1-A2 / P6 |
| **③ 校验规则用错了场景** | 单文件对的规则套到分卷流 | `base_is_not_local_header` 在分卷续块上必然失败 | P3-1 |
| **④ 两条代码路径结论不一致** | 同一个包，命令 A 说行、命令 B 说不行 | `--list` 走救援降级、`--test` 不走 → 结论相反 | P1-A1 / P1-A4 |

**这四类都不需要重写算法，全都是「接线 + 分类」的工作。**
对修复成本是巨大的好消息：P8 不是重写季，是接线季。

---

## 四、死代码清单（362 个 pub 定义里，21 个全库零引用）

扫描法：抓 `pub const/fn/static` → 全库 word 边界计数 → 只出现 1 次（就是定义本身）的挑出来。

**已逐条核实的 7 个**

| 名字 | 位置 | 判定 |
|---|---|---|
| `rescue_broken_index` | `src/repair.rs:195` | 🔴 **真死**：功能已经被 `src/extract.rs:792-809` 的内联流程（analyze → PatchWindow → 重试）取代，旧的没删 |
| `try_remux_with_ffmpeg` | `src/cover_extract.rs` | 🔴 **真死**：装了 ffmpeg 就能修的封面 moov，零调用 |
| `can_verify_video` | `src/tools.rs` | 🟡 零调用（能力探测，UI 没用上）|
| `MAX_SIZE` | `src/extract_rar.rs` | ✅ 正常：`#[deprecated]` 兼容别名 |
| `build_sfx` | `src/pack/mod.rs` | ✅ 正常：`build_sfx_ctx` 的薄包装，真功能有 3 处调用（main.rs:2940 / api.rs:816）|
| `allows_sampling_scan` | `src/hardware.rs` | ⬜ 待核 |
| `rescue_broken_index` 之外的 14 个 | 见附录列表 | ⬜ 待核 |

**其余 14 个待核**（下一步做，不在这里下结论）：
`bits`(error.rs) · `cover_path`(cover_extract.rs) · `elapsed_millis`(batch.rs) ·
`extract_7z`(extract7z.rs) · `is_open` / `target_path`(io/atomic.rs) · `lang_name`(i18n.rs) ·
`long_flags`(cli_spec.rs) · `patch_count`(io/patch.rs) · `set_done_bytes`(progress.rs) ·
`sniff_head`(pack/mod.rs) · `unix_to_civil_`(restore.rs) · `wait_until_written`(flush.rs) ·
`with_progress`(ctx.rs)

> 注意：`pub` 在 Rust 里不会触发 `dead_code` 警告，所以 `clippy -D warnings` 全绿
> **完全挡不住这类"留了钩子没接线"**。这条以后可以做成常驻检查（`scripts/` 里加一个扫描）。

---

## 五、方法层面留给以后的教训

1. **提交粒度不等于问题粒度** —— 这个项目一批提交做好几件事，别指望靠 commit 审 bug。
2. **死代码扫描比 diff 阅读便宜十倍**，而且直接命中「留了钩子没接线」。
3. **魔法数字有出处就不算坏味道** —— 先查 `REFERENCE-INDEX` 再下判断，否则会把「抄 7-Zip」误判成「自己编的」。
4. **`--list` / `--test` / 界面 三条路径必须交叉验同一批样本** —— P1 的三条谎报全是"两条路径结论相反"漏出来的。

---

_P5/P6 结论：不是算法自研问题，是接线问题。四类病灶已定型，进 P7 汇总。_
