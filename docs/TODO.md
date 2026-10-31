# 待办清单（**唯一真源**）

> 项目里所有「还没做的事」只记在这一个文件，别处（HANDOFF / 计划文档）不再记待办，
> 只记历史与证据。**做完一项就在这划掉一行**，证据写进 `CHANGELOG.md`。
>
> 排序依据（2026-10-31 夜改口径）：**「从内到外」= 引擎核心 → 协议/协议接线/界面 → 门面**。
> 老原则「真人会撞上的概率优先」仍然成立，另外加三条排法规则（定矩阵先做 / 同页攒批 /
> 截图最后因为它会过期），三条都在下面「下一轮施工顺序」节里。
> 每次开工只做**当前批**（表里第一个还没做的批），出现的新问题不插队，加到表尾。

最后更新：2026-10-31 晚（**T14 界面多语言做完，0.9.39 已发版并装机验收**：
479 条文案 × 七种语言全部到位，界面里已扫不到硬编码中文；设置页那段过期的
「还没做的（我不装样子）」已删。
**下一项：批 2 T18-3 防炸链 + T18-2 自定义显示名**）
> 上一批：2026-10-31 午（**T7 全部做完（5 / 5）**：E4 只检查 `a63f2c9`、E1 防网盘扫描 `6df4be1`、
E5 智能解压 `a46ced6`、E2 不改封面 + E6 诊断包存下载 `38bb58a`；顺带修掉两个真 bug：
打包页溢出、诊断包 `error` 字段没脱敏 `023017c`。**已发版并装机 0.9.38**）
> 上一批：2026-10-31 凌晨（**T16 完成，0.9.37 已出包并装机** —— 命令行 `--candidate`）
> 上一批：2026-10-30（0.9.35 已出包 —— 清 `docs/AUDIT-GAP.md` 对账欠账，4 批 9 项）
> 表里还没勾的：**T9**（差真截图）、**T11**（无障碍）、**T14**（界面多语言，两天级）、
> **T15**（覆盖率报告）。

## 🔨 本轮施工进度（2026-10-30 夜 · 来源 `docs/AUDIT-GAP.md`）

> 一行一项，**做一项、验一项、勾一项、提交一次**。上面这份表是「本轮要做完什么」，
> 唯一的进度清单就是这张表（别处不再记）。顺序按层：引擎 → 协议 → 界面 → 门面 → 文档。

| # | 项 | 层 | 状态 | 验收标准 |
|---|---|---|---|---|
| T1 | 解压/`--pick` 结束前真 `fsync`（报 ✅ 时数据真在盘上） | 引擎 | ✅ **0.9.34 已实现，本轮复核** | `tests/flush_movable.sh` 本机 2/2 过（移动设备两项要插 U 盘，未测）——**不是本轮新做，别记账成本轮功劳** |
| T2 | 空间不足：三个数 + 事件真发 + 去掉「0 B」 | 引擎 | ✅ 完成 | 真机实测：3 GB 包 → `/run/user/1000`(2 GB) 拦下，输出「需要 3.00 GB / 只有 1.94 GB / 还差 1.06 GB」；`--json` 有 `SpaceShortage` 事件；`--lang en` 英文正常；正常小包不受影响 |
| T3 | `default_mode` 死配置 | 引擎 | ✅ 完成（**按「删」处理**） | 真机制是程序名（yshjy/yshys），配置项重复且零读取 → 5 处全删；`--show-config` 无该项；BACKEND-SPEC 改口径 |
| T4 | S5 界面：三个数 + [换个位置][只解压部分][继续试试] | 界面 | ✅ 完成（真机截图验过） | 造 3 GB 包撞 2 GB 盘：界面显示「需要 3.00 GB / 目标盘只有 1.94 GB / 还差 1.06 GB」+ 三个按钮都在。真机撞出并修掉两处：协议层把 detail 吞了、错误页误报「认不出的文件」 |
| T5 | S10 多候选：列出候选让用户选 | 界面 | ✅ **完成（真机验过）** | 引擎 `scan` / `list` 都带 `candidates[]`（一个文件里几处藏着归档，数据来自定位时**本来就要跑**的全文件扫描，不额外扫）；协议 `extract.candidate` 按偏移精确切那一段（`layout::zip::locate_at_base`，找不到那段就报 `NOT_ARCHIVE`，绝不回退解默认那处）。界面：**预览页**顶部提示条「这个文件里像是有 N 处藏着东西…」+「换一处解压」，队列行也有同样入口；**只有 ≥2 处才显示**（正常包一个字都不多说）。**真机证据**：样本 `[3MB垃圾][ZIP-A][3MB垃圾][ZIP-B][20MB填充]`（尾巴超 16MB 才会退化成全文件扫描）→ 提示条真出现（`screenshots/t5-预览页.png`），点「换一处解压」后产物**从「第B处解出来的.txt」变成「第A处解出来的.txt」**（`~/下载` 实测）。**探针 8/8**（含「非法 candidate 必须被拒」）、**cargo test 404 过 0 挂**、**flutter test 109 过**、clippy / flutter analyze 干净 |
| T6 | `--force` 界面入口（与 T4 同根） | 界面 | ✅ 完成 | 就是 T4 的「继续试试」：`engine.dart` 的 extract 加了 force → 协议 `extract.force`。真机点过（报错卡片上那个按钮） |
| T7 | 引擎有、界面没入口的其余项（8 项） | 协议 + 界面 | ✅ **完成（5 / 5，真机全点过）** | E4 ✅ `a63f2c9`、E1 ✅ `6df4be1`、E5 ✅ `a46ced6`、E2 + E6 ✅ `38bb58a`；E7 三项判定「不做」，理由见下面「T7 明细」。真机还撞出两个真 bug：打包页溢出（E1 顺带修）、诊断包 `error` 没脱敏（`023017c`） |
| T8 | S4 密码框两个「自动找」勾选 | 界面 | ✅ 完成（真机点过 + 端到端验过） | 引擎拆成两档（配置 `auto_from_book` 新 + `auto_from_notes` 原有、开关 `--no-auto-password-notes` / `--no-auto-password-book`），界面设置页接成两个真勾选，保存写回 `[password]` 段。**不需要**新协议命令——设置页本来就能逐行读写 `config.toml`（`flush_movable` 一直这么干），HANDOFF 之前那句「得先加读/写配置的协议命令」是保守误判，已纠正。**真机证据**：`screenshots/settings-auto-password-off.jpg`（两个开关关）+ `settings-save-button.jpg`（保存按钮）；保存后 `config.toml` 里是 `auto_from_notes = "off"`；**端到端**：同一个加密包 + 旁边放 `说明.txt`，开关 off 时输出「这个包带锁，需要解压码」（不翻说明文件），改 on 后输出「解压码是从旁边的说明里找到的：./说明.txt: 解压码：notePW」并解开 |
| T9 | 门面：`metainfo.xml` + 多尺寸图标 + README 版本/如实说明 | 门面 | 🟡 **差真截图** | `appstreamcli validate` ✔ 通过；7 尺寸图标已接入 deb 与用户级安装；README 0.9.31→0.9.34、补「界面只有中文」。**剩**：真截图（README 那 3 张是徽章不是截图）+ 元数据里填 screenshots |
| T10 | 开源规范：SECURITY.md / CODE_OF_CONDUCT.md / issue 模板 | 门面 | ✅ 完成 | 三份都在；issue 表单是 GitHub YAML 格式（bug_report / feature_request / config） |
| T11 | 无障碍：`Semantics` 语义 + 键盘快捷键 | 界面 | ⬜ | `grep -c 'Semantics('` > 0 且 `flutter test` 全绿 |
| T12 | CI 加 Flutter 测试 | 工程 | ✅ 完成（**本地只能验 YAML，跑绿要到 GitHub 上才算真验**） | `.github/workflows/ci.yml` 新增 `gui` job：装 Flutter stable → `pub get` → `analyze` → `test`（`XDG_CONFIG_HOME` 指临时目录）。YAML 已用 `yaml.safe_load` 解析通过，jobs = check / licenses / gui。**没加** `--fatal-infos`：项目里那条既有 info 不在本轮范围，加上来只会一上来就红。出包流水线（deb + tag 发布）仍未做 |
| T13 | 台账失真 + README 假「规划中」 | 文档 | ✅ 完成（16 处） | REFERENCE-INDEX 9 处（含 decoy 真身其实在 `src/pack/decoy.rs`，审计自己写错了路径）、DESIGN 3 处、GUI-PLAN 4 处；每条都先 grep 核代码再改 |
| T14 | 界面多语言接线（474 条硬编码） | 界面 | ⬜ | 切 `--lang en` 界面变英文；**本轮不保证做完，见下方说明** |
| T15 | 覆盖率报告（cargo llvm-cov） | 工程 | ⬜ | `scripts/cov_by_file.py` 出分文件表 |
| T16 | `--candidate` 的 CLI 入口（T5 带出来的小账） | 工程 | ✅ **完成（真跑验过）** | T5 做完了，但「解第 N 处」以前只有协议 + 界面能表达，命令行没法指定 —— 用户拿命令行复核时办不到。现在加 `--candidate <字节偏移>`（与协议 `extract.candidate` 同名同义），非数字**报 USAGE(2) 不静默忽略**；落在垃圾里的偏移**报 NOT_ARCHIVE(3) 且零产物**（绝不偷偷解默认那处）；一次给两个文件**直接拒绝**（一个偏移不可能同时对上两个包，提示「它说的是这一个文件里的第几处」）；放进 `cli_spec.rs` 的 `OPTS` 表，帮助 / man 页 / 三份补全全部自动跟上（对外长选项 46、内部 3、共 49，`cli_spec_match.sh` 7/7）。**证据**：新探针 `tests/candidate_cli.py` **6/6**（老行为不变 / 换到另一段 / 非法偏移 / 非数字 / 多文件 / 值不被当文件名），已挂进 `tests/all.sh`；顺手把一直没挂的 `tests/t5_candidates.py`（8/8）也挂上。回归：`cargo test` 404 过 0 挂、`clippy` 干净、`flutter test` 109 过、`bash tests/all.sh` **46 通过 / 0 失败 / 5 跳过 / 210 秒** |
| T17 | 界面打磨与打包格式可选（5 项，详见下文「## T17」节） | 功能 | 🟡 **T17-4 完成（真机截图 3 张）**；T17-1 / T17-2 待做 | 打包格式 zip/tar.gz/tar.zst（**用户拍板第一件**）· 设置页并行数三档可调 · 格式能力直接展示 · 设备检测**已砍** |
| T18 | 「设计写了、现在没有」审计的 7 项缺口（详见下文「## T18」节） | 功能 | ⬜ 待做 | 打包布局 L3/L4/L7/L8 · 自定义显示名 · 改时间戳防炸链 · 队列全部暂停 · 历史页排序 · 关于页检查更新 · `--keep` 无入口 |

### T7 明细（8 项 · 一项一个提交 · 这是唯一记账处，别另建清单）

| 项 | 能力 | 层 | 状态 | 证据 / 说明 |
|---|---|---|---|---|
| E4 | 只检查不落盘（`verify_only`） | 界面 | ✅ `a63f2c9` | 引擎与协议**早就有**（CLI `--test` 走的就是它），只缺界面。真机：点完目录零新增（82259 字节不变）；测试 3 条 + 反向验证做过 |
| E1 | 干扰数据 + 假签名（`decoy` / `decoy_signatures`） | 界面 | ✅ `6df4be1` | 总开关撒 4 段×16K；普通压缩 / 自解压模式下禁用并写明原因；测试 6 条 + 反向验证；顺带修掉打包页 `BOTTOM OVERFLOWED BY 20 PIXELS` |
| E5 | 智能解压开关（`smart_dir`） | **协议 + 界面** | ✅ `a46ced6` | **缺口真在协议层**：`api.rs` 的 extract 把 `smart_dir` 写死 true（只有 batch 有）。现在 extract 可传（不传 = true，老行为不变）、PROTOCOL.md 补齐；界面在「高级选项」加开关，选了输出目录时**禁用并说明**。探针 `tests/smart_dir_proto.py` 3/3（已挂 all.sh）+ widget 测试 3 条 + 反向验证；真机 `screenshots/t7-e5-smart-dir-{on,off}.png` |
| E2 | 强制打包布局（`tail`） | 界面 | ✅ `38bb58a` | 协议 `pack.tail` 早就有（`TailAppend`），界面只**显示**结果、从不传。打包页加「不改封面，接在文件末尾（更保险）」：**默认关且请求里不出现 tail 键**（默认传 true 会把 MP4 伪装包都变成播不了的视频）；普通压缩/自解压禁用并说明。实测同一封面：自动模式 ZIP 起于第 2569 字节（封面 2319 后插了 250 字节 free 盒），尾部模式起于 2319（紧贴封面末尾）。真机 `screenshots/t7-e2-tail-{on,off}.png` |
| E6 | 诊断包指定输出目录（`report.out_dir`） | 界面 | ✅ `38bb58a` + `023017c` | 协议有，界面只传 `path` → 引擎落在**进程当前目录**，用户找不到。现在 `platform_paths.downloadsDir`（中文系统「下载」）兜底、一律传 `out_dir`、提示带「打开文件夹」；真机弹出文件管理器且文件在第一个（`t7-e6-report-{error-page,saved,opened}.png`）。**★ 真机撞出隐私 bug**：`findings.error` 是引擎原文，带着 `/home/用户名/…`，与「路径已脱敏」自相矛盾 → 已修 `023017c`（`error` 现在也换成同一个 `<path-2>`），3 条单元测试盯着 |
| E7-a | `--jobs` 并行数 | — | ⛔ **不做** | 引擎 `batch.jobs` 有，但界面**故意**固定 1：队列进度才有意义、机械盘不会被打成碎片。加开关等于造两套并行语义 |
| E7-b | `--keep` 保留中间 ZIP | — | ⏸ 极客项延后 | 只在打包路径有意义（CLI 打完告诉你中间文件在哪）；界面用户看不到中间文件，要做先定「留在哪、怎么清理」 |
| E7-c | `--no-auto-password` 总开关 | — | ✅ **已被覆盖** | T8 已把「自动抓解压码」拆成两个勾选（笔记 / 说明书），两个都关＝总开关关。再加一个只会造出两套优先级 |

### T14 明细（界面多语言 · 进行中）

| 项 | 状态 | 说明 |
|---|---|---|
| 地基：l10n 框架 | ✅ 框架 + about 页接上（真机验过：用户自己点「关于」，英文页正常） | 文案放 `assets/l10n/<code>.json`，与引擎语言包同名同规则（`段.键` + `{占位符}`）；不引 `flutter_localizations`/`intl`，理由见 `gui/lib/l10n.dart` 顶部。`gui/test/l10n_test.dart` 16 条（含「各语种缺/多/空/占位符」全量比对） |
| 错误码文案（humanize 45 条） | ✅ 完成 | 文案搬进语言包的 `err.*` / `diag.*` / `item.*` / `batch.*`；`errorText` / `diagText` / `itemResultText` / `batchOutcomeText` 改成查表，查不到仍回退「原因不明」那类通用人话（**绝不把码名甩给用户**，D21 铁律不动）。新增 `test/flutter_test_config.dart`：所有测试统一装基准语言表（否则表是空的，界面显示 key，一堆断言人话的测试会集体假红） |
| 设置页 68 条 + 界面语言下拉 | ✅ 完成 | 下拉选完**立刻生效**、写回 `[general] ui_language`；每种语言用**它自己语言里的名字**（不翻译）—— 切成看不懂的语言时至少还认得出哪个能切回中文。`gui/test/settings_locale_test.dart` 3 条（读回 en-US / 坏值回落成跟随系统 / 保存不冲掉注释与同段其它键） |
| 打包页 77 条 | ✅ 完成 | 三种模式 / 伪装说明 / 输出 / 解压码 / 级别 / 布局 / 防扫描 / 分卷 / 校验 / 说明.txt / 进度 / 三种结果卡片。带插值的 8 条改占位符（结果卡片原本用 `? :` 拼三段长文案，现在一种结果一条完整译文） |
| 外壳导航 + 历史页 24 条 | ✅ 完成 | 导航五项、状态栏、首次上手三步、托盘菜单、历史页 5 条。品牌名「杨三海解压」中英同值（不翻译） |
| 首页 264 条 | ✅ 完成 | 状态提示 / 错误与炸弹警告 / 高级选项 / 30 种格式名 / 结果页 / 拖放区 / 队列词 / 加密提示（ZipCrypto·AES）/ 内容树 / 封面 / 磁盘不够 / 分卷不全 / 诊断包 / 错误详情 / 解压码面板 / 三步进度 / 检出落盘 / 时长速度。其中 8 处源码里分两段拼的句子已合并成整句 + 占位符 |
| engine / 面包屑 / 设置页零散 10 条 | ✅ 完成 | 偏移描述（`从开头算 X MB 处`）、三档可信度、引擎退出·超时提示、面包屑「全部」、选目录按钮「就用这个目录」 |
| 有意不翻译的 4 处 | ✅ 定论 | `localeNames` 语言名（每种语言用自己的写法）、`platform_paths` 的 `$HOME/下载` 系统目录、`notify` 品牌名、`engine_transport` 给开发者看的 StateError |
| **合计 479 条，中英双份** | ✅ | 源码里已扫不到用户可见的中文字面量（用正则扫全部 `lib/**/*.dart` 的字符串字面量验证） |
| 6 个语种的翻译 | ✅ 完成 | zh-TW 用 `opencc -c s2twp` 转 + 术语按引擎 `locales/zh-TW.toml` 对齐（壓縮檔/解壓密碼/資料夾/網路/預設），品牌名写繁体「楊三海解壓」；ja/ko/es/ru 用 DeepSeek 分批翻（每批 50 条，带英文对照和语气要求），共 12 万 token。五种各 479 条，键名与占位符逐条对齐。**踩坑**：日语第一批有 49 条直接抄英文（高级选项、格式名），整批重翻；格式名改确定性映射（video→動画）比让模型自由发挥稳；DeepSeek 的 `json_object` 模式要求 prompt 里出现 JSON 字样，否则 400。**`availableLocales` 已列全 7 种** |
| **Windows 上的系统语言识别** | ⬜ **等 Windows 打包那轮做** | 现在用 `Platform.localeName`（dart:io）：Linux 读 LANG、macOS/iOS 读系统设置、**Windows 官方文档没写来源**（实际返回 `zh_CN` 这种下划线格式，代码已兼容 `_` / `-` / `.UTF-8` 三种写法）。**本机无法实测 Windows 路径**，等用户在自己的 Windows 机器上打包时：① 真机看默认语言对不对（中文系统应出简体、繁体系统应出繁体）；② 若有偏差，改成优先用 `PlatformDispatcher.instance.locale`（返回拆好的 language/script/country，跨平台统一）再把 `Platform.localeName` 当兜底；③ 顺带确认 Flutter 在 Windows 上 `computePlatformResolvedLocale` 仍未实现（issue 151755），所以映射表必须自己写，不能指望那个 API |

### 下一轮施工顺序（2026-10-31 夜重排 · 「从内到外」· 开工前跟用户对一次）

> **这一张取代旧表**（旧的 P1/P2/P3 按紧急度混排已作废，不留第二份）。
> 已清账留个记号：P1-1 崩溃日志脱敏测试 3 条 · P1-2 `tail_proto.py` + `report_outdir_proto.py` 已挂 `all.sh` ·
> P1-3 `downloadDirCandidates`（证据都在 `CHANGELOG.md`）；T14 多语言已随 0.9.39 发版，不在表里。

**排法三条规则（下回排新活也照这个判）**
1. **定矩阵的先做**：打包这块「格式 / 布局 / 加密 / 分卷 / 自解压」在引擎里是同一张参数矩阵，
   谁先定骨架，后面就是往里加格子；定晚了要返工两次 → 所以 T17-4 排最前。
2. **同一页的改动攒一批**：打包页、设置页各只动一次，一次真机点 + 截图验收。
3. **截图放最后，因为它会过期**：界面在 T17/T18 里还要变，先拍的图改完就废。

| 批 | 做什么 | 层 | 为什么排在这 | 验收 / 注意 |
|---|---|---|---|---|
| 0 | **文档对账（约 10 分钟）**：`AUDIT-GAP.md` 的 E1/E2/E4/E5/E6/E8 勾叉、`DESIGN.md` 14.2 里已判死的 L5、`CLI-SPEC.md` 的 `--help-full` 与三个错参数名 | 文档 | 照错文档干活必走偏，先清账再开工 | 改完 grep 旧说法 0 命中；**不动代码** |
| 1 | ✅ **T17-4 打包格式可选**（zip 默认 / tar.gz / tar.zst）—— 四层都做完，真机验过 | 引擎 + 协议 + 界面 | 用户拍板第一件；它同时定下「格式 × 选项」矩阵骨架 | 选 tar 时密码 / 分卷 / 自解压**置灰并写明原因**（不是藏起来）；伪装外壳先探针验（MP4 `free` 原子是按 ZIP 写的）；协议测试 3 条 + 两态真机截图 |
| 2 | **T18-3 防炸链**（随机化 mtime/哈希，默认关）+ **T18-2 自定义显示名** | 引擎（+ 协议 + 界面） | 两件都是引擎小活，半天级；防炸链接网盘的人真需要。★**T18-2 已核（2026-10-31）**：`PackOptions` 没有名字字段、`cli_spec.rs` grep `name` 零命中 → **引擎 + 协议 + 界面三层全缺**，不是「界面接线」 | 防炸链必须有「默认关」的测试；显示名要挡空值与非法字符 |
| 3 | **T18-1 打包布局**：L3 → L7 → L8 → L4 | 引擎 | 最重的一块，单独一轮；内部按难度递增 | L4 要改 `mdat` size + `moov` 的 `stco/co64`、插入点必须卡 chunk 边界 → **允许只做完前三样就收**，L4 再单独排 |
| 4 | **界面攒一批**：T17-2 并行数三档（默认仍 1）· T17-1 格式表直接显示 · P2-2 诊断包存放位置 · T18-4 队列全部暂停 · T18-5 历史排序 · T18-6 检查更新 · T18-7 `--keep` 入口 | 界面 | 全是界面小事，攒一起只做一轮真机点 + 截图 | 并行数旁常驻「机械盘 / U 盘选 1，固态可以 2 或 4」；格式表进页面就显示、去掉那个按钮 |
| 5 | **门面与护栏**：T15 覆盖率 → P2-1 版本号一条命令 → T11 无障碍 → T9 真截图 → P2-5 拆 `home_page.dart` | 工程 / 门面 | 覆盖率要等这一波改完才有意义；截图必须最后（会过期） | T9 挑 3–4 张真机图（预览页 / 打包页 / 高级选项 / 报错页）进 README + metainfo |

**两条不占顺序的轨道**
- **Windows**：7 项待办（含 `Platform.localeName`）都要在他自己的 Windows 机器上 → 攒够一波改动再一起验，本机只备代码。
- **已定欠账，不进本轮**：`--reclaim` 边解压边删源（用户口径「以后更新做」）· 真 I/O 错误要写 >3.9 G（太贵，等长时写入场景）· T17-3 设备检测（已砍）。

> 老规矩：开工只做当前批，出现新问题追加表尾不插队；做完一批 → `bash tests/all.sh` 全绿 →
> 证据写 `CHANGELOG.md` → 这里划掉 → 本地 git 提交。

**本轮范围说明**：T1~T13 是本轮目标；**T14（474 条文案 + 6 语种 2844 条翻译）与 T15
单独排**，做完 T13 再定是否连做 —— 用户口径「全搞完」，但这两项的体量是「两天级」，
硬塞进同一轮必然做一半，会在表里如实标「顺延到下一轮」，不装完成。

## 📊 完成度总览（2026-10-30 实测，不是估的）

> 这张表只在这里一份，别处（HANDOFF / ROADMAP / README）写完成度一律指回这里。
> 数字全部来自本节末尾列的实测命令；改完代码要跟着改这张表。

| 块 | 状态 | 证据 |
|---|---|---|
| 引擎·解压 | ✅ 完成 | 12 条能力组合（`--caps` 实测）；**53 个 `.rs` / 31,209 行**（顶层 41 个） |
| 引擎·打包与伪装 | ✅ 完成 | MP4 free 原子 + 其他外壳尾部追加 + 自解压 sfx + 分卷 + 干扰数据/假签名 |
| 引擎·可靠性 | ✅ 完成 | 原子写入（六条落盘路径）/ 断点续传 / 救援 / packSize 封顶 / 路径穿越 / 炸弹双闸；fuzz **只读入口 2.38 亿次 + 写盘路径 7.2 万次，均零崩溃** |
| 测试与护栏 | ✅ 完成 | `cargo test` **404 通过 / 0 失败 / 1 ignored**（2026-10-31 实跑）；`tests/all.sh` **46 通过 / 0 失败 / 5 跳过 / 210 秒**；**49 个探针脚本** + 12 份契约 golden |
| 命令行（CLI） | ✅ 完成 | **46 个对外长选项**（另有 3 个内部选项，共 49；实测 `--dump-cli-spec`）；补全三份 + man 页均由 `cli_spec.rs` 自动生成 |
| 图形界面（Linux） | 🟡 **页面齐、缺口在入口** | 5 页（首页/预览树/打包/历史/设置）+ 托盘；**18 个 dart / 6,661 行**；`flutter test` **109 条**。**缺口**：S5 空间不足、S10 多候选、8 项引擎能力没入口、无多语言/无障碍（见「🔨 本轮施工进度」T4~T8/T11/T14） |
| 装包与发版 | ✅ 完成 | deb + 两档 tar.gz；菜单认领 **14 种格式**；`test-packages.sh` 37 项；**0.9.34 已装机**；AppStream 元数据 + 7 尺寸图标已接进出包脚本 |
| 协议面 | ✅ 完成 | **11 个命令 / 30 种事件 / 14 种 MIME**；协议只加不删 |
| 多平台·Windows CLI | 🟡 **引擎全绿，缺分发包** | 真 Windows 机器上编过、跑过、解过包；W1~W10 十项审计全清（时区/续传/回收站/密码回显/内存闸/波浪号/单测/回归/界面）；`docs/BUILD-Windows.md` 已写。**剩**：Windows 分发包（zip/安装器）要在他自己的 Windows 机上出 |
| 多平台·Windows 界面 | ⛔ 本机做不了 | 必须在 Windows 上编（用户自己的 Win 机器） |
| 多平台·Android | ⛔ 未开始 | SDK/NDK 未装（约 8–10 G）；代码侧 seek 三档已就位，落地 5 条待验清单见下 |
| 多平台·macOS / iOS | ⛔ 未开始 | 靠 GitHub 免费 CI；零成本口径（不上架/不公证/不买账号）已定 |
| FFI（iOS 唯一通道） | 🟡 引擎侧已通 | `ffi/` 9 个 C 导出 + 头文件对账探针；**缺** Dart 侧 `FfiTransport` 与 ffigen 绑定 |
| `--reclaim` 边解压边删源 | ⏸ 延后 | 用户 2026-10-28 口径「以后更新做」，方案在 `docs/INPLACE-EXTRACT.md`，**不是否决** |

**一句话总评**：**Linux 单平台已经是发布级**（装机、真机 1200 条目 sha256 全一致、回归全绿）；
**全平台是「1 / 5 个平台完成」**（Linux ✅、Windows 半、Android / macOS / iOS 未开始），
多平台这条线上最便宜的下一个动作是补 Windows 时区。

### 🧾 Linux 还剩什么（**别把上面的 ✅ 读成「零遗留」**）

| # | 项 | 性质 | 现状 / 要做什么 |
|---|---|---|---|
| 1 | `--reclaim` 边解压边删源 | **功能未做（用户主动延后）** | 三档方案在 `docs/INPLACE-EXTRACT.md`（分卷读完删卷 / `fallocate` 打洞 / 倒着解）；用户 2026-10-28 口径「以后更新做」，**不是否决** |
| 2 | RAR 在「库因 CRC 失败提前 drop writer」时已写字节仍落地 | 已知小账（不阻塞发版） | `rars` 回调拿不到条目大小/CRC，彻底修要迁 `extract_with_control_and_errors` 重写回调层；现在靠库自身 CRC + 总量限额兜，见 `CHANGELOG.md` 第 12 项 |
| 3 | 真实磁盘满 / 中途拔盘 | 🟡 **已实测一半（2026-10-29）** | ① **磁盘满**：预检 3 次全命中（剩 110M 想解 300M → 直接拒、0 字节落地、退出码 1）；「写到一半才满」本机造不出（预检准 + 页缓存秒回，要造得写超过脏页上限 ≈3.9G）② **拔盘**：**屏幕报 ✅ 后立刻拔 → 300MB 全灭**（重插后 `fsck.exfat`: `clean. directories 1, files 0`，解压目录都不存在）；真正落盘要约 200 秒，与提示差 660 倍 —— 见 `CHANGELOG.md` 未发布节与表尾新条目 ③ **仍欠**：需要「写超过脏页上限」的长时写入，才能测真 I/O 错误下 `.yshjy-part` 的表现 |
| 4 | 「下载 → 装 → 出错」完整首用链路 | **未走完**（T6 ◐ 半勾） | 用户实机试过 0.9.25-c「都正常」，但没按新用户视角从零走一遍（下载页 → 安装 → 遇到错误 → 报障） |
| 5 | 界面只有中文 | 架构决定（非缺陷） | 引擎 7 语文案全齐；界面未接多语言，设置页已如实写「目前只有中文」 |

**物理上做不到 / 已决定不做 —— 不算欠账**：
- **窗口位置记忆** —— Wayland 协议不许客户端给自己定位（本机会话 `Type=wayland`；X11 才做得到，`window_manager` 接进来同样做不了）
- AppImage、7z 写包、自动更新 / 崩溃上报、云盘同步、内置文件管理器、GNOME Shell Introspect —— `## 不做` 一节已写死

> **口径**：Linux = 「**功能完整 + 4 条小账 + 1 个延后功能**」，不是「零遗留」。
> 有人问「Linux 做完了吗」，照这两行答，别只说「完成」。

<details><summary>这些数字是怎么来的（复现命令）</summary>

```bash
cargo test --quiet          # 404 通过 / 0 失败 / 1 ignored（371 + 31 + 2）
bash tests/all.sh           # 46 通过 / 0 失败 / 5 跳过（约 210 秒）
yshjy --caps                # 12 条格式能力组合
yshjy --dump-cli-spec | python3 -c 'import json,sys;d=json.load(sys.stdin)["options"];print(len(d), sum(1 for o in d if not o["internal"]))'   # 49 49·46 对外
find src -name '*.rs' | wc -l ; find src -name '*.rs' -exec cat {} + | wc -l   # 53 / 31209
find gui/lib -name '*.dart' | wc -l ; find gui/lib -name '*.dart' -exec cat {} + | wc -l  # 18 / 6661
ls tests/*.sh tests/*.py | wc -l ; ls tests/contract/golden | wc -l            # 49 / 12
```
</details>

**当前状态**：装机上是 **0.9.38**（2026-10-31 13:40 装机，`dpkg -l yshjy` 与 `yshjy --version` 都实测过）。
**装机后五项实测（2026-10-31 13:4x，本轮）**：① `dpkg` = 0.9.38 / `--version` = 0.9.38 ✔
② 菜单 `MimeType` 认领 **14 种格式** ✔ ③ `bash tests/pick.sh /usr/bin/yshjy` **11 通过 / 0 失败** ✔
④ 用装机版真打包再解压：3 个文件（中文名 + 三层嵌套 + 500KB 随机二进制）**sha256 逐一致** ✔
⑤ **本版新功能在装机版上单独验**：`python3 tests/smart_dir_proto.py /usr/bin/yshjy` **3/3** ✔，
装机版 GUI 上「只检查」按钮在、状态栏 `引擎 0.9.38`（`screenshots/v0.9.38-装机预览页.png`）
（装机包 10M；`test-packages.sh` 37 项全绿；sha256 见 `docs/RELEASE-CHECK.md` 顶层）
**0.9.31 当时的装机记录（2026-10-29 的历史证据，别当成现在的实测）**：
装机后实测四项全过：`dpkg -l` / `--version` = 0.9.31、桌面菜单认领 **14 种格式**、
`bash tests/pick.sh /usr/bin/yshjy` **11 项全绿**（含新的 Ctrl-C 第⑤组 5 项，证明修复真的进了装机版）、
真打包解压**逐文件 sha256 与源目录一致**。装前用 `apt-get -s` 模拟过：只升级 1 个包、0 新装 0 卸载。
`dist/` 只留 0.9.31 全套 7 项，**36M**；旧 0.9.30 全套（deb + 两档 tar.gz + 校验 + 两个解包目录）
已按用户要求清掉（2026-10-29）。

**上一版 0.9.30 装机现场（历史，别重复验）**：`dpkg -l yshjy` = 0.9.30、`--version` = 0.9.30、
桌面菜单认领 **14 种格式**、三种补全（bash/zsh/fish）都装到系统目录、
`~/.local/bin/yshjy` 是指向 `/usr/bin/yshjy` 的软链（不是覆盖）。
`dist/` 当时只留 0.9.30 全套 7 项（更早的 0.9.21–0.9.29 + 4 个 `*.before-tree` 共 55 项已按用户要求清掉）；
现在 `dist/` 只有 0.9.31 全套（0.9.30 也已清，见上）。

**装机后真机实测（1200 条目 · 带干扰数据）**：`ys 包 -o 验证.mp4 --decoy 4 --decoy-signatures`
→ 1200 条目、布局 free 原子、🎭 4 段共 64 KB、自检通过；`--list` 与第三方 `7z l` 都列出 **1200 files**；
`--test` 校验通过；真解压出 **1200 个文件**，与源目录**逐文件 sha256 完全一致**；`--help` 里三个 `--decoy*`
参数都在（7 个语言包生效）。
回归：`cargo test` **357 + 29 + 2 全绿**（B 新增 8 条 decoy、C 新增 6 条 seek + 7 条 blockcache）；
`cargo clippy --all-targets -- -D warnings` 干净；`bash tests/all.sh` **41 通过 / 0 失败 / 5 跳过 / 191 秒**
（日志 `~/.hermes/cache/scratch/all_phase3bc.log`；上一轮 39 通过，本轮 +`decoy.sh` +`seek_probe.py`）；
`flutter test` 46 条全绿（本轮没动界面）；`scripts/gen-cli-assets.py` 已重跑（补全 + man 页 46 个长选项）。
> 注：`strings libapp.so` 查中文新字符串恒为 0（Dart AOT 读不出中文字符串，连「选择文件」都是 0），
> 别用这个方法验包新旧，直接开界面看。

**当前该做的（2026-10-29 0.9.31 装机之后）**：
1. ✅ 装机与真机实测：0.9.30 那轮见下面「装机后真机实测」，0.9.31 这轮见上面「当前状态」。
2. ✅ `--pick` 被 Ctrl-C 后光标不恢复 —— 修好并已进 0.9.31（证据在表尾那条 + `CHANGELOG.md`）。
3. ✅ 出包 —— 0.9.31 已出、已验（验包 37 项全绿）、已装机；记录在 `docs/RELEASE-CHECK.md` 顶部。
4. **「多平台适配」整块**（用户口径**最后做**，探测结论见下面那张表，别重复调研）。
   〔用户 2026-10-29 口径：**小修不单独出包，攒一批再出** —— 下次改动仍按这条，别每次小修就发版〕。
   ⚠️ **2026-10-29 夜实测后，这条不再是「只剩一件」** —— 新增一条必修在表尾 🔴：
   「解压报成功 ≠ 已落盘（未 `fsync`）」，属**用户数据安全类**，排多平台之前做。
5. 安卓批要补验的 seek 清单见下面「⏸ 最后做：多平台适配」那一节的子清单（第三期 C 的落地部分）。
6. **优先级排序（2026-10-29 夜实测后定，条目全部指向本文件已有那几张表，不再另开清单）**：
   - **最该修**：表尾 🔴 `fsync` —— 屏幕 ✅ 变真话，直接关系用户会不会丢文件（证据见 `CHANGELOG.md` 未发布节）
   - **最便宜**：Windows 命令行差的两步（`crash.rs` 非 unix 时区占位 + 出 zip + `BUILD-Windows.md`），
     约 1 小时，做完「完成度总览」里 Windows CLI 就从 🟡 变 ✅（账本上多一个完整平台）
   - **最省心**：新用户首用链路（「Linux 还剩什么」#4）—— 走一遍才知道还有没有别的坑
   - **最贵**：Android（SDK/NDK 15–20 G，放最后）；其次 GitHub 公开 + CI 骨架（所有平台的前置）
   - 仍欠：长时写入下的真 I/O 错误实测（「Linux 还剩什么」#3 ③，要写 >3.9 G）

### ⏸ 最后做：多平台适配（整块延后；已探测清楚，到时候别重复调研）

| 平台 | 到时候要做什么 | 谁出包 | 本机现状（2026-10-28 实测） |
|---|---|---|---|
| Linux 终端 + 桌面 | ✅ 已完成 | 本机 | 0.9.26 已装机，`tests/all.sh` 24 通过 0 失败 |
| 安卓 | 装 SDK + NDK + **JDK 17**（现装 25 太新，apt 仓库暂无 17）→ `cargo-ndk` 编 `.so` → `ffigen` 生成 Dart 绑定 → `flutter build apk`；分区存储照 localsend：私有目录直写、外部走 SAF、**不许静默失败** | 本机 | SDK/NDK 都没装；磁盘余 74G（够，约吃 15–20G）；`dl.google.com` 直连 200 能下 |
| Windows | 命令行版：`cargo build --release --target x86_64-pc-windows-gnu`（目标 + mingw + wine 10 都已装，本机可验）；图形界面版只能在 Windows 上编 | **用户在 Win 上自己编** | 平台目录已铺 `gui/windows/`（未提交）；待写 `BUILD-Windows.md` |
| 苹果（macOS + iOS） | 「专属层」：`ffi/` C ABI → Dart 侧 ffigen 绑定 + `FfiTransport` → `ios/` + `macos/` 工程 → `BUILD-iOS.md`（免费 Apple ID 自签，7 天有效期可续） | **用户最后用 GitHub 编** | 平台目录已铺 `gui/ios/`、`gui/macos/`（未提交）；不买 99 美元账号、不上架、不公证 |

#### 安卓批要补验的 seek 清单（第三期 C 的落地部分，**别当成已完成**）

代码已经就位（`src/seek.rs` 探测三档 + `src/io/blockcache.rs` 块缓存 + 协议 `scan.seek`），
但「手机上到底是哪一档」本机验不了（无 Android SDK / 无设备）。到安卓批要回答：

- [ ] SAF 打开的 `content://` 句柄在主流机型上属于哪一档（实测走 `seek_ability`，别猜）
- [ ] 若判成 `block_cache`：8 MiB 上限够不够解一个 2 GB 的 zip（zip 要来回跳 central dir）
- [ ] 若判成 `copy_first`：复制兜底的**预检数字**（`safety::check_space` 的 available 与真实
      可写空间是否一致；分区存储下私有目录的配额）
- [ ] 复制过程被杀 / 空间中途耗尽时的清理（现在只保证「抄完核对字节数」）
- [ ] `scan` 的 `seek` 字段在界面上真的会被用（提示「会先复制一份，占 X MB」）

> 苹果口径（用户 2026-10-28 定，别再问）：不上架、不公证、不买开发者账号，发源码 / 未签名包，谁要谁自己签名 —— 花钱 0。
> **上 GitHub 前两件必修（已探测，别重查）**：① `.git` 300 MB，历史里有 `target-full/` 编译产物（最大 65 MB），
> 推之前必须清历史；② 个人路径 `/home/user` 在 `HANDOFF.md`、`refs/analysis/*.md`、
> `assets/sfx/yshjy-sfx.exe`（**二进制内嵌**）里，要脱敏。**无任何密钥泄露**（sk-/ghp_/AKIA/私钥格式零命中）。

下一步：把 §优化建议 那节从上往下清，一次一行。
> 已知小账（不阻塞发版）：RAR 在「库因 CRC 失败提前 drop writer」时，已写字节仍会落地；
> 彻底解决要迁 `rars::extract_with_control_and_errors` 重写回调层（见 CHANGELOG 第 12 项）。
> RAR 的中断实测已补上（装 `rar` 后加 `tests/rar_atomic.sh`，SIGKILL 打断 → 只剩 part、重试零副本）。

## 阶段 0：体检（✅ 已完成，证据在 `docs/AUDIT-PHASE0.md`）

| # | 项 | 结论 |
|---|---|---|
| T1 | 依赖安全 | ✅ 1 漏洞 + 1 yanked + 1 许可证缺口，已修并验证 |
| T2 | 测试覆盖 | ✅ 66.3%；协议层是真空洞（serve.rs 26.2% / api.rs 8.1%） |
| T3 | 性能压测 | ✅ 2 GB 单文件内存只 7 MB；10 万碎文件 8.6 秒 |
| T4 | 模糊测试 | ✅ 2.38 亿次零崩溃；写盘路径无 target（并入梯队三第 7 项） |
| T7 | 多语言 | ✅ 引擎 7 语全齐；界面未接多语言（架构决定，非缺陷） |
| T8 | 异常场景 | ✅ 路径穿越守住；中断留半截（见梯队一第 1 项）。**2026-10-29 已实测**：磁盘满预检 3 次全命中 / 报成功后立刻拔盘 300MB 全灭（见「Linux 还剩什么」#3、`CHANGELOG.md` 未发布节）；仍欠长时写入下的真 I/O 错误 |
| T9 | 数据安全 | ✅ 不弄坏原有文件；假开关与文案问题已入梯队一、二 |
| T6 | 新手走一遍 | ◐ 用户已实机体验 0.9.25-c「都正常」；缺「下载 → 装 → 出错」完整首用链路 |
| T5 | 装包与发布检查 | ✅ **0.9.26 完成**：对外号去 `-c` 并五处同步 / deb 逐项核对 33 项全绿 / AppImage 评估＝本轮不做（配方见 `docs/RELEASE-CHECK.md`） |

## 阶段 1：要做（按梯队排，梯队一优先）

### 梯队一：✅ 三项全部做完（原子写入 / 错误原因说人话 / 重名策略接线）
> 详细证据见 `CHANGELOG.md` 的 0.9.26 三节，回归探针见 `tests/errmsgs.sh`、`tests/conflict.sh`

### 梯队二：✅ 两项全部做完（自动抓密码收窄 / 设置页语言文案）
> 详细证据见 `CHANGELOG.md` 的 0.9.26 后两节

### 梯队三：工程护栏（防以后改坏）✅ 三项全部做完
> 协议契约测试 / fuzz 写盘路径 / caps 能力位 —— 证据见 `CHANGELOG.md` 对应三节

### 梯队四：体验功能（按真人会撞到的频率排）

| # | 项 | 用户能感觉到什么 | 验收标准 |
|---|---|---|---|

### 待查（偶发问题，别当新功能做）

- [x] ✅ **已定性并修好（2026-10-29）**：整轮里真正会红的是 `rar_atomic.sh` ——
  它默认用 `target/debug/yshjy`，而 `all.sh` 第 1 关**只建 release**。所以「改过 `Cargo.toml` / `src`
  却没手动 `cargo build`」时**必然红**、没改过时绿，看着就像偶发 —— 实为**确定性**，不是 I/O / OOM
  （实测时间戳：`Cargo.toml` 比 debug 二进制新 5 分钟，正是改 0.9.28 号那一下）。
  修法：`all.sh` 第 1 关补一条 `cargo build --quiet`（debug，增量 8 秒、无改动近 0 秒），
  三个 debug 探针（rar_atomic / pack_progress / resume）保持 debug 并写明原因。
  **试过「三个探针全改 release」：不可行，别再重复** —— release 解 81 MB 只要 151 ms，
  `rar_atomic` 的中断点（耗时÷3）打不中、`pack_progress` 的进度事件只剩 1 条，两边当场假红。
  验收：`bash tests/all.sh` **30 通过 / 0 失败 / 4 跳过 / 188 秒**（`all10.log`）。
  证据见 `CHANGELOG.md`「未发布 · 2026-10-29」一节。
  原记录里「`cargo test --release` 那一关失败」今日两次全绿、现场已不可考，故并入本条；
  若再复现仍按老规矩先落完整日志（`bash tests/all.sh > ~/.hermes/cache/scratch/all.log 2>&1`），别再只 tail。

- [x] ✅ **已修（2026-10-29）：探针跑一次弹一次文件管理器** —— `pack_progress.sh` 执行自解压 `.sh`
  验证产物时漏了 `YSHJY_SFX_NO_OPEN=1`，而存根解完默认 `xdg-open` 打开目标目录
  （`src/pack/mod.rs:655`）→ 每轮回归在用户桌面弹一个窗口（用户当场抓到两次）。
  其它四个 sfx 探针早就设了，只有这一处漏。验收：跑前后数 journal 里
  `org.gnome.Nautilus` 激活次数，修后**增加 0 次**。**以后新写「会真执行 sfx 产物」的探针，
  一律带 `YSHJY_SFX_NO_OPEN=1`（`YSHJY_SFX_SILENT=1` 同理，别问确认）。**

- [x] ✅ **已修（2026-10-29）：`--pick` 被 Ctrl-C 打断后光标不恢复，得 `reset`**
  原记录（2026-10-28 pty 实测）：发 `\x03` 后进程以 SIGINT 结束（rc=-2），屏幕上只有
  `\x1b[?25l`（隐藏光标）没有对应的 `\x1b[?25h`，真实终端里光标会一直不显示。
  〔2026-10-29 用户点头，本轮做掉〕

  **根因是两层**（动手时才暴露第二层，原记录只写了第一层）：
  ① `console` 后端把 Ctrl-C 变成 SIGINT，`dialoguer` 的复原代码没有执行机会（原记录已写）；
  ② 一旦接管 SIGINT（信号不再杀进程），`dialoguer` 的阻塞读会被 `EINTR` 打断 →
     `interact_opt()` 返回 `IO error: read interrupted`，**会被当成「❌ 交互勾选出错」报给用户**。
  所以「handler 里补一行显光标」不够，**主线程那条 Err 路也得收**。

  **落地（没走方案 B 自己写按键循环；`ctrlc` 已在依赖里，没加新依赖）**：
  - `src/main.rs` 新增 `装中断接管()`：Ctrl-C **全进程只装一次**、按阶段分派 ——
    `--pick` 在屏 → 复原屏幕 + 退 130；解压中 → 转成引擎「取消」；其余阶段 → 退 130（等价系统默认）。
    这也顺手收掉一个隐患：`ctrlc` 全进程只允许装一次（第二次 `MultipleHandlers`），
    原来的 `let _ =` 会把它吞掉 —— 若 `--pick` 先装过，解压那边的取消接管就**静默失效**。
  - `src/pick.rs`：`在屏中` 标志 + `屏幕守卫`（RAII，早退/出错也复位）、`中断复原()`
    （只在界面里写 `\x1b[?25h`，重复写幂等）、`是中断()`（标志 或 错误文本，双保险 ——
    两个线程谁先醒没保证）；`interact_opt()` 的 `Err` 先判中断，是就复原 + 退 130。
  - 退出码 130 统一成 `main::中断退出码` 常量。
  - **termios 不用碰**：实测 `console` 的 raw mode 是 RAII 的，被信号打断时自己复原
    （pty 前后对比 lflag / ICANON / ECHO / ISIG 完全一致）→ 省掉一层 unsafe 快照。

  **验收**：`tests/pick.sh` 新增第 ⑤ 组 5 项（退 130 / 有 `\x1b[?25h` / 不出现「交互勾选出错」/
  termios 一致 / 一个文件都没解），单项反向验证过（注释掉 `装中断接管()` 重编 → 当场红 2 条）。
  回归：`pick.sh` **11 通过 0 失败**、`tests/all.sh` **41 通过 / 0 失败 / 5 跳过 / 203 秒**、
  `cargo test` **357 + 29 + 2 全绿**、clippy `-D warnings` 干净。
  额外实测「解压中的 Ctrl-C 没变味」：2 万碎文件包解到一半发 SIGINT → 退 130、打印
  「⏹ 收到 Ctrl-C —— 正在收尾」、保留已解 6890 个文件、残留 `.yshjy-part` 0 个。
  文档同步：`docs/CLI-SPEC.md` §4.5 那段「Ctrl-C 得 reset」已改成「干净退 130」。证据见
  `CHANGELOG.md`「未发布 · 2026-10-29 Ctrl-C 光标复原」一节。

  > 原计划的执行顺序留档（**已完成，别再照它做一遍**）：① 依赖已够（`ctrlc` 在架）；
  > ② handler 只做复原 + 退 130；③ 只在进屏期间生效；④ `tests/pick.sh` 加断言。
  > 备用方案 B（自己用 crossterm 读键、不进信号路径）**没用上**，留在
  > `refs/tech/keka-nanazip-inquire-etc.md` §3 备查：
  > 根因上游背书是 `inquire/src/error.rs:27-33`——「**If using `console`, pressing Ctrl+C will trigger SIGINT.**」
  > 而 dialoguer 用的正是 `console`。

### 待查 / 改进（2026-10-28 补拆参考项目时新发现，按铁律记表尾，不插队）

- [x] ✅ **已完成（2026-10-29）**：`gen-cli-assets.py` 三处 fallback 改成 `mapfile -t COMPREPLY < <(…)`，新探针 `tests/completions.sh` 11 项（旧脚本跑会红在劈词/目录/注册三条）；顺带实测否决 `IFS=$'\n' COMPREPLY=(…)` 前缀写法（会泄漏全局 IFS）。原记录：
  🔴 **bash 补全的真 bug：没装 bash-completion 包时，带空格的文件名被劈成两个候选**。
  `scripts/completions/yshjy.bash` 的 fallback 是 `COMPREPLY=($(compgen -f -- "$cur"))`，
  命令替换按默认 IFS 再分词 → `有 空格.zip` 变成 `有` + `空格.zip`。实测：无 bash-completion 时
  COMPREPLY 2 个元素（错），装了是 1 个（对），clap 的 `IFS=$'\n'` 写法也是 1 个（对）。
  修法见 `refs/tech/clap-complete-mangen.md` §6.1（生成器 `gen-cli-assets.py:295`、`:312` 两处）。
  验收：新增 `tests/completions.sh`，断言带空格文件名只出一条候选。

- [x] ✅ **已完成（2026-10-29）**：探针 4 项（函数集合双向 diff / ABI 版本 / `yshjy_` 前缀 / ffi 版本号），反向验证过。**顺带抓到真漂移**：`ffi/Cargo.toml` 停在 0.9.20 → 已改 0.9.29，并做成断言（发版漏改会红）。原记录：
  🟡 **新增探针 `tests/ffi-header-sync.sh`**：`ffi/include/yshjy.h` 是手写的第二份真源
  （头文件 43 行 vs `ffi/src/lib.rs` 9 个 `extern "C"`），改了实现忘改头文件就会漂移 ——
  和 man 页版本号、补全 18/35 项是**同一种病**（已经栽过两次）。做法照 `tests/cli_spec_match.sh`：
  两边抓函数名清单 `diff`，不一致就红。参考 `refs/tech/hiddify-core-c-abi.md` §7.1-B。

- [x] ✅ **scratch 已清（2026-10-29 收尾）**：删掉 `progtest_192424`（12 G 抓帧样本）+
  **805 个 `yshjy_*` 探针现场（7.2 G）**，scratch 从 **19 G → 968 K**，磁盘可用 71 G → 89 G。
  只留 `*.log`（116 K，CHANGELOG 引用的证据）。`/tmp` 里本轮 8 个临时文件也删了。
  ⚠️ **顺带发现的系统性问题（值得下一轮做）**：探针失败时会在 `$TMPDIR` 留「现场」（一个 RAR 现场
  400~500 MB），而且**从不自动清理** —— 攒了 **805 个目录 / 7.2 G** 才被发现。建议给探针加
  「跑完删现场，除非设了 `YSHJY_KEEP_SCENE=1`」或「启动时清掉 24 小时前的同类现场」，
  并把这条写进 `tests/all.sh` 的收尾。**别只记「记得手动清」** —— 那就是这次攒到 7 G 的原因。
  铁律不变：动手前先念清单给用户确认，`rm -rf` 单独一条命令。

- [x] ✅ **已完成（2026-10-29）**：`-o nosort -o bashdefault -o default` 都补了（nosort 判 bash ≥ 4.4），并在「故意不给候选」的分支里 `compopt +o default` 关掉回落。原记录：
  🟢 `complete` 那行补 `-o nosort -o bashdefault -o default`（bash ≥4.4 才有 `nosort`），
  用户可感知的差别：选项候选不再被 bash 按字母序重排。位置 `gen-cli-assets.py:316`。

- [x] ✅ **已完成（2026-10-29）**：做成 CLI `--test` + 协议 `verify_only`（不设 caps 字段：七种格式都能测，加个恒真字段等于假能力位）。只测不写走 IO 层空设备，不建目录/不设属性/跳过空间预检/停用续传，与源包处置互斥；`tests/verify_only.sh` 8 项全绿。原记录：
  🟢 `--test`（测试包完整性）作为独立能力位 —— `Ark` 的插件元数据里 `SupportsTesting: true`
  是一个独立能力（`refs/tech/ark-fileroller.md` §2）。我们现在只有打包侧的 `--no-checksum`，
  解压侧没有「只测不写」的动作。另记：Ark 的能力是**按 MIME 声明**的，我们的 `caps` 是全局一份。

## 2026-10-28 对照审计：真缺口与平台前置（路线见 `docs/ROADMAP.md`）

> 由来：拿「5 份参考项目拆解 + `docs/INTERNALS.md` 内部全景」对 64 条台账逐条核实，
> 结论是 **14 条台账标 ❌ 但代码里早做了**（清单在 `ROADMAP.md` §2.2，`REFERENCE-INDEX.md`
> 的状态列要照它重写），**9 条是真缺口**，外加 Windows 编译的一个实锤阻塞。
> 这一节只记「要做的动作」，为什么这么排、平台怎么切全在 `ROADMAP.md`。

### 第一期：对齐与止血（半天）—— 方案 `docs/ROADMAP.md` §3.1

- [x] ✅ **重写 `docs/REFERENCE-INDEX.md` 的状态列（2026-10-29 完成）** —— 上一版 **14 条失真**
  （标 ❌/🟡 但代码里早就做完了；逐条证据原本在 `ROADMAP.md` §1.2，注意不是 §2.2）。
  这次**不是照抄上一份审计**：14 条逐条 `grep` 复核，每条都换成实测的 `文件:行`，落点列一并更正
  （例：`extract.rs:172,349`→`190 密码候选`、`src/scan.rs（无 ctx）`→`job.rs:311`、
  `--split 未实现`→`pack_post.rs:83`）。后 3 条在 §9.1 重复登记的也指回 §8 实现。
  **重写时又抓到 2 条同一类失真**（不在原 14 条里，ROADMAP 上一轮漏了）：
  ① 「输出目录 + 原包处理」标 ❌ 但 `dest: _outDir` 与设置页 keep/trash 都在 → 改 ✅；
  ② 「主题 + 窗口尺寸记忆」整条标 ❌ 但主题早能切 → 改 🟡（只剩尺寸/位置记忆没做）。
  回验：抽查 10 条证据行号逐行打印核对（`extract.rs:190` 是 `密码候选`、`zip.rs:149` 是
  「64 KiB + 20 B 重叠」、`job.rs:311` 发 `ScanProgress`、`extract7z.rs:239` 内存预检、
  `app_shell.dart:114` 三步引导、`main.rs:1246` MultiProgress、`main.rs:872` comfy-table、
  `pack_post.rs:84` split_file、`home_page.dart:1836` 复制解压方法、`history_page.dart:128` onTap），
  **10/10 都对得上**。仍在标 ❌ 的表格行是 **9 条**（`grep -c` + 逐块列数校验过，不是估的），
  已确认为真缺口不是失真：条目 packSize 校验 / 预览树+部分解压 / 干扰数据+假签名 / 移动端 seek 三档 /
  解压后删除源开关 / 移动端桥接（ffigen）/ Android 分区存储 / 发布工作流拆法 / 工作流单文件骨架。
  （另有 1 行标 ❌ 的是 Keka「无法拆：上游闭源」，不是缺口。）
- [x] ✅ **Windows 编译阻塞已解除（2026-10-29）** —— 全仓 48 处 unix-only 逐个看过，**真正没隔离的只有
  `src/extract.rs:499` 一处**（`std::os::unix::fs::symlink`，`E0433: cannot find unix in os`）。
  其余 47 处**早就 cfg 隔离好了**（`main.rs` 的 SIGPIPE/termios、`crash.rs` 的 `localtime_r`、
  `extract_stream.rs` 的 symlink、`tools.rs`/`resume.rs`/`api.rs`/`restore.rs` 的 `PermissionsExt`/`MetadataExt`
  全都有 `#[cfg(unix)]`；`hardware.rs` 的 `/proc`、`/sys` 走 `.ok()?` 降级；`pack/mod.rs` 的 `xdg-open`
  是 shell 存根里的字符串，不参与编译）。**别信「扫到 48 处就要改 48 处」**，先看有没有 cfg。
  修法：`#[cfg(unix)]` 直接建符号链接；`#[cfg(not(unix))]` 退回「目标是什么就复制什么」
  （目录建同名目录、文件 `fs::copy`），并记一条 `DiagnosticCode::SlowPath` 让界面看得到（不静默）。
  **实测三步全过**：`cargo check --all-targets --target x86_64-pc-windows-gnu` rc=0（7 秒）→
  `cargo build --release --target x86_64-pc-windows-gnu` 出 `yshjy.exe`（13 MB，PE32+ console，19.9 秒）→
  **wine 真跑**：`--version` 正常、解开一个真 zip（中文内容逐字节一致、时间戳有还原）。
  Linux 侧回归 `bash tests/all.sh` **30 通过 / 0 失败 / 181 秒**（`zip_symlink.sh` 探针照旧绿）。
  ⚠️ **wine 实测顺手抓到 Windows 侧一个真 bug**：`crash.rs` 的非 unix `本地时区偏移秒()` 还是
  `return 0` 占位 → zip 里的本地时间被当 UTC 解析，**文件名时间差 8 小时、跨日会显示成明天**
  （实测 10-29 的文件解出来显示 10-30）。这条已记进「平台」节的 Windows CLI 一项，跟时区一起修。
- [x] ✅ **进度条补「已用时」（2026-10-29 完成）** —— 就是 indicatif 里 `{elapsed}` 那件事。
  引擎的进度事件不带 elapsed（不为一句话升协议），所以**外壳自己记**：`终端出口` 新增
  `开始时刻: Mutex<HashMap<u64, Instant>>`，按 job 记第一帧时刻（`-j 2` 时两个任务各有各的起点）。
  渲染放在 `进度消息()`（单任务手绘与多任务 `{msg}` **两条路径共用同一份算法**，跟 ETA 一样），
  `画进度行()` 透传；单测能直接塞 `Duration`，不用等真实时间。
  **踩到一个真 UX 坑（加完才暴露）**：`已 xx` 一加，100 列终端上「当前文件名」被挤掉了 ——
  而文件名恰恰是用户最想看的（「现在在解哪个文件」）。修法：已用**最后才加**，且必须先给文件名留
  **8 列**（跟文件名准入线 `剩余宽 >= 8` 同一个数），放不下就牺牲已用、保住文件名。
  验收：单测 27→**29**（新增「已用不足一秒不显示已零秒」「窄终端先丢已用再丢剩余」，后者带
  **属性断言**：任何宽度下都「不超宽」且「已用出现 ⇒ 文件名也在」）；真实终端抓帧
  （2 GB 单包、24 秒、**102 帧**带已用、**0 帧超宽**，最后帧
  `98.4% 1.99 GB/2.00 GB 1/1 项 61.1 MB/s 已 23 秒 in/big2.bin`）；
  `bash tests/all.sh` **30 通过 / 0 失败 / 198 秒**。

> 第一批另外两件（bash 补全 IFS、`tests/ffi-header-sync.sh`）已记在上面「待查 / 改进」节里，不重复登记。

### 第二期：补用户能感觉到的缺口（1–2 天）—— 方案 §3.2

- [x] ✅ **界面：窗口尺寸记忆（2026-10-29 完成）· 位置记忆在 Wayland 下做不了** —— 不引
  `window_manager`，自己修 GTK 侧（`gui/linux/runner/my_application.cc`）。两个真 bug：
  ① 保存只挂在窗口 `destroy` 上，而现在的界面「忙时关窗口 = 藏到托盘」「托盘退出 =
  `g_application_quit`」两条路都不一定走到 destroy —— 用户用了一个月，`gui-window.txt`
  **一次都没生成**，功能等于没做。改成 configure 后 800ms 防抖写盘 + destroy 兜底 flush
  （定时器在 destroy 里先摘掉，防野指针）。
  ② 保存的是「含 CSD 装饰的 surface 尺寸」（本机实测宽 +112 / 高 +144），载入却当内容区喂给
  `set_default_size` → **越开越宽**：实测 880x700 → 存回 992x849 → 再开 1104x849。改用
  `gtk_window_get_size`（不含装饰，与 `set_default_size` 同基准）。
  **位置记忆：Wayland 协议不许客户端给自己定位**（`gtk_window_move` 无效），本机
  `loginctl` 显示会话 `Type=wayland`，所以不做；X11 才做得到，但本机没有 X11 会话可验收。
  **`window_manager` 评估过、不引**：MIT、1126 赞、30 天下载 89 万、0.5.2 活跃，许可证没问题，
  但它同样是 GTK 侧调 `gtk_window_move`/`set_default_size`，Wayland 下一样做不了位置 ——
  接进来只是给「已经有的实现」再加一份真源。
  证据：`tests/gui_window_state.sh` **8 项全绿**（`YSHJY_GUI_VISIBLE_TEST=1` 才跑，默认跳过，
  因为它会真开窗口）；关键回归是「再开一次尺寸不变（不滚雪球）」。真机分辨率 1366x768
  （逻辑工作区 1280x720），极端值场景（文件被手改成 5000x4000）clamp 后仍会被合成器最大化，
  已实测、不算失败（真实路径存的就是窗口真实尺寸，走不到那里）。
- [x] ⛔ **界面：密码卡「这次不保存」勾选 —— 与项目安全设计冲突，不做（2026-10-29 核实）**
  查代码发现前提不成立：`src/config.rs` 里有一条**保险丝测试** `读密码本绝不改动文件本身`，
  注释原话「将来谁加了『自动把抓到的密码 / 试成功的密码写进密码本』的逻辑，这条测试会红。
  密码本只允许**用户主动**写（自己编辑，或 `--import-passwords`）」。
  也就是说界面现在**根本不保存任何密码**（`_recentPw` 只是只读 chips），
  「这次不保存」这个布尔等于「永远不保存」，做了也是假开关（项目最恨的那种）。
  真要做「记住这个密码」是**新功能**（显式按钮 = 用户主动写），得先定：
  存明文还是加密、怎么让用户看见并删掉、要不要按包名关联 —— 等用户点头再排期。
- [x] ✅ **界面：「解完把原包移回收站」开关 —— 早就做完了（2026-10-29 核实，台账又一条失真）**
  证据：`gui/lib/pages/settings_page.dart:265-283` 有「解压完原包：保留 / 移入回收站」下拉
  （读 `on_done` 在 `:65`、写在 `:155`、副标题在 `:279`）；引擎侧 `src/config.rs:244-246` 解析、
  `src/job.rs:845-855` 按「成功 / 部分成功才动」执行 `trash::move_to_trash`。三处都在，不做。
- [x] ✅ **CLI：`--trash-source` / `--delete-source`（2026-10-29 完成）** —— 选项进 `cli_spec.rs`
  （长选项 40 → 42，护栏单测同步改了），`main.rs` 里 `源包处置` 三态：不动 / 回收站 / 永久删除。
  两条都只在**成功 / 部分成功**时动原包（失败时原包是用户重试的唯一材料，与 `job.rs` 同口径）。
  `--delete-source` 开工**前**二次确认（只认 y/yes），非交互（`--quiet` / `--json` / stdin 不是
  终端）**直接拒绝、退出码 2**，并提示改用 `--trash-source`；默认两个都不动、命令行优先于配置。
  7 语言文案全同步（4 条运行时 + 2 行 help）。探针 `tests/trash_source.sh` **10 项全绿**：
  回收站真有 `files/` + `info/`、非交互拒绝且原包不动、输 y 真删、输 n 取消。
- [x] ✅ **引擎：条目 packSize 校验（2026-10-29 完成，zip + 7z；rar/单流如实说明做不到）** ——
  按头部声明的解压后大小给输出**提前封顶**：多出来的字节一个都不落盘，这一条判数据错误；
  少读（声明 1000 只出 300）判截断。语义照 libarchive `entry_bytes_declared`
  （`refs/tech/libarchive.md` §4）。
  **落点**：`src/extract.rs`（zip，原来只有「写完再比对」，现在改成边写边封顶 + 保留事后比对，
  声明为 0 的老式数据描述符包不封顶）；`src/extract7z.rs`（7z，`entry.size()` 取声明值，
  新增封顶 + 少读判错）。
  **做不到的两条（不是偷懒，是拿不到可信的声明值）**：
  ① **RAR** —— `rars 0.11` 的解压回调 `ExtractedEntryMeta` 只有 name/file_time/file_attr/
     attr_source/mtime_refinement/is_directory，**没有 unpacked_size**（`lib.rs:328-344` 逐个字段核过）。
     绕道方案是解压前用 `members()` 预扫描建「名字 → 大小」表，但 RAR 允许同名条目，
     错配就会把好包误判成损坏 —— 风险大于收益，不做；RAR 侧仍有库自己的 CRC + budget 总量限额。
  ② **单流（gzip/xz/zstd/bzip2 裸流）** —— 这些格式头部根本没有「解压后大小」，
     gzip 的 ISIZE 在文件末尾且对 >4 GB 会取模，不可信；tar 家族走的是另一条路径（tar crate
     自带 size），不在这条待办范围内。
  **验收**：`tests/pack_size_cap.sh` **10 项全绿**（zip 正常 / 多写不落盘 / 少写报截断 /
  7z 正常 / 7z 截短不许成功 / rar 正常回归）；fuzz 写盘路径 60 秒见 `scratch/fuzz60.log`；
  `bash tests/all.sh` 见 `scratch/all14.log`。
- [x] ✅ **nautilus 右键脚本按「目标目录可写」给提示（2026-10-29 完成，方案与原文不同）** ——
  原文要「抄 FileRoller 给两套菜单」，实测做不到：Nautilus script 的菜单文字就是脚本文件名，
  没法按权限动态变；真正的动态菜单要写 Nautilus 扩展（C/Python + 编译安装）。
  退一步：能写 → 原地解（原行为不变）；不能写（U 盘 / `/mnt` / 别人的目录）→ 解到
  「下载/解压-<包名>」，通知里写清「原文件夹不能写，已解到下载」，**绝不静默失败**。
  探针 `tests/nautilus_script.sh` **7 项全绿**（可写 / 只读两条路 + 假 `notify-send` 拦通知）。
- [x] ✅ **补全再修两处（抄 clap，2026-10-29 完成）** —— zsh 目录补全 `_directories` → `_files -/`；
  fish 目录补全 `-r -F` → `-r -a "(__fish_complete_directories)"`（⚠️ 必须双引号：单引号里 fish
  不执行命令替换）。落点 `scripts/gen-cli-assets.py`，已重新生成，`cli_spec_match.sh` **7 项全过**。

### 第三期：结构优化（2–3 天，要设计）—— 方案 §3.3

- [x] ✅ **已完成（2026-10-29）**：预览树 + 部分解压。新增 `gui/lib/entry_tree.dart`（纯 Dart，树/三态/面包屑/摊平，19 条单测）+ 预览页改树形（默认树、可切平铺、三态勾选、状态栏"已选 N/M · 合计 X"、面包屑真过滤、列标题"大小/压缩后"）。**引擎一行没改** —— `index` 本来就全程不变，`select` 早就能用；`EntryItem.packed` 也只是界面没解析（引擎 `--list --json` 一直带）。补了方案里没写但真机会卡住的两处：目录行"只看这一层"下钻入口、列标题。验收：`flutter test` 46 条绿、`tests/tree_select.sh` 7 项绿（序号串 vs 通配逐字节一致）、1026 条目真机渲染正常。证据见 `CHANGELOG.md`「未发布 · 第三期 A」。原记录：
  📐 方案见 `docs/THIRD-PHASE-PLAN.md` §A（依据 PeaZip 结构树 + 设计决策 + 边界 + 验收）。
- [x] ✅ **已完成（2026-10-29，进 0.9.30）**：`--decoy` 干扰数据 + 假签名。新模块 `src/pack/decoy.rs`（8 单测），
  噪声只写在压缩数据之外（L1 封面后 / L2 free 原子 pad 后），随机量由 `种子(输入路径, 盐)` 派生 →
  **同一份输入两次打包 sha256 完全相同**。方案被实测推翻两处：「噪声 ≤ 外壳 10%」不可行（内置封面 1688 B）、
  「假签名后跟随机字节」会随机骗坏 7-Zip（20 种子 × 2 长度 17/40 失败）→ 改「签名 + 30 字节零 + 其余随机」（0/40）。
  探针 `tests/decoy.sh` 14 项全绿，反向验证过（挪到 ZIP 之后当场红 2 条）。原记录：
  📐 方案见 `docs/THIRD-PHASE-PLAN.md` §B（只允许 L1/L2 布局、固定种子可复现、打完三条自检）。
- [x] ✅ **探测部分已完成（2026-10-29，进 0.9.30）**：`src/seek.rs`（`SeekAbility` 探测三档 + 复制到本地带空间预检，
  6 单测）+ `src/io/blockcache.rs`（32 KiB 块 / 8 MiB LRU，7 单测）+ 协议 `hello.caps.seek_probe` 与 `scan` 的
  `seek`/`seek_label`，探针 `tests/seek_probe.py` 7 项。**落地（手机上究竟是哪一档）仍在下面安卓批清单里，别当已完成**。原记录：
  📐 方案见 `docs/THIRD-PHASE-PLAN.md` §C（本轮只做探测 + 块缓存 + 协议字段，落地留 Android 批）。
- [x] ✅ **已完成（2026-10-29）**：`unpack::能力组合` / `能力行()` 升成唯一真源，协议 `hello.formats` 给 12 条（format + shape + 6 个能力位），CLI `--caps` 读同一份；**协议不用升版**（只加字段，老前端忽略即可）。界面侧顺手治了老毛病：设置页原来 spawn `--caps --json` 子进程，iOS 上根本跑不了，改成走 `EngineClient.hello()`。探针 `tests/caps_protocol.py` 5 项（含「两边逐字段一致」与「能力确实按格式」）。原记录：
  `caps` 按格式声明（Ark 的做法，协议面升版，影响 GUI + FFI 两侧）
- [x] ✅ **已完成（2026-10-29）**：新错误码 `HEAD_ENCRYPTED`（连清单都加密 → 必须先去要密码），错误密码单独报 `WRONG_PASSWORD`（不许把「给错了」说成「没给」）。**顺带修掉一个真 bug**：`do_list` 里密码写死 `None`，导致 `--list -p 密码 头加密.7z` 永远失败。`tests/head_encrypted.sh` 5 项全绿。原记录：
  7z 头加密专门提示（现在有 `encrypted` / `encrypted_count`，但「头加密时无密码连清单都列不出」没做提示）
- [x] ✅ **已完成（2026-10-29）**：prerm 只在 `remove|purge` 时动手 —— ① TERM 停掉还在跑的界面
  （关窗口只是藏托盘，不清会留个「已被删掉的进程」）② 清掉 postinst 设过的「双击默认打开」条目
  （改前先备份成 `mimeapps.list.yshjy-uninstall.bak`）。**升级分支立刻早退**：用户可能正在解 8 GB 的包，升级不许杀他的活。`build-deb.sh` 自查 + `test-packages.sh` 各加断言（存在 / 755 / `sh -n` / 必须区分 upgrade），出包验收 37 项全绿。原记录：
  `build-deb.sh` 补 `prerm`（现在有 postinst + postrm，缺 prerm）
- [ ] GitHub 发布流水线（现在只有单平台 `ci.yml`，没有多平台矩阵与 release 汇总）

### 平台

> **唯一真源在下面「全平台（零成本路线）」那一节（①~⑥）** —— 这里原有一份 4 条的简版，跟那节重复
> 登记（Windows / Android / 苹果各写了两遍）。2026-10-29 把这份副本收掉：进度只往那节写，
> **别再起第二份**。已完成的部分已在 ②/⑤ 里标出（引擎交叉编译已通、`yshjy.exe` 已出、wine 验过）。

## 优化建议（终端 ↔ 界面对账出来的缺口，2026-10-28）

> 由来：用户问「终端命令你更新了吗、匹配功能了吗」。逐项对账后发现**两边各缺一半**：
> 引擎其实都能做，缺的是「一边做了、另一边没接线 / 没写出来」。
> 排法按「用户能感觉到」优先，一次只做一行。
> **施工步骤见 `docs/CLI-GUI-PLAN.md`**（怎么下手 / 文件落点 / 红线 / 每步验收）—— 本文只记「做没做」，那边只记「怎么做」，不重复记状态。

### 终端（CLI）侧

- [x] **进度仪表盘**（0.9.26，第二十二批）：条 + 百分比 + 字节 + 正在处理第几项 + 速度 + 剩余时间 + 当前文件名。
  复用引擎里现成的 `EtaEstimator`（α=0.3 平滑、样本 <3 不给估算）；窄终端按「文件名 → 速度/剩余 → 项号」
  的顺序丢字段，40 列也读得懂；中文按显示宽度截断。实测 1 GB 包在 100 列下画出
  `[██████░░░░░░░░░░░░] 34.8% 356.6 MB/1.00 GB 1/1 项 big.bin`，样本够了会接上 `177.0 MB/s … 剩 1 分 12 秒`。
  验收：单测 6 条（时长分档 / 中文截断 / 全字段 / 窄终端不超宽 / 总量未知不硬凑分母 / 不足 1 秒不写剩 0 秒）。
- [x] **进度条改用 indicatif**（参考 `refs/upstream/indicatif`，用户明确要求「别纯手搓」）✅ **已完成（2026-10-28）**
  **约束**（已满足）：剩余时间仍用引擎的 `EtaEstimator` 算好再塞进 `{msg}` —— 不许让 CLI 和界面显示两个不同的「剩余时间」。
  **落地**：进度行的文字段抽成共用函数 `进度消息()`（两条渲染路径同一份算法）；
  单任务仍走原来那条手绘 `\r` 路径（**一个字没改**，7 条单测原样全绿），
  只有**真出现第二个任务**才切到 indicatif `MultiProgress`，每任务一条独立行。
  **实测（pty 真跑）**：① `-j 2` 包A 14 帧 / 包B 21 帧，不再互相覆盖 ✅；
  ② 单任务输出与改前等价 ✅；③ 管道模式一行都不画 ✅。
  **踩到的坑（重要）**：indicatif 在 `TERM=dumb` 下会**静默退化成 hidden**
  （`draw_target.rs` 的 `is_dumb()` 一票否决，不报错不提示）→「一点进度都没有」。
  已加判断：`TERM=dumb` 时退回单行手绘，宁可单行也不沉默。
  Hermes 自己的终端执行环境正是 `TERM=dumb`，本地测试特别容易撞上这个坑。
  新探针 `tests/progress_multi.sh`（4 项）已挂进 `tests/all.sh`。
- [x] **`--pick` 全屏勾选**（用户点名要的「档 3」，**用 `dialoguer` 不手搓**）✅ **已完成（2026-10-28）**
  键位直接是业界共识那套（`refs/upstream/dialoguer/src/prompts/multi_select.rs:245-305`）：
  `↓/Tab/j` 下移、`↑/BackTab/k` 上移、`←/h` `→/l` 翻页、`空格` 勾选、`a` 全选、`Esc/q` 取消、`Enter` 确认。
  **落地**：新增 `src/pick.rs`（预览用 `NullSink`，零进度噪音）；`main.rs` 里 `--pick` 优先于 `--select`；
  非 TTY 报 `cli.pick_no_tty` 并以 USAGE(2) 退出；Esc/q 取消算成功（退出码 0，不解任何文件）。
  3 个 i18n 键 × 7 语言 + help 里一行 `--pick`。新探针 `tests/pick.sh`（6 项）已挂进 `tests/all.sh`：
  **验收①** 勾第 1、3 项与 `--select "1,3"` 解出的文件**逐字节一致**；
  **②** 管道里退出码 2 且提示改用 `--select`；**③** 中文文件名不错行；**④** Esc 取消零产物。
- [x] **`--list` 改表格** ✅ **已完成（2026-10-28）**：引 `comfy-table 8`（MIT，只多带一个
  `unicode-segmentation`），四列「序号 / 大小 / 加密 / 路径」，中文按显示宽度对齐。
  **只在真终端且 ≥50 列时画表格** —— 管道/重定向保持原来一行一条（脚本解析不受影响），
  窄于 50 列也退回老格式（实测 40 列时表格会把路径折成「大文件.b / in」）。
  表头 4 个 i18n 键 × 7 语言。实测 80/100 列中文对齐正确、40 列自动退回、管道输出与改前一致。
- [x] **中文键位表** ✅ **已完成（2026-10-28）**：`docs/CLI-SPEC.md` 新增 §4.5，
  照 `refs/upstream/inquire/KEY_BINDINGS.md` 的 `<kbd>` 表格形式，列全 8 组按键 + 边界行为
  （非 TTY 拒绝 / 一个都不勾 / Ctrl-C 的真实后果）。键位不是猜的 —— 逐条对着
  `refs/upstream/dialoguer/src/prompts/multi_select.rs` 的按键分支抄，并用 pty 探针实测了 Ctrl-C 行为。
- [x] **帮助补三条**（✅ 9c749ee 已完成，2026-10-28 补勾）：`--level`（压缩级别 0-9，界面打包页已有滑块）、`--sfx` / `--sfx-linux`（自解压包，
  界面已有开关）、`--caps`（各格式能力表，界面设置页已有按钮）。三处引擎都实现了，就是 `--help` 没写。
  验收：`yshjy --help` 能搜到这三个词，且文案与界面上的说法一致。
- [x] **补全与 man 页都改成自动生成 ✅ 已完成（2026-10-28）** —— 三处手写副本全部退休
  （**不许再手写副本**，做法见 `docs/REFERENCE-INDEX.md` §9.1）：
  原状实测（2026-10-28 用 `target/release/yshjy --help` 数）：**`--help` 共 35 个长选项，手写补全只登记 18 个，
  缺 17 个** —— `--caps --config --cover --encoding --extract-cover --keep --layout --level --log
  --no-auto-password --no-checksum --no-color --no-readme --password-file --sfx --sfx-linux --split`。
  根因不是「漏抄」，是**手抄副本必然漂移**。
  **已落地（补全这一半）**：新增 `src/cli_spec.rs`（唯一真源，39 项）+ `--dump-cli-spec` 吐 JSON +
  `scripts/gen-cli-assets.py` 生成 bash/zsh/fish 三份（说明文字从 `help.text` 自动提取，不产生第二份文案）+
  `tests/cli_spec_match.sh` 五条对账探针并挂进 `tests/all.sh`。手写三件套已退休（文件头写明「别手改」）。
  实测：bash 里 `yshjy --imp<TAB>` → `--import-passwords`、`-j<TAB>` → `1 2 4 8`、`--lang<TAB>` → 7 种语言；
  三份补全都从 18 项变 36 项对外选项全在；`bash -n` / `zsh -n` 语法过。
  **man 页也已完成**：`docs/yshjy.1` 改由 `docs/yshjy.1.tmpl` + 生成器产出（说明优先取 help 文案；
  help 里只出现在「用法」段的 6 个选项 —— `--list/--rescue/--report/--no-paths/--help/--version` ——
  由 `MAN_DESC_OVERRIDES` 补），版本号从 `Cargo.toml` 读（**不再手写**），`build-deb.sh` 打包前自动重新生成。
  探针已扩到 7 条（补全/man 的选项集合 + man 版本号全部对账）。
  实测 man 页从 22 个选项变 **36 个**，`groff -man -Kutf8` 只剩 3 条「cannot adjust line」排版微告警（中文长行，无害）。

### 功能（新增，用户点名）

- [ ] **边解压边删源（`--reclaim`）**〔**以后更新做，本版不做** —— 2026-10-28 用户口径：
  「这一版核心已经完整，现在要做多平台适配」，此条排到后续版本，**不是否决**〕：
  不是「解完再删」，而是解的过程中就把源让出来 ——
  峰值从「包 + 解出」降到接近「解出」。三份现成项目已拉下来逐篇读过，结论写在
  **`docs/INPLACE-EXTRACT.md`**：`hatchzip`（Rust，从尾部倒着解 + tail 转存 + journal）、
  `delete_when_unzip`（Python，单 ZIP 搬移后截断 / 分卷读完删卷）、`ReclaimArc`（Windows，靠 NTFS 特性）。
  三档落地，互不阻塞：
  · **第一档（建议先做）分卷读完删卷**：我们的分卷归组、卷名推导都现成，风险最低。
    一卷里的条目全部 fsync 落盘才删这一卷；有失败/取消就停手，剩下的卷一个不删。
  · **第二档 打洞**：Linux `fallocate(PUNCH_HOLE)`（用 `rustix` 的安全封装，引擎不必破 `forbid(unsafe_code)`）。
    顺序解压时每批（约 256 MB）fsync 落盘后，释放已读区间；FAT/exFAT/网络盘自动退回「解完再删」并说明。
  · **第三档 倒着解 + tail 转存**：照 hatchzip 全套做，全格式通用但工程量最大，
    而且 7z 的 solid 块天生省不了 —— 除非实际包都是 ZIP，否则不建议现在做。
  安全铁律（不许破）：**默认关** / **先 fsync 落盘再释放**（顺序反了就是断电双丢）/ 有失败·跳过·取消则一字节不删 /
  分卷整组 / 删前把清单和将释放的空间打出来 / 不动 `.sha256`、`.说明.txt`、密码本、内层包。
  验收：3 卷共 3 GB 的包峰值占用 ≤ 2 GB；`kill -9` 后能续；产物 sha256 与常规解压逐字节一致。

### 界面（GUI）侧

- [x] **「只取封面」入口** ✅ **已完成（2026-10-28）** —— 界面上其实**早就接线了**
  （预览页「伪装封面 / 压缩包内容」两个勾选框，只勾封面时主按钮就写「只提取封面」，
  Dart 侧传 `cover: _wantCover, wantExtract: _wantExtract`）；文档落后于实现，已实测坐实。
  **但实测挖出一个真 bug，顺手修了**：`--extract-cover` / 协议 `cover` **不认 `-o` / `dest`**，
  封面一律落在源包旁边 → 只读目录直接失败、可写目录越跑越多（`tests/data` 里实测堆了 36 个
  `A1_steg_modeA_plain.封面 (N).mp4`，就是契约测试每次跑它留下的）。
  修法：`cover_path_in(input, dir)` + `extract_zip_cover_to(input, base, dest)`（旧的
  `cover_path` / `extract_zip_cover` 保留成薄包装，老调用点零改动）；落点目录不存在就建、
  失败把自建空目录收回；顺手改成 `.yshjy-part` → rename 的原子写（半截 mp4 用户会当成原片）。
  **验收**：新探针 `tests/protocol_cover_only.py` **25 项全绿**，已挂进 `tests/all.sh` ——
  ① 只取封面时 `entries_done=0`、dest 里只有封面一个文件、封面是合法 MP4（ftyp 头）；
  ② 源包目录里零副本；③ 不给 dest 时仍落包旁边（老行为不回退）；④ 同名自动改名不覆盖；
  ⑤ 解压 + 取封面同时要时两者共存同一 dest。
- [x] **「救援模式」入口** ✅ **已完成（2026-10-28，0.9.28）** —— 协议新增 `rescue` 命令
  （`api::救援`，移植 CLI 那条链），`serve.rs` 走「一次一个任务」调度（慢路径 + 可 cancel），
  `hello.features` 带上它；Dart `EngineClient.rescue()`（30 分钟超时）；失败页在**救得回来**的错误上
  才给「试着救出来（索引坏了也能抢救）」按钮（`DATA_LOST` / `TRUNCATED`，
  以及 `NOT_ARCHIVE` 但 `looks_like=zip`；密码错、缺分卷不给）。
  > 2026-10-30 修正：本节原来写的 `PARTIAL_DATA` / `CHECKSUM_FAILED` 两个码都不对 ——
  > `PARTIAL_DATA` 是**诊断码**（`DiagnosticCode`，不进错误码通道），`CHECKSUM_FAILED`
  > 引擎里**根本不存在**。判断已收进 `humanize.dart::codeCanRescue` 并加单测。
  `list_only:true` 只重建清单不落盘；不给 dest 落 `<包名>.rescue`（与 CLI 一致）。
  **不用 `tests/contract/` 的 CLI golden 兜（那份已有 `rescue.json`），改用真实进程协议探针**：
  `tests/protocol_rescue.py` **22 项全绿**（样本＝现造「中央目录签名被打乱的 zip」），已挂 `tests/all.sh`。

### 全平台（零成本路线，2026-10-28 定）

> 背景：**用户没有 Mac 设备，也不买 Apple 开发者账号（99 美元/年）**。所以不出官方签名/上架版本，
> 改为「公开源码 + 免费 CI 出**未签名**包 + iOS 由懂的人自编自装」。**花的钱 = 0。**
> **2026-10-28 用户改口径（三次，以最后一次为准）**：①先上 GitHub 再干活 → ②先做优化清单、GitHub 需要时再上
> → ③**（本次，最终）核心功能已完整，现在做多平台适配**；苹果不上架不公证，只发未签名包/源码、谁需要谁自己签名装。
> **下面 ①~⑥ 就是本轮主要工作**，其中 ①（CI 骨架）是 mac / iOS 出包的**唯一零成本路径**。
> 项目是开源的迟早要推，但推的时机由这轮需要决定。本机能力实测：
> · 能出：Linux deb/tar（现成）、Windows **命令行** exe（`x86_64-pc-windows-gnu` 目标 + mingw 已装，可用 wine 本机验）、
>   安卓 APK（需先装 Android SDK + NDK，约 8–10 G 下载，待用户点头）。
> · 出不了：Windows **图形界面**版、macOS、iOS —— Flutter 的这三个平台必须在对应系统上编。
> 参考依据（已查证）：GitHub Actions 对**公开仓库免费**（含 macOS runner）；macOS 未签名 app 用户右键「打开」即可，
> 不需要开发者账号；iOS 用免费 Apple ID 可自装到真机（**7 天有效期**，SideStore 之类可续签）。
> 参考项目（真身已在 `refs/upstream/`，台账见 `docs/REFERENCE-INDEX.md` §9）：
> `rustdesk/`（Rust+Flutter 全平台，CI 矩阵与打包脚本）、`localsend/`（Flutter 全平台，一平台一个 reusable workflow）、
> `hiddify-app/`（ffigen 接 Rust 核心）、`tauri-bundler/`（各平台安装包规范）；
> 工具链：`ffigen`（**选它，不选 flutter_rust_bridge**）、`cargo-ndk`、`fastforge`（原 flutter_distributor）、`cargo-dist`。

- [ ] **① 仓库上 GitHub（公开）+ CI 骨架** —— 这是后面所有平台的前置。
  push 触发 `tests/all.sh`；打 tag 触发构建矩阵。验收：push 后 CI 自动跑绿；`git tag v0.9.26` 能触发构建。
- [ ] **② Windows**：`flutter create --platforms=windows` 补平台目录；产物 zip（exe + 界面目录）。
  **引擎部分已在真正的 Windows 机器上打通（2026-10-30，不是 wine 了）**：MSVC 目标
  `cargo build --release` 57 秒出 **7.8 MB `yshjy.exe`**，`--version` / `--caps` 正常，
  真解了一个伪装成 `.mp4` 的中文名 zip，内容正确。
  **但 Windows 上另有一批只有 Windows 才会犯的错**（时区 / 断点续传 / 回收站 / 密码回显…），
  **清单与证据见本文件末尾「2026-10-30 Windows 实测审计」一节（W1~W10），按那里的编号做，别在这里再抄一份。**
  本项剩余：① W1 补时区 ② W2 修断点续传 ③ W3~W7 按性价比排
  ④ **出两个版本（用户 2026-10-30 拍板，两个都要）**：
     **免安装绿色版**（解压即用、双击 `yshjy.exe`）+ **安装包**（自动挂右键菜单 / 文件关联）
  ⑤ 写 `docs/BUILD-Windows.md`
  ⑥ **图形界面（用户 2026-10-30 明确：一定要上）** → 装 Flutter SDK + 修 Dart 侧 3 处写死的
  `XDG`/`HOME` + 引擎名补 `.exe`（W9）。
     **设置页新增一项「关窗口时：缩到托盘 / 直接退出」，默认「直接退出」**（用户 2026-10-30 拍板；
     与 Linux 版默认「缩托盘」相反，别照抄）。
  **未签名** → 用户首次点「仍要运行」，README 写明怎么点。验收：Windows 10/11 上双击能开界面、能解压、能打包。
- [ ] **③ Android**：`cargo-ndk` 编 arm64-v8a / armeabi-v7a 的 `.so`，`flutter build apk --release`；
  keystore 用 `keytool` 自己生成（免费，**别丢**，丢了以后没法覆盖升级）。
  **重点风险 = 分区存储**：必须说清"能解到哪、不能解到哪"，别做成静默失败。
  验收：真机装 APK → 选一个伪装包 → 解压到指定目录成功，或给出明确的权限提示。
- [ ] **④ macOS（未签名，靠 CI 的免费 Mac 机器）**：`flutter build macos` 出 dmg，**不签名、不公证**。
  README 写明：首次要**右键 →「打开」**（或系统设置 → 隐私与安全性里允许）。
  验收：CI 产出 dmg；在真 Mac 上右键打开能跑（需要借一台或找朋友试）。
- [ ] **⑤ FFI 层** —— iOS 的唯一道路，顺带给 Android 提速。
  **现状修正（2026-10-29，上一版文档是错的）**：`ffi/` **早就存在**（`ffi/src/lib.rs` 9 个
  `extern "C"` + `ffi/include/yshjy.h` 手写头文件），不是「`src/ffi.rs` 不存在」；缺的是
  **Dart 侧的 `FfiTransport`** 和绑定生成。
  **绑定用 `ffigen`，不用 `flutter_rust_bridge`**（结论在 `REFERENCE-INDEX.md` §9.2：我们的 C ABI
  已经是 JSON 进 / 事件出，ffigen 只在 Dart 侧生成绑定、引擎一行不用改；frb 要从 Rust 侧改导出层）。
  另：手写头文件与 `ffi/src/lib.rs` 会漂移 → 要做 `tests/ffi-header-sync.sh`（已记在表尾）。
  验收：Dart 侧 `FfiTransport` 在桌面上跑通一次 list + extract，结果与 `ProcessTransport` 逐字节一致。
- [ ] **⑥ iOS 工程 + `BUILD-iOS.md`**（给愿意自己编的人）：`flutter create --platforms=ios` + Xcode 工程 +
  一页说明（装 Xcode、免费 Apple ID、**7 天有效期**、怎么续）。
  验收：照文档在一台 Mac 上从零编到真机装上（需要借 Mac 或 CI 验证）。

**这个方向明确不做（写死，别反复讨论）**：App Store 上架 / TestFlight（99 美元/年）、macOS 公证、
Windows 代码签名证书（几百到几千/年，用户点「仍要运行」即可）、购买 Mac 设备。

### 已评估、本轮不做

- AppImage 打包 —— 理由与完整配方见 `docs/RELEASE-CHECK.md` §4（本机 glibc 2.41 当基线做出来没人能跑）。
- 终端全屏 TUI 的鼠标支持、终端里画封面预览 —— 各家终端协议不兼容，收益抵不上维护成本。

## 不做（**已经定过，不再讨论**）

- 7z 写包（D18/D22 已评估：只读够用，写包收益低）
- Windows / macOS 全套（自解压 exe 存根、dmg、MSI、winget、跨平台 CI）—— 本阶段冻结
- 云盘 / 同步 / 内置文件管理器 / 追 200 种格式
- 自动更新、崩溃上报（默认关，不急）
- GNOME Shell Introspect 授权（D3：不阻塞，不动）

## 做完的（留个记号，详细证据在 CHANGELOG.md / AUDIT-PHASE0.md）

- ✅ **「解完自动打开目录」的口径定了**（2026-10-29，用户问「自动打开好还是不打开好」后拍板）：
  **自解压 `.sh`** = 真人在终端跑或双击才开，脚本调用/重定向不开；**图形界面** = 默认不开，
  设置页给开关、批量只开第一个目录；**命令行** = 永不开。证据与四场景验证见 CHANGELOG「未发布」三、四节。
- ✅ **探针二进制档位**（2026-10-29）：`rar_atomic`/`pack_progress`/`resume` 必须用 debug，
  `all.sh` 第 1 关补 `cargo build --quiet`（增量 8 秒）。**「全改 release」实测不可行**（release 太快，
  中断点与进度事件数假设全崩），别再试。
- ✅ **探针不再弹用户文件管理器**（2026-10-29）：`pack_progress.sh` 执行自解压 `.sh` 补
  `YSHJY_SFX_NO_OPEN=1`；验收方式 = journal 里 `org.gnome.Nautilus` 激活次数 +0。

- ✅ **RAR 接原子写入**（`CountingWriter` 改走 `PartFile`，Drop 时原子改名；调研确认 `rars` 回调**拿不到条目大小/CRC**，只能「Drop 即落地」——CRC 失败时落地与改前相同、不算退步；真实样本与 `7z` 内容 sha256 完全一致 + 分卷正常 + 2 条落盘语义单测 + **`tests/rar_atomic.sh` 真打断**：SIGKILL 后只剩 part、正式名一个没出现、重试零副本零残留、删文件能自愈）—— 0.9.26

- ✅ **打包进度**（伪装 / 自解压 / 普通三条路径都报；新增 `PackTick::TotalHint` 让拼装段换分母，进度条不再卡在压缩段百分比；拼装用 `拷贝带进度` 替掉 `io::copy`；**CLI 与协议两条代码路径都接**；节流 500→200 ms；实测 2 GB 伪装包 82 条事件、0→100% 无回退；新探针 `tests/pack_progress.sh` 7 项）—— 0.9.26

- ✅ **断点续传**（条目级 + JSONL 记录 + 三重校验：包没变 / 文件还在 / 大小对得上；SIGINT 与 SIGKILL 都能续，`tests/resume.sh` **15 项**探针证明续出来的内容与参照逐字节一致，含**随机 kill 点 12/12 轮**与「改名成功 / 记录没写」临界区定向；新事件 `RESUMED`、`caps.resume` + `resume_level: "entry"`、CLI 7 语言文案、界面备注一句）—— 0.9.26

- ✅ **托盘 / 后台**（GNOME 顶栏图标 + 显示窗口/退出菜单；解压中点关窗口 = 藏到托盘继续跑；托盘状态行显示真实百分比、涨 2% 才刷一次；托盘「退出」先让引擎在数据块边界收尾再退；`caps.background` 改 true；界面 13 条测试）—— 0.9.26
- ✅ **默认打开方式**（MimeType 7→14 种；postinst 给 $SUDO_USER 设默认；新增 set-default-mime.sh 可设/查/还原；`tests/mime_default.sh` 7 项断言隔离实测）—— 0.9.26
- ✅ **首屏单一入口**（本来就收口了，但只是注释里的规矩；新增 widget 测试钉住「FilledButton 只能有一个」，并验证过改坏会失败）—— 0.9.26
- ✅ **五态审计**（五页 × 五态逐页对照；修掉打包页/关于页失败态甩裸错误码；历史页失败降级是有意设计，已记档）—— 0.9.26
- ✅ **zip 符号链接**（看 external attributes 的 Unix mode；目标在条目内容里；安全规则与 tar 共用，绝对路径和 `..` 逃逸都拒；`tests/zip_symlink.sh` 9 项断言）—— 0.9.26
- ✅ **RAR「最后一卷没下完」判得出**（靠头里的 `split_after` 位；缺卷说「分卷不齐」而不是「这不是压缩包」；`tests/rar_volume.sh` 8 项断言）—— 0.9.26
- ✅ **RAR 还原修改时间**（回调拿 `file_time` 按版本换算，整包解完统一 `set_modified`；实测解出 2012-07-13；caps 的 `restores_mtime` 改 true）—— 0.9.26
- ✅ **`caps` 能力位如实**（引擎 `--caps` / `--caps --json` 输出 12 个组合 × 6 项能力；设置页点一下问引擎要，不写死）—— 0.9.26
- ✅ **fuzz 写盘路径**（`extract_target`：8 种组合真解压到沙箱；600 秒 71,661 次 0 崩溃；断言不留半成品、不超限额）—— 0.9.26
- ✅ **协议契约测试**（`tests/contract/` 12 份 golden：事件名/字段名/类型/退出码/错误码；故意改坏两种都能测出）—— 0.9.26
- ✅ **设置页「界面语言」文案改实话**（不再并列 7 个语言代码暗示能切，直说目前只有中文）—— 0.9.26
- ✅ **自动抓密码收窄**（同目录要「名字有说明性或与包同名」，上一级只认同名；父目录 README/无关 txt 不再被读）—— 0.9.26
- ✅ **重名策略接线**（rename/replace/skip 真生效，含单流；「全部按设置跳过」不再算失败；`tests/conflict.sh` 14 项全过）—— 0.9.26
- ✅ **错误原因说人话**（没权限/目标是个文件/路径太长/截断包各给专属原因；`tests/errmsgs.sh` 探针全过）—— 0.9.26
- ✅ **解压原子写入**（六条落盘路径：zip/7z/单流/xz/tar/救援；1 GB 包被杀后只剩 `.yshjy-part`，重试自愈、0 个 `(1)` 副本）—— 0.9.26
- ✅ 依赖漏洞与 yanked 清零、`deny.toml` 适配新版 cargo-deny —— T1
- ✅ 阶段 0 体检 T1/T2/T3/T4/T7/T8/T9（证据见 `docs/AUDIT-PHASE0.md`）
- ✅ 0.9.25-c 打包装机；清掉 0.9.21-c 手工残留（`~/.local/opt/yshjy` + `~/.local/bin/yshjy-gui`）
- ✅ 界面文案表（错误码 17 / 诊断码 10 / 条目结果 6，查不到也不露原始码）—— L3/N1
- ✅ 打包进度事件 `PACK_PROGRESS`（引擎 + 协议 + CLI + 打包页进度条）—— L3/N2
- ✅ `tests/all.sh` 一条命令回归（15 通过 / 0 失败）—— L3/N3
- ✅ 批量单行取消（D5）—— L3/N4
- ✅ Del 键删队列条目（D4）—— 早批次
- ✅ 拖入文件夹提示（D6）—— L3/N6
- ✅ 文件管理器右键菜单（Nautilus 脚本 + deb + 用户级安装）—— L3/N7
- ✅ RAR 分卷 / 去 256 MiB 上限 / tar 符号链接 / 跳过不再报数据损坏 / D8 失败带原因 —— L1+L2

- ✅ **解压报成功 ≠ 数据已落盘（慢盘/U盘上「看到✅就拔」= 全灭）**（2026-10-29 实测；
  **2026-10-30 已按用户拍板的方案实现**，证据全文见 `CHANGELOG.md` 的 `[0.9.34]` 节）
  - **方案（用户 2026-10-30 定）**：设置里一个**默认关**的「解压到U盘/移动硬盘时，等数据真正写完再报成功」；
    打开的人给真进度条；本机硬盘一秒都不多等。**开关关着也要提示「拔盘前先安全弹出」**。
  - **实现**：`src/flush.rs`（认移动设备 + 等真落盘 + 真读数）+ `[safety] flush_movable`
    + `FlushProgress`/`FlushFinished` 两个事件 + 诊断码 `MovableNotVerified`
    + 终端进度行 + 界面检验页 + 7 语言文案。检验耗时**单列**（`stats.verify_millis`），
    `stats.millis` 扣掉它 —— 两个数字各说真话。
  - **真机证据**：关检验时报「完成」那一刻 U 盘实际写入 **0.0 MB**（12 秒后才写入 20 MB）；
    开检验时那一刻已写 **20.0 MB**。界面亲手点过：`用时 0.1 秒` + `落盘校验通过（13.2 秒）`。
    探针 `tests/flush_movable.sh` 4/4 通过（已挂进 `tests/all.sh`）。
  - 原始实测记录保留在下方，作为「为什么要有这个开关」的证据。
  - **现象**：解 300MB 到 U盘，屏幕 `✅ 解开（300.0 MB，用时 0.3 秒）`、退出码 0；
    此刻 `Dirty: 307576 kB` / `Writeback: 0 kB` —— 数据全在内核页缓存，盘上一个字节都没写。
  - **后果**：立刻拔盘（`echo 1 >/sys/block/sdb/device/delete`，不卸载不 flush）→ 重插后
    `fsck.exfat` 报 `clean. directories 1, files 0`，解压目录根本不存在，**300MB 一个文件不剩**；
    内核日志 `device offline error, dev sdb, sector … WRITE` + `lost async page write`。
    真正落盘约 200 秒（该盘持续写速 ≈1.5 MB/s），与提示差 **660 倍**。文件系统这次恰好没坏，
    是因为元数据也没来得及写；拔得晚一点（20~30 秒）就会留下半截文件 + 脏盘。
  - **定性**：不是本轮新引入（`Nautilus` 同样毛病，见 `refs/`），但我们是「打出 ✅ 就算完成」的那个：
    解压结束路径与 `--pick` 都没 `fsync`。**别拿「大多数程序都这样」当不用修的理由**。
  - **修复方向**：解压结束前对目标文件 + 目录 `fsync`，让 ✅ 代表真落盘；或退一步把成功文案改成
    「已写入缓存，正在写盘…」并等真落盘再收尾。顺带把预检文案 `磁盘空间不够（0 B，用时 0.0 秒）`
    里的 `0 B` 改成人话（需要 X / 只有 Y / 还差 Z）。
  - **验收**：解 300MB 到 U盘 → 报 ✅ 那一刻拔盘 → 重插后文件应完整；再补一条能自动化的探针
    （小 exFAT loop 盘 + `dmsetup` error 目标模拟写失败，不必每次真拔硬件）。

## 2026-10-30 Windows 实测审计（在真 Windows 机器上编、跑、解压出来的）

> 由来：项目副本搬到 Windows 机器上开工，第一次**真的**在 Windows 上编（以前只有 Linux 交叉编 + wine）。
> 方法：`cargo build --release` 真编 + `cargo test` 真跑 + 造伪装包真解 + 逐条读平台分支代码。
> **数字都是实跑的**：编译 57 秒 / 产物 7.8 MB / 单测 **341 过 14 挂**。
> 排序按「真人会撞上的概率」，不看实现难度。**做的时候只做最上面一条。**

| # | 项 | 性质 | 一句话 |
|---|---|---|---|
| W1 | ✅ 时区（**2026-10-30 已修**，证据见 CHANGELOG） | 🔴 用户一眼看得见 | ~~解出来的文件时间 +8 小时~~ 实测解出 mtime 与源一致 |
| W2 | ✅ 续传（**2026-10-30 已修，两层根因**，证据见 CHANGELOG） | 🔴 功能假死 | ~~append 下 set_len 被拒~~ 之外还有 **canonicalize 存在与否指纹漂移**（验收时实测挖出） |
| W3 | ✅ 回收站（**2026-10-30 已修**，证据见 CHANGELOG） | 🟠 | ~~只认 XDG~~ Windows 引 trash 5.2 走系统回收站（COM），Unix XDG 实现保留 |
| W4 | ✅ 密码明文（**2026-10-30 已修**，证据见 CHANGELOG） | 🟠 | ~~关回显只有 Unix 分支~~ 换 dialoguer::Password，顺手删手写 termios |
| W5 | ✅ 内存预检（**2026-10-30 已修**，证据见 CHANGELOG） | 🟠 | ~~读 /proc，Windows 永远「内存未知」~~ 外壳注册 GlobalMemoryStatusEx 探针，预检实时取数 |
| W6 | ✅ 配置 `~`（**2026-10-30 已修**，证据见 CHANGELOG） | 🟡 | ~~展开只认 HOME~~ 走 paths::home（USERPROFILE 优先） |
| W7 | ✅ 回归套件（**2026-10-30 全绿：35 过/0 挂/9 明说跳过**，四个深坑见 CHANGELOG） | 🟡 | ~~缺 zip/rar/unrar~~ 7z 造夹具 + MSYS .exe 兜底/星平面 grep/GBK 输出/kill -INT 四坑修平 |
| W8 | ✅ 单测 14 挂（**2026-10-30 清零：355 过 0 挂**） | 🟡 | 10 条随 W2、1 条随 W6；paths 加 cfg(unix)；tar/make_with 系挖出 **WSL bash 假绿**（直起工具修复） |
| W9 | ✅ 图形界面（**2026-10-30 落地**，证据见 CHANGELOG） | ⛔ 用户点名要 | ~~Flutter 没装、3 处 Unix 路径~~ 出包实测通过 + Windows 托盘 + 关窗口设置（默认分平台） |
| W10 | 本机构建环境 | 🔧 不是代码 | 代理 / 沙箱两个坑，不解决编不动 |

### W1 ✅ 时区（2026-10-30 已修，证据全文见 CHANGELOG.md「[未发布] 2026-10-30」）

> **已修**：`GetLocalTime() − GetSystemTime()`（夏令时归 Windows 管），`windows-sys 0.61` 仅 Windows 构建生效。
> 真实解包 mtime 与源完全一致；新增单测 3 条全绿。**与原审计的一处出入**：`SYSTEMTIME` 在
> `Win32::Foundation`（原审计只核对到函数位置）。

- **现状**：`src/crash.rs:78-82`，`#[cfg(not(unix))] fn 本地时区偏移秒() -> i64 { 0 }`
- **实测证据**（2026-10-30）：`2026-10-30 00:44:56` 的源文件，解出来 `08:44:58` —— 整 8 小时，正好东八区
- **根因**：zip 里存的是「本地时间」，转 Unix 时间戳要减本地偏移；偏移写死 0 就等于当成 UTC
- **修法（2026-10-30 查完参考项目后改的 —— 比原方案简单，用这个）**：
  **不要自己算 `GetTimeZoneInformation` 的 Bias / StandardBias / DaylightBias**（单位是分钟、
  符号「西正东负」、夏令时还要判断处于哪一段，三处都容易错）。
  我们只需要「**现在**」的偏移，所以直接 `GetLocalTime()` 减 `GetSystemTime()`
  = 本地偏移秒，**夏令时由 Windows 自己算，零手算**。
  出处：`refs/upstream/time/time/src/sys/local_offset_at/windows.rs`（它走
  `SystemTimeToTzSpecificLocalTime` 的同一思路；我们只取「现在」，还能再省掉 SYSTEMTIME→FILETIME 那步）。
  API 位置已核对（`windows-sys 0.61.2` 源码）：`GetLocalTime` / `GetSystemTime` /
  `GlobalMemoryStatusEx` 都在 `Win32::System::SystemInformation`（feature
  `Win32_System_SystemInformation`）→ **W1 与 W5 共用同一个 feature，只加一条依赖**。
  `crash.rs` 是 bin 的模块（不在 `lib.rs` 的 `#![forbid(unsafe_code)]` 管辖内），unsafe 合法。
- **验收**：解出的 mtime 与资源管理器显示一致；补一条 `#[cfg(windows)]` 单测（偏移落在 `-43200..=50400`）

### W2 ✅ 修断点续传（2026-10-30 已修，两层根因，证据全文见 CHANGELOG.md「[未发布] 2026-10-30」）

> **根因一**：`append` 句柄上 `set_len(0)` 被拒（os error 5）→ 截断分支单独开 truncate 句柄。
> **根因二（验收时实测挖出，比第一层深）**：`绝对化()` 走 canonicalize，Windows 上输出目录
> 存在与否返回的字符串不同（`\\?\` 前缀），指纹哈希跟着漂 → 查不到上次的记录。
> 改为 Windows 纯词法归一（Unix 行为一字不动）。真机验收：硬杀后重跑出现
> 「⏩ 接着上次没解完的来（上次已完成 19 项）」，50/50 全齐 0 副本；`resume::` 单测 12 条全绿。

### W3 ✅ 回收站（2026-10-30 已修，证据全文见 CHANGELOG.md「[未发布] 2026-10-30」）

- ~~只认 XDG，Windows 上会建到当前盘根~~ → Windows 引 `trash 5.2`（COM IFileOperation，
  系统回收站、能还原、不弹窗；crate 自己处理了「PerformOperations 成功≠真删」的坑），
  Unix XDG 实现保留。真删进系统回收站的单测已过；「资源管理器能还原」留老板一眼。

### W4 ✅ 输密码回显（2026-10-30 已修，证据全文见 CHANGELOG.md「[未发布] 2026-10-30」）

- ~~关回显只有 `#[cfg(unix)]` 的 termios 分支，Windows 上退回普通读~~
- **已修**：整个函数换 `dialoguer::Password`（跨平台关回显 + 退格编辑），删手写 termios unsafe；
  管道/stderr 重定向走普通读降级。**唯一留给人的**：真终端敲密码不显示字符这 10 秒肉眼验收。

### W5 ✅ 内存预检 / 机械盘探测（2026-10-30 已修内存半边，证据全文见 CHANGELOG.md）

- ~~只读 `/proc/meminfo`、`/proc/mounts`、`/sys/block/*/queue/rotational`，Windows 上全 `None`~~
- **已修（内存闸）**：`hardware::注册内存探针`（lib 禁 unsafe，探针归 bin：`GlobalMemoryStatusEx`
  实时取数，与 W1 共用 feature 零新增依赖）；盘型（只影响提示与并行度）按审计「可以暂时不做」搁置。
  「造超内存包被拦」的端到端演示因压缩端要实分配同量内存而留待老板（详见 CHANGELOG）。

### W6 ✅ 配置里的 `~` 不展开（2026-10-30 已修，证据全文见 CHANGELOG.md）

- ~~`展开波浪号()` 只认 `HOME`~~ → 走 `paths::home()`（Windows 优先 `USERPROFILE`）；
  测试比较换 PathBuf（Windows 分隔符 `\`，字符串比对永不等）

### W8 ✅ 单测在 Windows 上 14 挂（2026-10-30 清零，证据全文见 CHANGELOG.md）

- 10 条 `resume::tests::*` → 随 **W2** 全绿；`config::波浪号会展开` → 随 **W6** 全绿
- `paths::tests::xdg_绝对路径优先` → 加 `#[cfg(unix)]`（`/tmp` 在 Windows 没盘符，纯 Unix 语义）
- `extract_stream::tests::tar_能展开成目录树` → **挖出更深假绿根源**：原生进程
  `Command::new("bash")` 撞上 System32 的 **WSL bash**（CreateProcess 搜索顺序 System32 先于
  PATH）→ make_with 系的 gzip/xz/bzip2/zstd 四条一直是「静默跳过」的假绿。
  全部改直起工具 + `current_dir`，五条现在**真跑真过**。**结果：355 过 / 0 挂**

### W7 ✅ 回归套件在 Windows 上跑全（2026-10-30 已修，四个深坑全文见 CHANGELOG.md）

- zip 夹具 → `7z a -tzip`；rar/unrar 没有就明说跳过
- **实测挖出的四个深坑**（比"缺命令"深得多）：①MSYS `[ -x/-e/-f ]` 对无后缀名兜底 `.exe`，
  BIN 探测必须先查 .exe；②MSYS grep 在 UTF-8 locale 匹配不了星平面 emoji（🔒）；
  ③Windows 7-Zip 按 GBK 转码输出文件名，grep 要用 ASCII 标记；④MSYS `kill -INT` 送不进
  无控制台的原生子进程，优雅取消场景在 Windows 跳过、硬杀路径全真跑
- **验收达成**：`bash tests/all.sh --no-gui` = **35 过 / 0 挂 / 9 跳（每条跳过带原因）**

### W9 ✅ 图形界面（2026-10-30 落地，证据全文见 CHANGELOG.md「[未发布] 2026-10-30」）

- 三处写死 Unix 路径 → 全走 platform_paths；`flutter build windows` 出包实测通过
- 设置页新增「关窗口时（正忙的话）」：tray/exit，**默认分平台**（Windows=exit、Linux=tray），
  存 `[general] close_action`（引擎侧收编 + 单测）
- Windows 托盘（C++ Shell_NotifyIcon）：与 Linux GTK 同一份 `yshjy/tray` 协议 + 气球通知
- 两个 Windows 构建深坑已记档：MSVC 按 GBK 读无 BOM 源码（CMake 加 `/utf-8`）、
  EncodableValue 无 wstring（新增 Utf8ToWide）
- **留老板肉眼**：GUI 拖包解压/打包、正忙关窗进托盘、托盘菜单、气球通知、设置页保存

### W10 本机构建环境（不是代码问题，但会挡路 —— **下次开工先看这条**）

1. **代理**：注册表里有 `ProxyServer=127.0.0.1:7890`（FlClash 留下的，`ProxyEnable=0` 但 cargo 照读它）。
   FlClash 没开时 7890 没人监听 → `cargo` 报「download of config.json failed」。
   **不用开代理**（直连实测 200），编译时带 `CARGO_HTTP_PROXY= cargo build --release` 就行。
2. **沙箱**：cargo 要执行自己生成的 build script（新生成的 exe），受限环境里会 `os error 5`，要在普通终端跑。
3. **`target/` 里 14 G 是 Linux 编译产物**（`.so` / `.rlib` / 无扩展名 ELF），Windows 上毫无用处，
   删了随时能重建。`gui/build`、`ffi/target` 同理。

### 修复战役里顺带观察到（不在审计编号内，先记账不动手）

- **LANG 没设时多打一行回退提示**：Git Bash 里 `LANG` 为空/`c`，引擎启动会打
  「⚠ 没有 "c" 这个语言包，改用 zh-CN」再正常干活。回退本身是对的（zh-CN 是默认语言），
  只是这行提示在「系统语言本来就是中文」的机器上属于噪音；装成产品包时 `LANG`/`LANG_D` 会是
  `zh_CN`，这行就不会出现。要不要在「回退结果 == 默认语言」时闭嘴，等 W9 界面那轮一起看。

- ✅ **打包页命名/路径逻辑抽成纯函数 + 补单测**（2026-10-30 当天补齐，已还款）
  - 已完成：新增 `gui/lib/pack_naming.dart`（`suggestOut` / `withExt` / `extOf` /
    `baseName` / `dirOf` / `parentOf`），`pack_page.dart` 改为调用它们并删掉私有副本；
    新增 `gui/test/pack_naming_test.dart` **20 条断言**（含 Linux 与 **Windows 反斜杠**两套路径、
    「名义带点的目录不算扩展名」「无扩展名就追加」等边界）。
  - 顺带修出一个**真 bug**：老代码写死 `'/'` 分割路径，Windows 上选 `C:\a\b.zip`
    会被当成「没有目录」，建议输出名直接算错 —— 与 W9 修的「界面三处写死 Unix 路径」同源。
  - `flutter test` **89 过 / 0 挂**（69 + 20）。

## 验收方案（唯一一份）

- 📋 **`docs/AI-验收方案.md`** —— 2026-10-30 起，发版前必须走「验收四层」，
  其中**第三层「AI 真开界面点一遍 + 截图」是新增的、也是今天三连返工的根因所在**。
  里面写清了：调研结论（Maestro 不支持 Flutter 桌面 / Patrol 非主场 / 选 CUA）、
  本机能力盘点、7 条固定用例（A1~A7，每条对应一个历史 bug）、诚实规则（没跑就写"未验"）。
  **开工先读它，别再自己另想一套。**

## 已发版：0.9.34 已带上这些改动（2026-10-30，此节留作记录）

- 🟡 **Windows 路径算错的修复**（提交 `82d3088`，2026-10-30）
  - 内容：`gui/lib/pack_naming.dart` 新增 + `pack_page.dart` 改用纯函数，把写死的 `/`
    换成 `[/\\]`；补 `gui/test/pack_naming_test.dart` 20 条。
  - **现状：代码在仓库里、测试全绿，但 0.9.33 安装包不含这个修复**
    （0.9.33 是修「输出名不跟着换 / 找不到文件」那轮出的）。
  - 后果：在 **Windows** 上打包时，"输出文件"的默认名字会算错
    （选 `C:\a\b.zip` 被当成没有目录）。Linux 上不受影响。
  - 发版时要做的：跟平常规矩一样 —— 五处版本号 → `cargo build --release` →
    `flutter build linux --release` → `release.sh` → `build-deb.sh` → `test-packages.sh` → 装机四验。
  - Windows 侧同步：从本机拉 `82d3088` 之后**必须重编 `yshjy.exe`**，
    否则 Windows 上的界面还是老逻辑（Windows 的界面是 Flutter 编的，不重编等于没修）。

## 2026-10-30 顺藤查同类：「说完成但没验证」的还有哪些（用户追问后扫的）

> 由来：落盘检验做完，用户问「有没有可能不止这一个问题」。
> 结论：**同一个根因还有 3 处**，已全部接上同一个开关（见 `CHANGELOG.md` 的 `[未发布]` 节）。

| # | 位置 | 2026-10-30 现状 |
|---|---|---|
| ① | 打包（pack） | ✅ 已接（CLI + 界面），且**排在自检之前**：自检读的是页缓存，通过≠落盘 |
| ② | 救援（rescue） | ✅ 已接（产出是"救回来的数据"，最不该让人以为安全） |
| ③ | 封面提取 | ✅ 已接（只落盘、不发事件） |
| ④ | 断点续传记录 | ⏸ **故意不改**：断电最多丢最后几条，后果只是重解，不产生坏文件 |
| ⑤ | 空间不足文案 `0 B` | 🔴 **未修**：应改成「需要 X / 只有 Y / 还差 Z」 |

**教训（写进脑子，别再犯）**：**「回读校验」挡不住「没落盘」**。
内容对不对（校验）和东西在不在盘上（落盘）是两件事 —— 以前我们只有前一半，
而前一半还会给出"我检查过了"的假安心。

## T17 界面打磨与打包格式（2026-10-31 晚谈定，**只记账未动手，本轮已冻结**）

> 由来：0.9.39 装完验收时用户看设置页最下面两段，说「这两段你看一下和我聊聊」；
> 聊完定了下面这些。**用户原话：「我先说好这轮已经结束现在聊的你写到清单里面不做」** ——
> 所以这一节只记谈定的做法，代码一行没动。

| # | 项 | 谈定的做法 | 代价 / 注意 |
|---|---|---|---|
| T17-1 | 设置页「支持的格式」 | 标题从「各格式能力（问引擎要的，不是写死的）」改成「支持的格式」；**去掉「读取各格式能力」按钮，进页面直接显示表格**（一行一个格式：能解 / 能打包）。数据源仍走引擎 `--caps --json` | 小活。现在的毛病：括号里「不是写死的」是说给开发者听的；点一下才给表，像在查远程数据 |
| T17-2 | 设置页「其它」改成可调 | **并行任务数 → 三档下拉 `1 / 2 / 4`**，旁边常驻一行怎么选：「机械硬盘、U 盘、网盘选 1；固态硬盘可以 2 或 4。文件又多又小时并行收益最大」。**默认保持 1**（设计口径，不动）。程序已经探到的磁盘类型要显示出来当地道建议（现在那句「机械盘建议 1」就是它）。可执行提醒 → 开关；**压缩比上限整条撤掉**（默认 100 够用）；标题改「性能与提醒」，不再提 config.toml | 中活。写回 config.toml 可复用现有「保存」逻辑；改完要补真机截图验收。**为什么不按 CPU 核数自动定档**：并行的瓶颈在磁盘不在 CPU，机械盘多开反而更慢（磁头来回跳），而「自动探测后自动设值」在廉价固态/网络盘上会猜错 —— 猜错比不猜更糟，所以给档位 + 说清怎么选，值由用户定 |
| T17-3 | ~~设备检测（插拔监听）~~ | **❌ 不做**（2026-10-31 用户决定）。原想法：插入外接设备时再检测一次，据此建议并行数。砍掉的理由：① 启动时**本来就有**机械盘/移动盘探测（设置页「jobs = 1（机械盘建议 1）」那半句就是它给的），已经够用；② 插拔监听要动 C++ runner（GIO `VolumeMonitor`）+ platform channel，为这点收益不值 | — |
| T17-4 | ✅ **完成（2026-10-31，真机截图 3 张）· 打包格式可选** | 引擎 `pack` 加 `format` 字段，界面加下拉：<br>· `zip` —— **保持默认**（主场景是发给 Windows 用户/网盘，双击就开）<br>· `tar.gz` —— Linux 老习惯，兼容性好<br>· `tar.zst` —— 自己备份最优（更快、更小、保 Linux 文件权限）<br>**用户要求：选了 tar 系之后，必须在界面上直接告诉他哪些功能用不了**（不是隐藏、不是点了才报错） | 中活。① 引擎加 tar 写入器（tar + flate2/zstd），补 PROTOCOL.md；② **选 tar 时必须禁用并写明原因**：密码、分卷、自解压 —— 这三样是 zip 格式独有；③ 伪装外壳要先用探针验证：MP4 的 `free` 原子布局是按 ZIP 写的，tar 字节流能不能同样塞进去，验过再承诺；④ 三条协议测试 + 界面两种状态的截图 |

> **T17-4 的界面文案草稿**（等真做的时候用，别忘）：
> - 下拉旁常驻一行：「tar 格式不支持加密和分卷，选它时那几项会自动关掉」
> - 选了 tar.* 之后：密码框置灰并写「tar 格式不支持加密」；分卷、自解压同样置灰并写明原因
> - 原来的「压缩级别 0-9」对 tar.gz / tar.zst 依然有效（这俩也有级别），保留 |

**关于并行数，四份设计文档的口径是一致的（别再翻一遍）**：
- `docs/ARCHITECTURE.md`：「任务级并行（`--jobs N`），默认 1（避免磁盘争抢），批量模式可调到 2~4」
- `docs/CLI-SPEC.md`：`-j, --jobs <N>` 默认 1（顺序跑，对磁盘最友好）
- `docs/UI-SPEC.md` 设置页线框图：`并行任务数 [1 ▾]（磁盘友好）` —— 设计上本来就是下拉
- `docs/V0.5-ENGINE.md`：并行度默认 1；**>1 时必须有退回串行路径**（台账 §2.5：并行路径必须有 fallback）

**另外这条缺口早就在账上**：`docs/AUDIT-GAP.md` 的 E7 写着「并行数 / 关掉自动抓密码 / 保留中间 ZIP —
**无真实入口**」，当时判「极客项，优先级最低」。现在用户主动要调，等于把它从极客项提上来了。

**下一轮做什么**（2026-10-31 用户拍板）：T17-4 打包格式可选是**第一件**（用户主动提的缺口），
然后是 T17-2（用户真会调）、T17-1（顺手）。T17-3 已砍，T17-5「压缩两层」确认就是 `tar.gz`，
已并入 T17-4，不再单独列。


## T18 「设计写了、现在没有」审计（2026-10-31 晚，用户要求全库核一遍）

> 用户原话：「帮我检查一下还有哪些明明设计写了的现在没有的，我明明让 ai 自查了好多遍还是发现一些」。
> 方法：不信任何文档的状态标记，**逐条拿设计文档去代码里 grep 核实**（引擎 src/ + 界面 gui/lib/ + 协议三处都查），
> 界面文案对照 `gui/assets/l10n/zh-CN.json`。**只记账，未动手。**

### 真缺口（设计明确要求、代码里确实没有）

| # | 缺口 | 设计出处 | 现状证据 |
|---|---|---|---|
| T18-1 | **打包布局缺 4 种高级选项** | `DESIGN.md` 15.1 实测矩阵 + 15.2 明确写「高级选项 L3 / L4 / L7 / L8（各配提示）」 | 引擎 `src/pack/mod.rs:35` 的 `PackLayout` **只有 `TailAppend` / `FreeAtom` 两种**；界面语言包也只有 `pack.layout.free` / `pack.layout.append` 两条。缺：**L3 ftyp 后 free（需 7-Zip 解）**、**L4 插在视频中间（技术亮点，要改 mdat size + moov 里的 stco/co64，插入点必须卡 chunk 边界）**、**L7 两个包/双包（通用工具只看到一个）**、**L8 MKV 附件（可选 + 警告）** |
| T18-2 | **打包「自定义显示名」** | `DESIGN.md` 14.2「自定义显示名：伪装成 `电影.2026.1080p.mp4` 这种，用户填」；`CLI-SPEC.md:77` 的 `--name` | 输出名**只读展示**（`pack_page.dart:547` 注释「只读展示（不用 TextField：每帧 new controller 会丢状态）」），文件名跟着输入文件/外壳自动生成，用户改不了 |
| T18-3 | **改时间戳/哈希（防网盘炸链）** | `DESIGN.md` 14.2 表最后一项「改时间戳/哈希 / 防网盘炸链（隐写者有此功能）/ 默认关」；`CLI-SPEC.md:82` 的 `--tweak-time` | `grep tweak\|炸链\|randomize_mtime` 在 `src/` 和 `gui/lib/` **全部零命中** —— 引擎和界面都没这功能 |
| T18-4 | **队列「全部暂停」** | `UI-SPEC.md` 队列段 | 界面只有「移出队列」（`home_page.dart:1387` 附近，Del/Backspace 单条移除），没有整体暂停 |
| T18-5 | **历史页「排序：时间」** | `UI-SPEC.md` | `history_page.dart` 里 `排序` / `sort` **零命中**，历史列表固定顺序，不能切换 |
| T18-6 | **关于页「检查更新」** | `UI-SPEC.md` | `about_page.dart` 里 `更新` / `update` **零命中** |
| T18-7 | **`--keep` 保留中间 ZIP 无界面入口** | `AUDIT-GAP.md` E7（引擎侧早就有） | 引擎有实现（`src/pack/mod.rs:113` `static 保留中间产物`），界面零引用 |

### 不是缺口，是文档过期（该改文档，别当成待做）

| 项 | 文档现在写的 | 实际情况 |
|---|---|---|
| AUDIT-GAP E1/E2/E4/E5/E6 | 「零入口 / 不能选 / 无开关 / 不传」 | **T7 全做完了**（decoy、tail/layout、verify_only、smart_dir、report.out_dir 在引擎+界面+协议三处都能 grep 到） |
| AUDIT-GAP E8 | 「界面多语言整块没接」 | **T14 做完了**（7 语言 479 条，`availableLocales` 7 种） |
| `DESIGN.md` 14.2「分散多段」 | 还列在「全部做成可勾选开关」表里 | `DESIGN.md` 15.1 实测已判 **L5 分散多段 💀 不提供**（`unzip -t` 报 zip bomb、7z 只解 1/4），表里应划掉 |
| `CLI-SPEC.md:298` 帮助文本 | 「更多说明：`yshjy --help-full`」 | 该参数**不存在**（`./target/release/yshjy --help` 里也不打印这行），CLI-SPEC:338 自己还留着 `- [ ]` 待办 |
| `CLI-SPEC.md` 参数名 | `--output` / `--no-notes` / `--no-verify` | 实际是 `--dest` / `--no-readme` / `--no-checksum`（功能都有，名字不同） |

### 核对过、确认没问题（省得下次再查一遍）

`E1` 干扰数据/假签名 · `E2` 布局尾部/中间 · `E3` `--force` 跳过空间预检 · `E4` 只测不写 ·
`E5` 智能解压开关 · `E6` 诊断包输出目录 · `E8` 界面多语言 —— 这七项在**引擎 + 界面 + 协议**三处都能找到实现。
另外：解压码「先从说明文件里找 / 再试试密码本」两个开关**在设置页有**（`settings_page.dart:44`，T8 做的），
UI-SPEC 画在解压页密码面板里，位置不同但功能在；`--encoding`（GBK 等文件名编码纠错）界面也有（`home_page.dart:594` 传参）。

**建议优先级**：T18-1（布局，用户最可能感知，但 L4 要改 MP4 结构、最重）→
T18-2（自定义名字，用户每天都想改）→ T18-3（防炸链，接网盘的人需要）→
T18-4/5/6（界面小事，可顺手一起做）→ T18-7（极客项）。
文档过期那批**应该在做 T16 之前先改掉**，否则下次还会误导（尤其 AUDIT-GAP 的勾叉）。

## 用法（给自己看的三句话）

1. 开工：读 `HANDOFF.md` 顶部 → 回这个文件看梯队一第 1 行 → 只做那一项
2. 收工：`bash tests/all.sh` 全绿 → 证据写 `CHANGELOG.md` → 这里划掉一行 → 本地 git 提交
3. 半路发现新问题：**不要动手**，追加到表尾，编号往后排
