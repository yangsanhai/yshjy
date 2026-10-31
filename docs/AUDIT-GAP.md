# 全项目对账：现在有什么 / 还缺什么

> 2026-10-30 生成（**v2**）。方法：分层实测 + 台账逐条对账，不抄文档里的 ✅/❌。
> 本文件是**一次性的对账报告**，不是第二本待办 —— 要做的事仍然只记 `docs/TODO.md`。
>
> **v2 为什么重写**：v1 的界面部分是靠搜关键词撒网（结果漏了"界面多语言"这种整块的）。
> v2 改成三路交叉：**`docs/UI-SPEC.md` 的 14 个状态**（界面设计权威）× **42 个 CLI 选项** × **协议字段**，
> 按状态/字段逐个对，不再靠猜。

---

## 0. 一句话结论

1. **引擎 / 命令行 / 协议三层干净**：42 个长选项全部核到真实读取点；12 种格式能力与 `--caps` 一致；
   协议 11 命令 / 6 能力位 / 7 语言实测可跑；45 个探针 + 12 份契约基准。
2. **界面曾经是唯一短板，2026-10-31 已补齐**：UI-SPEC 14 个状态里 S4 / S5 / S10 三项本轮做完（T4/T5/T8）；
   引擎/协议已有、界面没入口的 7 项全部接通（T7）；界面多语言 7 种、487 条（T14）。
3. **台账大面积失真**：`REFERENCE-INDEX` / `DESIGN` / `GUI-PLAN` 里 **14 条标 ❌/🟡 的其实早做完**。
   这既是"AI 说没做其实做了"的出处，也会让下一轮重复劳动。
4. **一个真死配置**：`[general] default_mode` 解析了、文档承诺了，**全库零读取**。
5. **"越更新越丢东西"查不到证据**：git 没删过任何 `.dart`/`.rs`；界面历史文案差异 51 条逐条核完全是搬家。
   真实体感来源 = 第 2 条（**引擎越更新越强，界面没跟上**）。

---

## 1. 实测基线

| 层 | 实测 | 怎么测的 |
|---|---|---|
| 装机版本 | **0.9.34**（`dpkg -l` 与 `--version` 一致） | 实跑 |
| 引擎 | **53 个 `.rs` / 30,695 行** | `find src` |
| 界面 | **18 个 `.dart` / 6,186 行**，5 页 + 托盘 | `find gui/lib` |
| 格式 | 12 条能力行（zip / zip 分卷 / 7z / rar4 / rar5 / tar / gzip / xz / zstd / bzip2 / brotli / lz4） | `yshjy --caps` |
| 协议 | 11 命令 + 6 能力位 + 7 语言 + `formats` 12 条 | `printf '{"id":1,"cmd":"hello"}\n' \| yshjy --serve` |
| 命令行 | **42 个长选项，逐个核到读取点，零空转** | 扫 `main.rs` 的 `has("--x")` / `val_of("--x")` |
| 语言包 | 7 份 × **154 key**，递归比对 **缺 0 / 多 0** | tomllib |
| 测试 | 45 个探针脚本 + 12 份契约 golden + `tests/all.sh` | 实物清点 |
| **回归（本轮真跑）** | **42 通过 / 0 失败 / 5 跳过 / 221 秒**（`cargo build` + `clippy -D warnings` + `cargo test` + 34 个协议/探针 + `flutter test`） | `bash tests/all.sh` 全绿 |
| 发布物 | `dist/` 14 项 / 72 MB | `ls dist` |

---

## 2. 界面审计（本轮重点）

### 2.1 UI-SPEC 的 14 个状态 × 实现程度

> 权威清单来自 `docs/UI-SPEC.md` §二「状态全景（14 个状态 · 每个都要有设计）」。

| 状态 | UI-SPEC 要求 | 界面实现 | 判定 |
|---|---|---|---|
| S1 空状态 | 图标 + "还没有文件" + 拖入引导 | `Phase.empty` + 三步引导 | ✅ |
| S2 识别中（小文件） | 旋转 + 文件名 + 取消 | `home_page.dart:240` 吃 `STAGE_STARTED` | ✅ |
| S2b 识别中（大文件进度） | 进度条 + "还需约 1 分钟" | `home_page.dart:242` 吃 `SCAN_PROGRESS` | ✅ |
| S3 列出内容 + 勾选 | 条目勾选 + [全选][只选必需][开始解压] | `_picked` → `select`（`home_page.dart:661,912`）+ 全选（`:1641`）+ 开始解压（`:1225`） | ✅ |
| S4 需要密码 | 输入框 + 两个"自动找"勾选 + 错误给建议 | 输入框 ✅ 密码本 chips ✅ 按码文案 ✅；**两个勾选已接**（T8：设置页拆成「笔记 / 说明书」两档，写回 `[password]` 段，真机 + 端到端验过） | ✅ |
| **S5 空间不足** | **需要 / 可用 / 还差 三个数 + [换个位置][只解压部分][继续试试]** | **已接（T4）**：真机 3 GB 包撞 2 GB 盘 → 「需要 3.00 GB / 目标盘只有 1.94 GB / 还差 1.06 GB」+ 三个按钮（「继续试试」＝`--force`，T6） | ✅ |
| S6 处理中 | 进度 + 当前文件 + [取消][转到后台] | `EXTRACT_PROGRESS` + 取消 + 设置页"缩到托盘" | ✅ |
| S7 完成（成功） | 文件数/大小/校验/主程序 + 三按钮 | `home_page.dart:672` 完成卡 + 打开目录 + 复制解压方法 | ✅ |
| S8 部分成功（救援） | "救出了大部分" + 百分比 + 明细 + 建议 | 救援按钮 + `PARTIAL_DATA` 文案 + 失败卡清单 | ✅ |
| S9 失败（按原因分支） | 每种原因一套文案 + 下一步 | `humanize.dart:17 errorText()` 按 30+ 码分支 | ✅ |
| **S10 多候选** | **列出 N 个候选让用户选（带可信度）** | **已接（T5）**：预览页提示条「这个文件里像是有 N 处藏着东西…」+「换一处解压」，≥2 处才显示；协议 `extract.candidate` 按偏移精确切 | ✅ |
| S11 已取消 | 说明“已解出来的保留着” | `humanize.dart:37 CANCELLED` | ✅ |
| S12 后台运行中 | 手机端常驻通知 | 桌面用托盘（合理替代）；手机端本身未开始 | — 不适用 |
| S13 批量队列 | 队列 + 移出 + 清空 + 保留已完成 | 队列 UI 完整（`home_page.dart` 队列段） | ✅ |
| S14 已完成列表 | 主界面常态列表 | 历史页（可点开目录） | ✅ |

**统计：✅ 13 / 🟡 0 / ❌ 0 / 不适用 1**（2026-10-31 复核：S4 / S5 / S10 本轮补齐；原始判定是 ✅11 / 🟡1 / ❌2，保留在本表各行「界面实现」列里）
> `GUI-PLAN.md` 把 S2b / S13 / S14 标成 ❌，实际全部已实现 → 见 §4。

### 2.2 引擎/协议有、界面没入口（8 项 · **2026-10-31 复核：7 项已接、1 项拆解**）

> 这张表是原始审计（2026-10 版）。**界面那一列已按现状逐条更新** —— 依据是 T7/T8/T14 的落地提交，
> 不是照文档改文档。别再把它当「八个缺口」引用；剩的活只有 T17-2 并行数与 T18-7 `--keep`。

| # | 能力 | 引擎 & 协议 | 界面 | 用户影响 |
|---|---|---|---|---|
| E1 | 干扰数据 + 假签名（防网盘扫描） | `--decoy` / `--decoy-size` / `--decoy-signatures`；协议 `pack.decoy*`；`src/decoy.rs` 83 处 | ✅ **已接**（T7-E1 `6df4be1`：打包页总开关；普通压缩 / 自解压下禁用并写明原因） | — |
| E2 | 打包布局（free 原子 / 尾部追加 / 头部 / 视频中间 / 两个包 / MKV 附件） | `--tail` / `--layout L1\|L2\|L3\|L4\|L7\|L8`、`--second`；协议 `pack.tail` / `pack.layout` / `pack.second` | ✅ **六档全接**（T7-E2 `38bb58a` 打底；T18-1 补完 L3 / L4 / L7 / L8，0.9.43 装机验收）：界面下拉六档、CLI 六档、协议 `tail/free/head/mid/two/mkv`；每种的高级代价（unzip 找不到 / 只见一个包 / 需 ffmpeg）都写进界面说明 | — |
| E3 | 强制跳过空间预检 | `--force`；协议 `extract.force` | ✅ **已接**（T4/T6：报错卡片上的「继续试试」传 `force`） | — |
| E4 | 只测不写（查包完整性） | `--test` | ✅ **已接**（T7-E4 `a63f2c9`：预览页底部「只检查」，目录零新增） | — |
| E5 | 智能解压开关（永远建同名文件夹） | `--no-smart-dir`；协议 `batch.smart_dir` | ✅ **已接**（T7-E5 `a46ced6`：协议补 `extract.smart_dir` + 高级选项开关） | — |
| E6 | 诊断包指定输出目录 | 协议 `report.out_dir` | ✅ **已接**（T7-E6 `38bb58a`；顺带修 `findings.error` 没脱敏 `023017c`） | — |
| E7 | 并行数 / 关掉自动抓密码 / 保留中间 ZIP | `--jobs` / `--no-auto-password` / `--keep` | **已拆解**：自动抓密码两档 → ✅ T8（两个勾选）· 并行数 → ⬜ T17-2（三档下拉，待做）· `--keep` → ⬜ T18-7（极客项，排批 4 尾） | — |
| E8 | 界面多语言 | 7 语言 × 154 key 全齐；`--lang`；协议 `hello.languages` | ✅ **已接**（T14：界面 7 语言 487 条，0.9.39 发版装机） | — |

### 2.3 界面"读了却没用"的字段（最隐蔽的一类）

| 字段 | 引擎给了 | 界面 | 后果 |
|---|---|---|---|
| `SPACE_SHORTAGE.available` | 有（`src/event.rs:263`，字节数） | 只读了 `needed`，**`available` 从未使用** | "还差多少"永远算不出 → S5 半残，用户不知道要清多少 |
| `scan.looks_like`（多候选） | 有 | 只在队列里显示类型名（`home_page.dart:973`），**没有候选选择界面** | S10 做不了 |
| `default_mode` 配置 | 解析 + 保存模板 | 引擎侧**零读取**（见 §5.1） | 假开关 |

### 2.4 界面多语言：分层实测

| 层 | 状态 | 证据 |
|---|---|---|
| 引擎语言包 | ✅ 7 份 × 154 key，**缺 0 / 多 0** | `locales/*.toml` + tomllib 比对 |
| 协议 | ✅ `hello.languages` 真返回 7 种 | 实跑 `--serve` |
| 命令行 | ✅ `--lang en/ja/ko/ru/es` 有效 | `src/i18n.rs` |
| **界面** | ✅ **已接（T14）**：`l10n.dart` + `assets/l10n/*.json` 7 份 × 487 条；`availableLocales` 列全 7 种（没翻完的语言不进下拉） | 0.9.39 装机版切 7 种语言实测 |
| 界面文案规模 | **487 条 × 7 语言**（原 474 条中英；T14 把 8 处源码里分两段拼的句子合并成整句 + 占位符） | 语言包全量比对 |
| 启动参数 | ✅ 界面语言默认跟随系统、可手动切换并写回 `[general] ui_language` | 读代码 + 真机 |
| 自我说明 | ✅ 设置页那段「401 处硬编码中文」的过期说明已删（T14 收尾），页面不再谈实现细节 | 读代码 |

**定性建议（2026-10-31 已执行完）**：原建议「改成 🟡 已知缺口」已不需要 —— T14 把界面接到 7 种语言，
`README` 与关于页如实写。原估工（474 条抽 key + 6 语种翻译 + 接线）实际是两天级，翻译花了约 0.16 元。

工作量：474 条抽 key（~1 天）+ 6 语种 2844 条翻译 + 接线与语言选择 UI（~1 天）+ 回归。

---

## 3. 其它三层（结论：干净）

- **命令行 42 个选项**：逐个核到读取点，**没有任何"只写在帮助里"的**。容易被误判成空的 10 个
  （`--delete-source` / `--trash-source` / `--no-color` / `--no-checksum` / `--no-readme` /
  `--no-smart-dir` / `--password-file` / `--import-passwords` / `--sfx-linux` / `--no-auto-password`）
  全部有真实读取点（`main.rs:781,783`、`:376`、`:692`、`:693`、`:775`、`:492`、`:697`、`:361` 等）。
- **协议 11 命令**：全部被界面调用过（`batch` 例外 —— 界面自己排队，**有意的**）。
- **格式能力**：`--caps` 12 行与 `hello.formats` 12 条一致（同源 `unpack::能力行()`，`tests/caps_protocol.py` 盯着）。

---

## 4. 台账失真：标"未做"，其实早做完（14 条，**必须先改台账**）

| # | 台账原文 | 实际证据 |
|---|---|---|
| D1 | `REFERENCE-INDEX §3` 干扰数据/假签名 **❌ 第22批** | `src/decoy.rs` + `api.rs:503` + CLI `--decoy*` + `tests/decoy.sh` |
| D2 | `REFERENCE-INDEX §8` 解完删源 `--delete-source`/`--trash-source` **❌ 未做** | `main.rs:781,783`（含二次确认 `:1229`）+ 界面设置页「原包处置」 |
| D3 | `REFERENCE-INDEX §2` 预览树 + 部分解压 P7 **❌ 第22批** | `gui/lib/entry_tree.dart`（33 处）+ `select` 已接（`home_page.dart:661`） |
| D4 | `REFERENCE-INDEX §1` 条目 packSize 校验 **❌ 第22批** | `src/rescue.rs` 按 `packSize` 逐条校验（29 处） |
| D5 | `REFERENCE-INDEX §1` 7z 头加密 **🟡 第20批** | `HEAD_ENCRYPTED` / `encrypted_names` 18 处 + 密码框入口 |
| D6 | `REFERENCE-INDEX §2` 密码卡三件套 **🟡 第20批** | 眼睛图标（`pack_page.dart:49`）+ 密码本 chips（`settings_page.dart:381`） |
| D7 | `REFERENCE-INDEX §2` 设置页可写 **🟡** | `settings_page.dart` 17 键完整保存流程 |
| D8 | `REFERENCE-INDEX §8` 进度条缺"已用时" **🟡** | `src/event.rs:321 elapsed_ms`（38 处） |
| D9 | `REFERENCE-INDEX §8` 分段虚接 E3 **🟡** | `src/extract7z.rs` 文件头即写"含分卷虚接"（11 处） |
| D10 | `REFERENCE-INDEX §9.3` CI workflows **❌ 未做** | `.github/workflows/ci.yml` 已存在（只 Rust 侧 → 真实 🟡，见 §5.2） |
| D11 | `GUI-PLAN` S2b 识别进度 **❌ P1** | `home_page.dart:242` 吃 `SCAN_PROGRESS` |
| D12 | `GUI-PLAN` S13 批量队列 / S14 已完成列表 **❌ P1** | 队列 UI 完整；历史页可点开目录 |
| D13 | `DESIGN.md:561-569` 界面选项表：干扰数据 ❌ / 假签名 ❌ / 分卷"CLI 的 `--split` 也没实现" | 三项**全部已实现**（`cli_spec.rs:77`、`pack_post.rs:83`、界面也传 `split`） |
| D14 | `docs/TODO.md` 顶部完成度表（2026-10-29） | 比 `HANDOFF.md`（10-30 深夜）旧：「Windows 时区还差两步」已不成立（W1~W10 已清账） |
| D15 | `REFERENCE-INDEX §2` / `TODO.md`：主题+**窗口尺寸记忆未做**（"要引 window_manager / Wayland 做不到"） | **已实现，且已进装机版**：`gui/linux/runner/my_application.cc`（GTK C++ runner，不是 Dart，所以搜 `gui/lib` 搜不到）；装机二进制 `strings` 命中 `gui-window.txt`；真机痕迹 `~/.local/state/yshjy/gui-window.txt` = `850 688 0`（2026-10-30 21:07 生成）；`tests/gui_window_state.sh` 三组断言。**Wayland 做不到的是"位置"，尺寸是记不住的** |

---

## 5. 平台 / 工程设施 / 死配置

### 5.1 死配置：`default_mode`
`src/config.rs:100,113,249,330,345` 只有「字段 / 默认值 / 解析 / 保存模板」四处，**全库无任何读取点**。
`docs/BACKEND-SPEC.md:75` 承诺 `default_mode = "auto" | "extract" | "pack"`。
→ 用户改了它行为不变。典型假开关。
> 另两个"只有一个读取点"的配置项核实是**真开关**：`output.dir`（`main.rs` 1 处够）、
> `safety.flush_movable`（`flush.rs` 1 处，有 `tests/flush_movable.sh` 4/4 撑着）。

### 5.2 平台与 CI
| 项 | 状态 | 证据 |
|---|---|---|
| 移动端 FFI 绑定 | ❌ 未做 | `ffi/` C ABI 已就绪；`gui/lib` grep `ffigen`/`yshjy_bindings` 零命中 |
| CI | 🟡 只有 Rust 侧 | `.github/workflows/ci.yml` 47 行：fmt/clippy/build/test/cargo-deny；**无 Flutter 测试、无出包流水线** |
| Windows | 🟡 引擎交叉编过，界面必须目标机编 | `HANDOFF.md` W1~W10 已清账 |
| Android / macOS / iOS | ⛔ 未开始 | `TODO.md` |

### 5.3 已决定不做 / 物理做不到（不算欠账）
- **窗口位置**记忆：Wayland 不许客户端给自己定位（**注意：尺寸记忆是做了的**，见 D15 — 别再把它当"未做"）。
- AppImage、7z 写包、自动更新、云盘同步、内置文件管理器 —— `TODO.md`「不做」一节已写死。
- S12 手机端后台常驻通知：桌面用托盘替代，手机端整体未开始。

### 5.4 ⚠️ 假绿风险：5 个默认跳过的测试

`tests/all.sh` 报 42 通过 / 5 跳过。**跳过的那几项恰好是"最没被验过"的功能**：

| 跳过项 | 为什么跳 | 风险 |
|---|---|---|
| 窗口尺寸记忆（`gui_window_state.sh`） | 会真开窗口、桌面闪几下（用户明确讨厌回归弹窗） | 功能做了、探针写了，但**回归从来不验**它；哪天 runner 改坏了没人知道 |
| 300 MiB 大 RAR | 慢（要 `--slow`） | 大文件 / 大压缩包的落盘路径平时不验 |
| RAR 分卷识别 | 慢 | 分卷（网盘主流形态）平时不验 |
| fuzz 写盘路径 | 10 分钟 | 写盘路径的模糊测试平时不跑 |
| `cargo fmt --check` | 手写风格，跑了会红一片 | 有意降级为提示，可接受 |

**结论**：这四个"慢/会弹窗"的项，就是"AI 说做完但其实没人盯着"最容易发生的地方。
窗口尺寸记忆这次就是活例子 —— 我按 `gui/lib` 搜关键字判它"未做"，真相是它写在 GTK runner 里，
而且**真机状态文件今天还在更新**。以后核功能：**Dart + C++ runner 都要搜，再看真机痕迹文件**。

---

## 6. 行业标准对照：这个项目"完不完整"（2026-10-30 补查）

> 由来：用户问「项目审核和项目完整方面网上有没有方法」。下面三套是业界真正在用的标准，
> 我用它们对你的项目实测了一遍。**判定只算仓库自己的文件**（`refs/` 里下载的参考项目一律不计，
> 上一版就因此误判过——`refs/` 里有 RustDesk 的 metainfo，差点算成"我们有"）。

### 6.1 三套标准，各治一种病

| 标准 | 治什么 | 来源 |
|---|---|---|
| **ISO/IEC 25010**（产品质量模型，8 个特性） | 给"软件质量"一个官方维度表：功能适合性 / 性能效率 / 兼容性 / 交互能力 / 可靠性 / 安全性 / 可维护性 / 可移植性 | iso25000.com |
| **Flathub 质量与元数据指南** | Linux 桌面应用"能上架、能在软件中心好看"的硬要求：metainfo.xml、截图、图标（SVG 或 ≥256 方形 PNG）、每个版本要写 release notes | docs.flathub.org |
| **开源项目成熟度模型**（Apache Project Maturity Model / OpenSSF Scorecard / CHAOSS） | 项目健康度：文档齐全度、CI、许可证、安全策略、社区治理 | community.apache.org、scorecard.dev |
| 补充：**GNOME HIG**（无障碍、键盘） | 界面"像个正经应用"的细节：无障碍名称、焦点顺序、快捷键 | developer.gnome.org/hig |

### 6.2 ISO 25010 八维 × 本项目实测

| 维度 | 判定 | 依据 |
|---|---|---|
| 功能适合性 | 🟡 | 引擎功能齐全；**界面缺 8 项入口**（§2.2）、S5/S10 未完成 |
| 性能效率 | ❓ 未测 | 无 benchmark（`benches/` 不存在）；大包/碎文件/慢盘没压过 |
| 兼容性 | ✅ 强 | 12 种格式 + 14 种 MIME 认领 + 与第三方工具逐字节比对；平台 1/5（见可移植性） |
| 交互能力（易用性） | 🟡 | 14 状态 11✅；但**无障碍零实现、界面无多语言、无截图素材** |
| 可靠性 | ✅ **最强项** | 原子写入 + 断点续传 + 救援 + 崩溃日志 + 诊断包 + 落盘校验 |
| 安全性 | 🟡 | `deny.toml` 许可证审计在 CI 里；**无 SECURITY.md、无 cargo-audit、无 CodeQL、写盘 fuzz 默认跳过** |
| 可维护性 | 🟡 | 45 探针 + clippy + 契约基准很扎实；但**无覆盖率报告**、**台账 15 条失真**（文档与实现脱节本身就是可维护性扣分） |
| 可移植性 | 🟡 | Windows 命令行半成品（缺分发包）、Android/macOS/iOS 未开始；metainfo + 7 尺寸图标已接（T9 只差真截图） |

### 6.3 具体缺项（实测，按"用户/上架会撞上"排序）

**① 门面 / 分发（最扎眼，成本最低）**
| 项 | 现状 | 为什么重要 |
|---|---|---|
| **AppStream `metainfo.xml`** | ✅ **已接**（`data/io.github.yangsanhai.yshjy.metainfo.xml`，`appstreamcli validate` ✔ 通过；装机在 `/usr/share/metainfo/`） | GNOME 软件中心 / Flathub / KDE Discover 靠它显示名称、简介、截图、版本说明。没有它，装了也像"野生程序" |
| **截图** | 🟡 **一半**：`screenshots/` 已有 **18 张真机截图**；但 README 里仍是徽章、metainfo 没填 `screenshots` → 差最后一步（T9） | 别人第一眼判断"这软件能不能用"就靠截图；上架也必须有 |
| **图标多尺寸 / SVG** | 🟡 只有 `gui/linux/yshjy.png` 一张 **256×256**，装到系统也只有 `hicolor/256x256/` 一个尺寸 | Flathub 要求 SVG 或 ≥256 方形 PNG（勉强达标）；但 16/24/32/48 缩放会糊 |
| README 徽章版本 | ✅ 写 **v0.9.41**，与装机 `dpkg -l` 一致（2026-10-31 核） | 对外数字必须真 |

**② 开源项目规范（GitHub 社区标准）**
| 项 | 现状 |
|---|---|
| SECURITY.md（漏洞怎么报） | ✅ 有 |
| CODE_OF_CONDUCT.md | ✅ 有 |
| issue / PR 模板 | ✅ 有（`bug_report.yml` / `feature_request.yml` / `config.yml`，GitHub YAML 表单格式） |
| README / LICENSE / CHANGELOG / CONTRIBUTING | ✅ 全有（这块做得比多数个人项目好） |

**③ 界面成熟度（GNOME HIG）**
| 项 | 现状 |
|---|---|
| 无障碍语义 `Semantics(` | ❌ **0 处**（读屏软件读不出按钮是干什么的） |
| 焦点顺序 `FocusTraversal` | ❌ 无 |
| 键盘快捷键 | 🟡 只有 2 处 `LogicalKeyboardKey`（偏少） |
| 深浅色主题 | ✅ 有 |
| 界面多语言 | ✅ 7 种语言 / 487 条（详见 §2.4） |

**④ 工程质量**
| 项 | 现状 |
|---|---|
| 覆盖率报告 | ❌ 无（`cargo llvm-cov` 没接） |
| 性能基准 | ❌ 无 `benches/` |
| 端到端界面测试 | ❌ 无 `integration_test/`（现在的 `flutter test` 是 widget 级，不开真窗口） |
| 默认跳过的测试 | ⚠️ 4 个（§5.4，其中窗口尺寸记忆就是"写了没人验"的活例子） |
| 依赖漏洞扫描 | ❌ `cargo audit` 没接；CI 里只有 licenses 检查 |

**⑤ 文档**
| 项 | 现状 |
|---|---|
| 用户手册 | ❌ 无（只有 man 页 + `USER-FEEDBACK.md` 反馈记录） |
| 关于页 / README 是否如实写"界面只有中文" | 🟡 设置页写了；README 没写 |

### 6.4 以后怎么自己查（可脚本化，不靠我肉眼）

```bash
# 1) 门面三件套：有没有 metainfo / 截图 / 多尺寸图标（排除参考项目）
find . -path ./refs -prune -o -path ./target -prune -o \
     \( -name '*.metainfo.xml' -o -name '*.appdata.xml' \) -print
ls screenshots/ docs/screenshots/ 2>/dev/null
find . -path ./refs -prune -o -path '*/hicolor/*' -print | head

# 2) 开源规范四件套
ls SECURITY.md CODE_OF_CONDUCT.md .github/ISSUE_TEMPLATE/ 2>/dev/null

# 3) 无障碍 / 快捷键 / 多语言（都应为非 0）
grep -c "Semantics(" -r gui/lib/ ; grep -c "LogicalKeyboardKey" -r gui/lib/

# 4) 覆盖率（装上 cargo-llvm-cov 之后）
cargo llvm-cov --text --summary-only

# 5) 测试是不是假的（变异测试，需装 cargo-mutants）
cargo mutants --file src/extract.rs
```

> 对外说法建议：**现在能当"发布级 Linux 工具"，但还不是"完整应用"** ——
> 引擎和可靠性是强项，缺的是**门面（metainfo/截图/图标）、无障碍、多语言、覆盖率与变异测试**这几块。

---

## 7. 专项：「越更新越丢东西」查证

**做法**：① `git log --diff-filter=D` 找被删源文件；② 抽 `gui/lib` 54 个提交里出现过的全部中文界面文案
（351 条）与当前存量（300 条）求差 → 51 条，逐条核。

**结果**：
- 被删的 `.dart` / `.rs` 源文件：**0 个**（只删过 `target-full/` 构建产物）。
- 51 条差异里 48 条是注释噪声；3 条疑似全部是**文案搬家**：
  | 疑似丢失 | 核实 |
  |---|---|
  | 分卷已自动拼起来 / 分卷不齐 | 在 `humanize.dart:19,52` |
  | 外壳类型 视频/图片/文本 | 升级为"任意类型"（`pack_page.dart:475`） |
  | 打包页「解压码」输入 | 在 `pack_page.dart:506` |

**结论**：界面在"有→无"方向干净。真实体感来自反方向 —— **引擎越更新越强，界面没跟上**（§2.2 八项是铁证）。
准确说法是「**越更新，界面和引擎的差距越大**」。

---

## 8. 本轮没核到的面（诚实边界）

| 面 | 状态 | 打算怎么核 |
|---|---|---|
| 每种格式造样本真跑解压/打包 | **未做** | 只核了 `--caps` 与代码路径 |
| 界面 5 页逐控件真机点一遍 | **未做** | 需 computer-use 按 `docs/AI-验收方案.md` A1~A7 记录 |
| 假开关端到端（临时配置→跑→diff 产物） | **只做静态判定** | 其余 16 个设置项只数了读取点 |
| Windows / Android / macOS | **未核** | 本机做不了 |
| 性能（大包/碎文件/慢盘）· 安全（cargo audit/deny） | **未核** | 需造样本与联网拉漏洞库 |

---

## 9. 复现命令

```bash
# 版本与能力
dpkg -l yshjy | tail -1 ; yshjy --version ; yshjy --caps
yshjy --help | grep -oE -- '--[a-z][a-z0-9-]+' | sort -u | wc -l        # 42

# 协议真身
printf '{"id":1,"cmd":"hello"}\n' | yshjy --serve | head -1

# 代码量
find src -name '*.rs' | wc -l ; find src -name '*.rs' -exec cat {} + | wc -l
find gui/lib -name '*.dart' | wc -l ; find gui/lib -name '*.dart' -exec cat {} + | wc -l

# 界面没接的协议字段
grep -n "'cmd': 'pack'" -A 12 gui/lib/engine.dart     # 缺 decoy/decoy_size/decoy_signatures/tail
grep -rn "force" gui/lib/ || echo "界面零引用"          # --force 无入口

# 界面多语言（应全为 0）
for k in l10n AppLocalizations Intl.message supportedLocales flutter_localizations; do
  echo -n "$k: "; grep -rl "$k" gui/lib/ | wc -l ; done

# 空间不足只用了 needed，available 没用
grep -n "SPACE_SHORTAGE" -A 4 gui/lib/pages/home_page.dart
grep -n "SpaceShortage" -A 6 src/event.rs

# 死配置
grep -rn "default_mode" src/ docs/BACKEND-SPEC.md     # 只有解析+模板，无读取点
```
