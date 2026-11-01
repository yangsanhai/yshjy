# 更新日志

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [未发布] · 2026-11-01（P2-1 版本号一条命令）

**做了什么**：新增 `scripts/bump-version.py` —— 一条命令把版本号改齐，第二个用法 `--check`
只核对不改（提交前 / 发版前跑，不一致退出码 1）。

```bash
python3 scripts/bump-version.py 0.9.48      # 改到 0.9.48
python3 scripts/bump-version.py --check     # 核对（11 处全表 + AppStream 落后提醒）
```

它管 11 处 / 6 个文件：`Cargo.toml`、`ffi/Cargo.toml`、`gui/pubspec.yaml`、
`README.md`（徽章 + 「现在就能用」标题）、`docs/PROTOCOL.md` 两处示例回包、
两个 `Cargo.lock` 的本地包条目、`docs/yshjy.1` 的版本行。
**不碰** `CHANGELOG.md` / `HANDOFF.md` / `docs/TODO.md`（历史记录，改了就是假账）
和 `dist/`（已发布快照）。

**★ 建这个工具当场挖出两个真问题（都不是我编的，是跑出来的）**：

1. **`ffi/Cargo.lock` 停在 `0.9.29`** —— 落后 18 个版本。那个子 crate（C FFI 库）
   平时不编译，谁也不看它的 lock，所以一直没人发现。
2. **同一个 lock 还缺 29 个依赖**（`trash` / `chrono` / `cc` / `iana-time-zone` …）
   —— 都是 0.9.29 之后引擎陆续加的。`cargo metadata --offline` 报
   `failed to download android_system_properties v0.1.6`，说明这份 lock 早就跟引擎对不上了：
   从 0.9.29 往后，这个 FFI 子 crate 用现有 lock 是编不过的。

顺带记一笔：`data/io.github.yangsanhai.yshjy.metainfo.xml` 的 `<releases>` 最新只到
`0.9.34`，落后 13 个版本 —— 这是 T9（门面）的活，`--check` 现在会提醒，但不算不同步。

**踩的坑（写进脚本注释了）**：第一版把 README 正文那条正则写成 `（v([0-9.]+)）`，
只按「长得像版本号」匹配 → 一次命中 **4 处**，把三条**历史里程碑**
「解压层…（v0.1.0）」一起改成了当前版本，等于篡改版本历史（`git checkout -- README.md`
恢复的）。规则已收紧成 `## ✅ 现在就能用（v…）` 这种**锚定语义前缀**的写法。
凡是「长得像版本号」的串（里程碑 / 示例 / 历史）都必须靠前缀区分开。

**两个设计约束**（都是踩出来的）：
- **原子性**：所有文件先读进内存、全部替换成功才写盘。任何一处正则 0 命中就一个文件都不写，
  绝不留「改了一半」的工作区。
- **lock 按包名锚定，不按旧版本号替换**：lock 里的旧值可能落后很多（`ffi` 那份是 0.9.29），
  拿当前版本当 old 去找会一处都匹配不上。改成「`name = "x"` 后面那行 version 无条件设成新版」。

**验证**：`--check` 报 2 处不同步（就是 ffi lock）→ `bump 0.9.48` 改 11 处 →
`--check` 全绿 0.9.48 → `bump 0.9.47` 改回 → `--check` 全绿 0.9.47。
最终 `git diff` 只剩 `ffi/Cargo.lock` 两行真修复（README 零残留，证明「改→改回」不留手尾）。
`cargo metadata --manifest-path ffi/Cargo.toml` 认可改后的 lock（报 `yshjy-ffi = 0.9.47`，不重写）。

---

### 同批：T15 覆盖率基线（`cargo llvm-cov`）

**做了什么**：`scripts/cov_by_file.py` 入库（`cargo llvm-cov --text` 只给逐行明细、没有按文件的
汇总表，得自己解析），跑出第一份基线。

```bash
cargo llvm-cov --text --no-fail-fast > /tmp/cov.txt
python3 scripts/cov_by_file.py /tmp/cov.txt --min-lines 30
```

**结论**：**13092 / 20249 行 = 64.7%**（51 个文件）。主力模块 74%–99%，是扎实的；
**真正裸的是协议层 —— `src/api.rs` 926 行可执行代码只有 8.4% 被跑到，且一条单元测试都没有。**

| 文件 | 覆盖率 | 可执行行 | `#[test]` | 为什么值得管 |
|---|---|---|---|---|
| `src/pick.rs` | 0.0% | 65 | 0 | 唯一零覆盖模块（`--pick` 交互式，要 tty） |
| `src/api.rs` | **8.4%** | **926** | **0** | 界面 / CLI / FFI 三方共用的唯一命令语义层 |
| `src/main.rs` | 13.3% | 2346 | 12 | **统计口径**（探针跑二进制不计入），不是「CLI 没测」 |
| `src/serve.rs` | 24.8% | 250 | 5 | 协议主循环：BUSY / EOF 收尾 / 事件投递 |
| `src/report.rs` | 36.6% | 347 | 6 | 诊断包 —— 用户出问题时唯一的求助材料 |

**三条口径**（写进报告，否则数字会把人带偏）：
① `main.rs` 的 13.3% 主要是统计口径 —— `cargo llvm-cov` 只统计**被插桩的 lib 测试进程**，
`tests/*.sh` 和 `tests/*.py` 探针跑的是编好的二进制，一行都不计入；而 CLI 参数解析正好在 `main.rs`。
② `ffi/src/lib.rs`（297 行 / 0 个 `#[test]`）**完全不在报告里** —— FFI 是独立 crate，没被插桩，
它的覆盖是**未知**不是 0。
③ 数字会漂，任何 Rust 改动后要重跑。

**带出的新账记为 T19**（`docs/TODO.md`）：补协议层单测，顺序 `api.rs` → `serve.rs` → `report.rs`。
活教材：0.9.47 那个「界面把 `jobs` 写成字符串 → 引擎静默退回默认 1 → 假开关」，
正是缺这层单元边界测试的后果（当时靠人肉真机才发现）。

---

### 同批：拆 `home_page.dart` 第 1 块 —— 队列视图抽成独立 `QueueView`

**做了什么**：`home_page.dart` **3117 → 2876 行**（减 241）。新增两个文件：

| 文件 | 内容 |
|---|---|
| `gui/lib/pages/home_queue_view.dart` | `QueueItem`（原 `_QueueItem` 公开化）+ `QueueView`（原 `_queueView`）+ `_QueueStatusChip` |
| `gui/lib/format.dart` | `formatBytes` —— 原来首页 / 打包页 / 历史页各写一份，这次只把首页那份接过来（另两处**故意不动**，免得一次改三处、出问题分不清） |

搬移是**纯搬移**：只把对 State 的直连（`_queue` / `_foldersScanned` / `setState(…removeAt…)`）换成
构造参数与回调，逻辑一字不改。搬完 **213 条原有测试全绿**，证明行为零变化。

**键盘（T11 的第一笔）**：Enter / Ctrl+E 开始解压、Del 移出当前行、↑↓ 在行间移动焦点。

**★ 键位不是自己拍的**（这条是被用户当场问出来的）：
- **Enter** 来自项目自己的 `docs/UI-SPEC.md` §4b.6「Enter 确认」+ §七「无障碍与细节」；
- **Ctrl+E** 对齐 PeaZip —— `refs/外部-归档软件/PeaZip/peazip-sources/res/share/lang/en.txt:1414`
  原文 `keyboard shortcuts: Ctrl+E (or F12) extracts the entire archive`。

第一版我拍的是 `Ctrl+Enter`，用户问「别的参考项目怎么做的你看过没有」之后才去查，**改成了 Enter + Ctrl+E**。
教训：加任何快捷键前，先查 `UI-SPEC §4b.6` 和 `REFERENCE-INDEX.md` 的「键盘快捷键」行。

**★ 方案也是被参考项目纠正的**：我原先判断「业界也就这样，RustDesk 自己都有 3376 行的文件，不拆也行」——
只看了行数。实际查下去：

| 项目 | 最大页面文件 | 里面几个类 | 最大单个类 |
|---|---|---|---|
| RustDesk `desktop_setting_page.dart` | 3376 行 | **26 个** | 759 行 |
| LocalSend（非 `gen/` 生成代码） | 851 行 | 一个小部件一个文件 | 851 行 |
| 本项目（改之前） | 3117 行 | **4 个** | **2976 行** |

差距在**「一个类独占多少行」**，不在文件行数。所以改成「每块抽成一个独立 widget 类，目标 300–800 行」，
与 RustDesk 的实际做法对齐。完整规范与剩余 9 块清单写进 `docs/ARCHITECTURE.md`「GUI 文件组织规范」。

**`part` 为什么不用**：实测（拆 6 行 + 类闭合到另一个文件）→ **309 个 analyzer 错误**，
Dart 不允许类的花括号跨文件。`part` 只能放顶层内容 —— LocalSend 也是这么用的
（`app/lib/pages/about/packagers.dart` 里是 `const _packagers = {...}` 顶层数据）。

**验证**：`flutter test` **219 条全过**（213 原有 + 6 新增）、`flutter analyze` 干净。
**反向验证两次**：① 把 `isControlPressed` 换成 `isShiftPressed` → 键位测试变红；
② 把 `isEnter || isCtrlE` 改成 `&&` → 两条键位测试同时变红（`Expected: length 1 / Actual: []`）。
两次都改回并复跑全绿，`grep` 自检无 `isShiftPressed` / `isEnter && isCtrlE` 残留。

**下一批**：5b `_fileList`。

---

### 同批：第 5a 块 —— 面包屑路径条（预览树拆成四小块的第一块）

预览树原来是「一块 548 行 / 引用 30 个成员」的打包项。动手前先跑了引用分析，结论改变了做法：

**它自己也在改状态** —— `_crumbBar` 1 处、`_entryRow` 4 处 `setState`。前四块都是「只读数据 + 回调」，
这块不是，所以不能一把搬走。改成拆四小块：

| 块 | 内容 | 行数 | 引用 | 状态 |
|---|---|---|---|---|
| 5a | `_crumbBar` | 38 | 3 | ✅ `pages/preview_crumb_bar.dart`（`CrumbBar`） |
| 5b | `_fileList` | 77 | 6 | ⬜ 下一块 |
| 5c | `_entryRow` | 116 | 4 | ⬜ 有 4 处 setState（展开/勾选），要决定「下沉成本地状态」还是「上提成回调」 |
| 5d | `_previewView` | 325 | 26 | ⬜ 最后做，它负责组装上面三块 + 密码区 + 勾选区 + 主按钮 |

**★ 5a 顺手修了一个真 bug**：第一节显示包名用的是 `(filePath).split('/').last` ——
而这是**本地文件路径**，Windows 下是反斜杠，只认 `/` 会把整条路径当文件名显示出来（第一节直接撑爆）。
改用项目已有的 `baseName()`（`pack_naming.dart`，两种分隔符（`/` 和反斜杠）都认）。
注意**包内条目路径**仍用 `split('/')` 是对的（ZIP 规范固定用 `/`），只改本地路径这一处。

反向验证：把 `baseName` 换回 `split('/').last` → 「Windows 反斜杠路径」那条立刻红
（`Found 0 widgets with text "伪装包.mp4"`）—— 证明这条测试真能抓住它，不是摆设。

5 条测试：根层显示包名不显示「全部」/ 没有包路径才退回「全部」/ Windows 路径只取文件名 /
二级目录三节齐全且点每一节回对层 / 最后一节加粗（区分「我在哪」和「能点去哪」）。

**验证**：`flutter test` **236 条**全过、`flutter analyze` 干净。`home_page.dart` **2691 行**。

---

### 同批：第 5b/5c 块 —— 文件清单 + 每一行（合成一块）

`home_page.dart` **2691 → 2518 行**（减 173）。新文件 `pages/preview_file_list.dart`（`FileListView`）。

**为什么 `_fileList` 和 `_entryRow` 合成一块**：后者只被前者调用，两者共享 `indent` / `tree` / `wide`
三个派生值。拆成两个文件反而要传一个 `rowBuilder` 回调，绕一层还不清楚 —— 一个「列表 + 行」本来
就是一个视图块。

**★ 4 处 `setState` 的处理是这块唯一的判断题，结论是「状态全留父层」**：

| 原 setState 改的 | 为什么不能下沉 |
|---|---|
| `_expanded`（展开哪些目录） | 跟树视图、缩进基准共用 |
| `_picked`（勾选的**文件序号**，`Set<int>`） | 解压时要**传给引擎**，是业务数据 |
| `_crumb`（当前层） | 面包屑和缩进基准都读它 |

下沉会让同一个状态出现两份真相（widget 内部一份 + 父层一份），于是一边显示「已选 3 项」、
另一边实际传给引擎 2 个。改成 3 个回调：`onToggleExpand` / `onTogglePick` / `onDrill`，
这块变成纯 `StatelessWidget`：**只负责画 + 把用户动作报上去**。

**7 条测试**（都是一次过，说明搬移真的没改行为）：

| 测什么 | 为什么值得测 |
|---|---|
| 平铺模式显示**完整路径**不是文件名 | 两个同名文件在不同目录，只给名字就分不清 |
| 树模式显示名字 + 目录带「N 个文件」 | 目录里有多少东西，用户进不进得先知道 |
| 点目录行 → 报展开（两次 = 展开+收起） | 只有展开没收起，等于抽屉打不开也关不上 |
| 「只看这一层」→ 报下钻 | 没这个入口，面包屑只能回不能进 |
| 点文件 → 报勾选；已勾的复选框是 `true` | 勾了但不显示勾，用户会重复勾 |
| 目录**半选**时复选框是 `null`（三态） | 画成 `false` 用户以为没勾，画成 `true` 以为全勾 |
| 窄窗口省掉「压缩后」列、保留「大小」 | 两列数字硬挤，文件名没地方显示 |

**反向验证**：把三态表达式改成 `(st == PickState.all)` → 「半选是 null」那条立刻红
（`Expected: null / Actual: <false>`），改回复跑全绿。

**验证**：`flutter test` **243 条**全过、`flutter analyze` 干净。

---

### 同批：第 5d-1 步 —— 三个纯零件（`MiniButton` / `pickedBytes` / `smartDirHint`）

`home_page.dart` **2518 → 2479 行**。新文件 `pages/preview_parts.dart`。

**为什么先摘这三个**：`_previewView` 主体 325 行要传 **25 个参数**才能搬（17 个数据字段、2 个控制器、
6 个回调），直接搬就是参数地狱 —— 得先把它们收成一个「预览页状态对象」，那是架构活、单独一轮做。
这三个是**纯的**：一个渲染小按钮、两个纯函数，抽出来零风险，还能让主体的参数表短一点。

**★ 顺手解决一个老问题：纯函数反而最难测。**
`pickedBytes` / `smartDirHint` 原来埋在 2976 行的 State 里 —— 要验「两个顶层目录时会不会谎称
解到其中一个里」，得 pump 整个预览页、造假引擎、精确点到某个按钮才间接摸到。抽成顶层函数后
**10 条测试**一次钉死边界：空包、勾了不存在的序号、两个顶层目录、单文件、全在一个目录下、散文件。

反向验证：把 `roots.length == 1 &&` 去掉 → 「两个顶层目录」那条立刻红
（`Expected: false / Actual: <true>` —— 说明「甲」真出现在文案里了，即谎称了解压目标），改回复跑全绿。

**★ 脚本又踩一个坑（已写进技能）**：切方法要按花括号配平，但 **Dart 的命名参数本身带花括号**
（`Widget _miniBtn(..., {IconData? icon})`）—— 从签名行盲目找第一个 `{` 会把参数列表当成方法体，
配平立刻结束、只切掉 2 行，方法体整段留在原地，报一串 `Undefined name 'onTap'`。
正确顺序：① 按圆括号配平跳过参数列表 → ② 取 `)` 之后第一个 `{` → ③ 从那里配平方法体。
（本次靠第二个 assert「`)` 与 `{` 之间只能是空或 `async`」把错误挡下来了，整体没写盘。）

**验证**：`flutter test` **253 条**全过（243 + 10）、`flutter analyze` 干净。

---

### 同批：第 5d-2 步 —— 预览页主体拆成 4 段（`home_page.dart` 2411 → 2378 行）

`_previewView` 原来是 340 行的一个大 `build`。**别一次搬**：它 26 个引用里除了数据还有 4 个带
`setState` 的异步方法，直接搬要传 25 个参数（参数地狱）。改成**按段抽**，每段只依赖 2–7 个东西：

| 段 | 抽成 | 依赖 | 行数 |
|---|---|---|---|
| A 密码区 | `pages/preview_password_box.dart`（`PreviewPasswordBox`） | `encryptedCount` / controller / showPlain / recent / 2 回调 | 51 |
| B 勾选卡 | `pages/preview_pick_card.dart`（`PreviewPickCard`） | `preview` / 2 个勾选值 / 2 回调 | 44 |
| C 备注区 | `pages/preview_header.dart`（`PreviewNotes`） | `notes` 列表 | 20 |
| D 摘要行 | 同上（`PreviewSummary`） | `preview` | 17 |

`_previewView` 主体 **340 → 236 行**，剩下的是组装层 + 多候选提示 / 智能解压预告 / 工具行 / 底部按钮。
**「25 个参数」这个问题根本不用解决** —— 它是「把整块当成一个单位」才出现的，分段之后自然消失。

**三个「把判断收进组件」的处理**（父层少一层 `if`，就少一处能写反的地方）：
- `PreviewPasswordBox`：`encryptedCount <= 0` 时返回 `SizedBox.shrink()`
- `PreviewNotes`：`notes.isEmpty` 时返回 `SizedBox.shrink()`
- `PreviewSummary`：`encryptedCount > 0` 才画锁 chip

**11 条测试**，每条守一个「不该出现」或「必须出现」：
没加密不许摆密码框 / 有加密标题要写几个 / 眼睛按钮只报名不自己改状态 / 输入 trim 后空串转 `null` /
点常用码报给父层 / 没封面不许凭空多一行 / 勾选各自报父层 / 内容行写明文件数 / 没加密不许画锁 /
备注空不许占位 / 备注每条都带图标。

**反向验证三次**（每次都能红）：
- 去掉 `v.trim()` → `Expected: 'abc123' / Actual: '  abc123  '`
- `if (pv.hasCover)` → `if (true)` → 没封面时 `pv.cover!` 空断言直接炸
- `if (pv.encryptedCount > 0)` → `if (true)` → 没加密也画出了锁

**★ 自己抓到的一个疏漏（B 段）**：搬移时把块末尾那个 `const SizedBox(height: 12)` 一起吃掉了 ——
`PreviewPickCard` 和下面的状态栏之间少了 12px 间距。**测试抓不到这个（没有测间距）**，
是核对「被删范围的末行」时发现的（末行是 `SizedBox` 而不是 `),` 就说明多吃了）。
后面 C+D 段特意把终点取在下一个 `SizedBox` **之前**。

**★ 写测试时自己踩的两个坑（记下来）**：
① 第二次 `pumpCard()` 的返回值没接，断言读的是上一个 `rec`（`Expected: true / Actual: <false>`）；
② `Preview.fileCount` 是 **getter**（= `entries` 里非目录的个数）不是构造参数 —— 传个 `int` 不生效，
必须真造 `entries`（否则「7 个文件」永远找不到）。

**验证**：`flutter test` **269 条**全过、`flutter analyze` 干净。

---

### 同批：第 6 块 —— 解压进行屏（`home_page.dart` 2325 → **2245 行**）

两块：`_flushView`（60 行 / 5 引用）和 `_progressView`（115 行 / 17 引用）。

**6A `FlushView`**（「正在检验落盘」那一屏）：只要 4 个参数（峰值 / 待写 / 已等秒数 / 取消回调），零状态。
6 条测试钉的是**「进度条不许说谎」**：算得出比例给具体数值（1000 待写剩 250 = 0.75）；
写完是 1.0；**算不出（`peak<=0`）必须退回不确定态** —— 画一根永远 0% 的死条，在用户眼里就是卡死，
会直接拔盘。另外「请勿拔出设备」必须还在（这一屏存在的理由就是这句话）。

**6B `_progressView` 拆三段**（同一个方法读 14 个 State 字段，硬搬就是参数地狱）：

| 段 | 组件 | 参数 |
|---|---|---|
| 进度条 + 当前文件 + 速度/ETA | `ExtractProgressBar` | 6 个 |
| 四种状态提示 + 第几步 | `ExtractStatusLines` | 7 个 |
| 底部按钮 | `ExtractActionRow` | 6 个（含 3 回调） |

`_progressView` 主体 **115 → 50 行**，只剩组装（顺序 + 间距）。
`_etaText` 原来读 `_paused` / `_speed` 两个 State 字段 → 改成 `etaText(p, paused:, speed:)` 顶层**纯函数**。

**18 条测试，两条直接来自真机验收的教训**（写在 `reason` 里，防止以后被「简化」掉）：
① **暂停时不许再挂速度** —— 用户原话抓到「停都停掉了还显示 `1.04 GB/s · 剩 0 秒`」，看着像没停住；
② **「正在暂停」和「已暂停」必须是两句不同的话** —— 按钮点下去到真停之间有延迟，说同一句用户会一直等。

还钉住了：单个文件只给「取消」（暂停单个没意义、跳过单个等于取消）/ 批量时「取消」改叫「取消整批」/
暂停请求发出后按钮禁用防连点 / 扫描阶段用扫描数字画条（不给 `null`，否则看起来没在动）。

**反向验证**：删掉 `if (paused) return '';` → 「暂停时不报速度」立刻红
（`Expected: '' / Actual: ' · 1000 B/s · 剩 1 秒'`）。

**验证**：`flutter test` **293 条**全过、`flutter analyze` 干净。

---

### 同批：第 3、4 块 —— 空态拖放区、处理中等待页

| 块 | 新文件 | 依赖 | `home_page.dart` |
|---|---|---|---|
| 3 | `pages/home_empty_view.dart`（`EmptyDropView`） | `dragging` + 2 个回调 | 2789 → 2733 |
| 4 | `pages/home_busy_view.dart`（`BusyView`） | 只有 `text` | 2733 → 2722 |

**第 3 块补的 4 条测试里，有两条是把「只写在注释里的设计约定」变成测试**：

- 拖拽中标题必须换成「松手就行」—— 平时挂这句，用户会以为程序卡在某个状态；
- 第一屏只允许**一个** `FilledButton`，次入口弱化成 `TextButton`（调研 07:921-923）。
  这条原来只是注释，**注释不拦人** —— 现在 `find.byType(FilledButton)` 必须是 `findsOneWidget`。

反向验证：把拖拽三元表达式改成恒定 `t('home.dropTitle')` →
`Found 0 widgets with text "松手就行"` 立刻红，改回复跑全绿。

**第 4 块（`BusyView`）零状态依赖**，注释里钉了那个老坑：`CircularProgressIndicator`
是**无限动画**，任何 `pumpAndSettle` 撞上它都会等到超时，测试只能用定次 `pump`。

**验证**：`flutter test` **231 条**全过、`flutter analyze` 干净。

**下一批：第 5 块 —— 预览树**（`_previewView` + `_crumbBar` + `_fileList` + `_entryRow`，
合计 548 行、引用 **38 个** State 成员）。这块和前面四块不是一个难度：前四块都是「一份数据 +
几个回调」，这块得先决定「哪些状态下沉成参数、哪些留在 State」，是唯一需要**先设计再动手**的一块。

---

### 同批：拆 `home_page.dart` 第 2 块 —— 批量完成卡抽成 `BatchDoneView`

`home_page.dart` **2876 → 2789 行**。新文件 `gui/lib/pages/home_batch_done_view.dart`，
只依赖一份 `batch` 结果 + 三个回调（打开输出目录 / 在文件管理器里定位失败项 / 再来一批），逐字搬移。

**★ 顺手捞到一处真空**：这张卡原来**一条测试都没有** —— 全库 `grep batch-done` 只命中生产代码。
它埋在 2976 行的 State 里，测不到也验不细。抽成独立 widget 后补了 **5 条**：

| 测什么 | 为什么值得测 |
|---|---|
| 全成功 → 对勾 + 「解压完成」+ 摘要写成功数 | 全成功却显示感叹号，用户会以为出了问题 |
| 有失败 → 换感叹号 + 列出失败项 + 摘要写失败数 | 不列出来，用户不知道哪个包没解开 |
| 失败项点一下 → 回调带对路径 | 调研 07:135 —— 静默隔离是反面教材 |
| 「打开输出目录」→ 用第一个有产物的目录 | 传错目录等于按钮没用 |
| 一个产物目录都没有 → **不显示**那个按钮 | 没有目录却给按钮，点下去只会报错（反向断言） |

**反向验证**：把失败项的 `onTap` 改成 `null` → 「点一下去定位」那条立刻红
（`Expected: ['/tmp/坏掉的.zip'] / Actual: []`），改回复跑全绿。

**顺手清理**：`_batchOutcomeText` 转发函数（`=> batchOutcomeText(oc)`）的唯一调用点在搬走的块里，
现在没人引用，删掉 —— `analyze` 从 1 个 warning 回到干净。

**验证**：`flutter test` **224 条**全过（219 + 5 新增）、`flutter analyze` 干净。

**★ 第 1 块遗留的小账（记一笔）**：第 1 块的搬移脚本用「下一个方法的**方法行**」当边界，
把夹在中间的那行文档注释 `/// 批量完成卡（Sunpack 三段式…）` 一起删掉了 —— 功能无影响
（新文件里写了等价注释），但**搬移脚本必须把被删范围的首尾行打出来核对**，这条已写进第 2 块脚本的注释。

---

### ★ 真机验收抓到 bug：队列页按键盘完全没反应（`d933357`）

用户真机验收原话：**「按键盘没反应啊」**。这是这批里最有价值的一次验收 ——
因为当时 `flutter test` 全绿，绿得毫无破绽。

**根因（两条，缺一不可）**：

1. 第一版给**每一行**挂 `Focus(autofocus: i == 0)`。`autofocus` 的语义是
   「这个节点首次挂载时，如果所在 FocusScope 还没有别的焦点，就自己接过来」——
   而真实流程是「用户拖文件 → 队列才出现」，那一刻焦点早就在窗口根
   （`_ModalScopeState Focus Scope`）上了，抢不到。
2. 就算焦点移动了也没用：`ListTile` 默认**几乎没有焦点样式**，按 ↓ 屏幕上一个像素都不变，
   用户只能判断成「程序坏了」。

**改法**：`QueueView` 转 `StatefulWidget`，**整块一个 `Focus` + 内部选中索引 `_sel`**，
选中行高亮（`selectedTileColor` `alpha 0.12` + 左侧 3px 竖线），挂载后第一帧主动
`requestFocus()`，鼠标点行同步选中；`didUpdateWidget` 里修 `_sel` 越界。

**★ 为什么原来 6 条键位测试没抓到 —— 假绿的典型形状**：
`queue_view_keyboard_test.dart` 是把 `QueueView` **当根 widget** 挂起来测的，
那种环境下 `autofocus` 必然成功。它证明的是「键位判断逻辑对」，
**证明不了「真实页面里焦点真在队列上」**。

新加的 `test/home_queue_keyboard_integration_test.dart` 换了形状：用 `debugScanItems`
把队列灌进**完整 HomePage**（真实 Navigator / Scaffold 全在），断言
`FocusManager.instance.primaryFocus?.debugLabel == 'queue-list'` 且 Delete 真能删。

**反向验证（关键证据）**：把 `requestFocus()` 注释掉 →
焦点实测 `Expected: 'queue-list' / Actual: '_ModalScopeState<dynamic> Focus Scope'`，
Delete 那条同时红（`Expected: no matching candidates / Found 1 widget with text "一号.zip"`）——
**和用户描述的症状完全一致**，这才叫复现，不是"我觉得修好了"。

**用户真机复验**：**「这回按键正常」**。

**给后续 8 块立的规矩**：凡是「焦点 / 键盘 / 拖放」这类**依赖真实 widget 树形状**的行为，
测试必须用完整 `HomePage` 挂（`debugScanItems` / `debugPreview` / `debugErrorCode` 这些注入口就是为此存在的），
**不许把子 widget 当根挂** —— 否则就是假绿。

## [0.9.47] · 2026-11-01（真机抓出来的：设置页「本机磁盘类型没探到」）

**这是用户截图点出来的，不是我测出来的**：0.9.46 装完，设置页那块小字写
「本机磁盘类型没探到 —— 保守起见保持 1」，可同一屏上底部状态栏写着引擎 0.9.46、
「支持的格式」也列全了 12 行 —— 说明引擎是好的、hello 也通了，偏偏磁盘那一项丢了。

**查的过程（都是真跑，不是猜）**：

1. 直接问二进制：`printf '{"id":1,"cmd":"hello"}\n' | /usr/bin/yshjy --serve`
   → `"disk":"rotational"`。装机版也对。**引擎侧没问题**。
2. 写临时探针用真引擎 + 真 `SettingsPage` 跑一遍 → 正常显示「机械硬盘」。
   **单页面也没问题**。
3. 差别只剩真机上**应用一启动就有三个人同时要 hello**：`AppShell`（要引擎版本）、
   `SettingsPage`（要 `hello.disk`）、设置页的格式表（要 `hello.formats`）。
   三个请求一起打向一个**刚 fork、还没预热**的引擎进程，真机上（release AOT + 机械盘
   冷启动）最先发出去的那次失败 —— 而最早发出去的正是磁盘那一次。

**修法两处（都不是"多试几次"的敷衍）**：

- `EngineClient.hello()` 改成 **Single-flight + 进程内缓存**：并发调用合并成一次请求，
  成功结果缓存（引擎能力表在一次运行里不会变，本来就不该问三遍）；
  **失败不缓存**，下次调用会重新发。要强刷就 `hello(refresh: true)`。
  顺带省掉两次往返。
- `_loadDisk()` 失败后重试一次（300ms）。配上失败不缓存，冷启动那次失败能自愈。

**验证**：新增 `gui/test/engine_hello_test.dart` 2 条（并发三次只发一次命令 /
第一次失败不缓存）→ `flutter test` **213**；`settings_perf_widget_test.dart` 加一条
「冷启动第一次 hello 失败 → 重试一次就拿到」（**反向验证**：把重试去掉，这条立刻红）。
`flutter analyze` 干净 · `cargo test` 403+31+2 · `all.sh` 全绿。包 0.9.47，`test-packages` 37/37。

**顺带修的测试 flaky**：`settings_switch_widget_test` 里「点保存」原来等固定 120ms，
全量并发跑时会假红（单跑绿）。改成轮询到「文件里真的写出去了」。

**给后续的人**：以后再加任何「进页面就问引擎」的地方，先想清楚会不会和启动抢跑 ——
`hello` 现在合并了，但别的命令（`scan` / `list`）没合并，别在 initState 里盲发。

## [0.9.46] · 2026-11-01（界面攒一批：并行数三档 · 格式表自动显示 · 历史排序 · 完全离线 · 删 --keep · 诊断包说明）

这一批是「攒一轮做、只开一次界面验收」的界面小事，六件一次做完：

- **T17-2 并行任务数改成真能调**（原来在设置页只是**一行说明文字**）：
  三档下拉 1 / 2 / 4，默认仍是 1（设计口径：机械盘不该被打成碎片）。
  旁边常驻一句怎么选：「机械硬盘、U 盘、网盘选 1；固态硬盘可以 2 或 4」。
  **不按 CPU 核数自动定档** —— 并行的瓶颈在磁盘不在 CPU，机械盘多开反而更慢
  （磁头来回跳）；而「探测完自动设值」在廉价固态 / 网络盘上会猜错，猜错比不猜更糟。
  ★ **`jobs` 必须以整数写盘**（`jobs = 2`）。写成 `jobs = "2"` 时引擎 `as_integer()`
  读不到 → 静默用默认 1 → 用户选了并行、其实还是一个一个解。这条有测试钉着。
  ★ 配置被手改成三档以外的值（3 / 16）会**夹到最近的档位**（2 / 4）——
  DropdownButtonFormField 遇到「value 不在 items 里」会直接 assert 崩。
- **`hello.disk`（新协议字段，只加不删）**：设置页那句「本机磁盘：机械硬盘 — 建议保持 1」
  现在是**真探测**来的 —— 引擎读 `/sys/block/<dev>/queue/rotational`
  （`hardware::detect_disk_kind`），目标是 `[output] dir`，没有就用 HOME。
  取值 `solid` / `rotational` / `unknown`；探不到就说「没探到」，**绝不假装是固态**。
  老引擎没有这个键 → 界面回落 unknown，照常能用。
- **可执行提醒变成真开关**（原来也只有一行 `warn_executables = true` 的说明）。
  引擎侧这个键一直在读，只是界面从来没给入口。
- **压缩比上限那一整条撤掉**（默认 100 够用，摆在设置页只会让人以为要调它）。
  段标题从「其它（改 config.toml）」改成「性能与提醒」—— 不再对用户提配置文件。
- **T17-1 格式表进页面直接显示**：去掉「读取各格式能力」按钮。
  标题从「各格式能力（问引擎要的，不是写死的）」改成「支持的格式」——
  括号里那句是说给开发者听的。读不到时如实说 + 一个「重试」
  （**故意不用转圈动画**：无限动画会让任何 `pumpAndSettle` 等到超时）。
- **T18-5 历史页排序**（UI-SPEC 4b.2 画了「排序：时间 ▾」，代码里一直零命中）：
  时间 / 名称 / 大小三档 + 一个方向按钮。默认时间、新的在上（和存储顺序一致，
  老用户看不出变化）；换字段时方向自动复位（名称 → A→Z，时间 / 大小 → 降序）。
  ★ **名称排序不承诺拼音序**（中文按码点排看着本来就有点乱，界面上也没这么写），
  排序只影响显示、**不回写历史文件**（有测试钉着）。
- **T18-6「检查更新」砍掉，改成「完全离线」的机器断言**（用户口径：
  「保证用户安全，尽量本地解决」）：新增 `tests/offline_probe.py`，三条一起查 ——
  Rust 依赖（Cargo.toml **和 Cargo.lock**）里不许有 HTTP / TLS 栈、
  `gui/pubspec.yaml` 里不许有网络包、源码**非注释行**里不许出现 `http(s)://`。
  关于页新增「完全离线」卡片，正面告诉用户这里没有联网这回事。
  为什么连 Cargo.lock 也查：直接依赖干净、某个依赖把 reqwest 拖进来，运行时一样能发请求。
- **T18-7 `--keep` 整条链删掉**（用户口径：「打包完就删了，不用留」）：
  引擎 4 处「保留中间产物」分支 + 5 处 `intermediate` 字段、`main.rs` 的开关与打印、
  `cli_spec.rs` 的选项表、CLI-SPEC 两行、探针第 ④ 项、生成器映射，**外加 7 个语言包
  `--help` 里的那一行**（最后一项是 `cli_spec_match.sh` 第 4 步抓出来的 —— 它专门防
  「文档说有、二进制里没有」）。man 页与三份补全由生成器重跑（长选项 53 → 52）。
- **P2-2 诊断包落点写清楚**：正文补「自动存到系统的『下载』文件夹」。
  参考项目实测结论：Keka 只给「复制 / 保存」、LocalSend 只留内存 200 条，
  都不主动写文件；我们的诊断包已经更好（脱敏 + 固定落下载 + 打开文件夹），
  只补这一句，不加设置项。

**验证**（全部真跑）：`cargo test` **403 + 31 + 2** · `clippy -D warnings` 干净 ·
`flutter test` **210**（新增 17 条：性能段 13 + 历史排序 4，另修 3 个老测试文件）·
`flutter analyze` 干净 · `cli_spec_match.sh` **7/7** · 7 语言 **530 键**（逐条对齐）·
`bash tests/all.sh` **58 通过 / 0 失败 / 5 跳过 / 252 秒**（比上轮多一项：offline_probe）。

**踩的坑（都写进测试注释了，别再犯）**：

1. **假传输必须清 `EngineClient` 的状态**。`EngineClient` 是全局单例，`_started`
   一旦为 true 就不会再调 `transportFactory` —— 同一个测试文件里的第二个用例会对着
   **已经关掉的旧假流**发命令，永远等不到回包。症状：单跑这个文件全绿、全量并发跑偶发红。
   解法：`EngineClient.resetForTest()`（`@visibleForTesting`）+ `installFakeEngine()`。
2. **别用「睡固定毫秒」等异步 I/O**。`300ms` 在 8 个测试文件并发时不够，于是
   「jobs = 4」那条偶发红、单跑又绿 —— 最费时间的那种假红。改成**轮询到条件成立**
   （保存按钮可用 / 格式表出结果 / 列表出条目）。
3. **历史页新加的排序下拉要求 Material 祖先**。`DropdownButton` 不是自带 Material 的
   组件，`MaterialApp(home: HistoryPage())` 会直接抛「No Material widget found」。
   真实应用里有 AppShell 的 Scaffold，所以只在测试里补一层 Scaffold。
4. **`--help` 的选项说明不在 Rust 源码里**，在 `locales/<code>.toml`（7 份）。
   只删代码不删这里，`cli_spec_match.sh` 第 4 步会判「假选项」—— 那道闸就是为了这个。
5. **T17-1 让设置页 `initState` 开始连引擎**，而 AppShell 的 IndexedStack 会把五个页面
   全构建一遍 → `widget_test.dart` 原来不注入假引擎，突然开始真 spawn 子进程
   （然后 dispose 时撞「ProcessTransport 还没 start()」）。给冒烟测试也补了假引擎。

## [0.9.45] · 2026-11-01（T18-4 真机验收修的两处 + 一处同类）

**这两处是用户在真机上点出来的，不是我想出来的**（他的截图里看得一清二楚）：

- **恢复后状态栏还挂着「已暂停」** —— 他点「继续」之后进度明明在涨
  （331.8 MB / 1.38 GB），底部状态栏却仍写「已暂停，点「继续」接着跑」，界面在撒谎。
  原因：`PAUSED{paused:false}` 那条分支只处理了 `true`，`false` 没推新文案，
  上一次写的「已暂停」就一直留在状态栏。
  修：恢复时把状态栏打回「正在解压…」。回归测试新增 2 条
  （暂停 → 状态栏有「已暂停」；恢复 → 不许留「已暂停」），
  **反向验证**：把恢复那段注释掉，第二条立刻变红。
- **暂停时还在报速度** —— 暂停画面上挂着「1.04 GB/s · 剩 0 秒」，停都停了还显示在动，
  看着像没停住。修：`_etaText` 在 `_paused` 时返回空串（恢复后下一帧进度事件会算回来）。
- **同类问题（顺藤摸出来的）**：批量结束没复位 `_paused` / `_pauseRequested` ——
  如果 `pause` 请求发出去了、但任务在到达挂起点之前就解完了，按钮会永远停在灰色。
  修：`_phase = Phase.done` 时一并清零。**规矩记下：凡是按钮状态标志，都要有
  「任务结束」这个终点复位。**

**给后续的人**：为了能回归「状态转移」而不是只有「静态状态」，`HomePage` 多了一个
`debugEvents`（首帧后把事件按顺序喂进处理器，生产代码永远 null）。没有它，
「恢复后有没有清掉文案」这类只有事件驱动的东西根本测不到。

踩坑记录：这一轮我自己用中文变量名写 Dart（`预设事件` / `状态`）→ 编译不过。
**Dart 标识符不许中文**（这条本来就在项目记忆里，手滑了），改成 `preEvents` / `statuses`。

## [0.9.44] · 2026-11-01（T18-4 · 队列「全部暂停 / 继续」）

- **是什么**：批量解压时可以**整体暂停再继续**。以前只有「移出队列」（单条）和
  「取消整批」（一停就没了），想中途让机械盘歇一会儿只能整批取消、回来从头跑。
  现在批量跑的时候进度页多一个「全部暂停」，停了还能接着跑。
- **★ 这是「软停」，不是打断**：挂起点选在 `ctx::entry_done`（每解完一个文件那一刻，
  ZIP / 7z / RAR / tar / 流式**所有格式共用这一处**）+ `batch` 循环的任务之间。所以：
  · **不打断正在解的那个文件** —— 它解完才停，已解完的条目一条不丢；
  · **继续时不重解** —— 从下一个条目接着来（机械盘上重解一个 7 GB 文件要几分钟，
    这个代价换成了「点暂停后要等当前文件解完才停」，界面把这句写在按钮下面）。
- **协议**：`pause` / `resume`（与 `cancel` 同级：忙的时候也能用），返回
  `{paused, delivered}`；新事件 `PAUSED{paused:true|false}`。
  ★ **回执 ≠ 已停住**：回执只说明「请求送到了」，界面必须按 `PAUSED` 事件翻按钮，
  否则就是「点了没反应」。这条同时写进 `PROTOCOL.md` 和代码注释。
- **界面**：批量（>1 项）才有按钮 —— 单个文件解压不给（它本来就快跑完了，给了反而像卡住）。
  点下去先显示「正在暂停…（当前文件解完就停）」且按钮置灰，收到 `PAUSED` 后翻成
  「继续」+「已暂停，点「继续」接着跑」。7 语言各 4 条文案（**518 条**）。
- **验证**：`cargo test` **403 + 31 + 2**（新增 4 条暂停单测）；`tests/t18_4_pause.py`
  **17/17**（已挂 `all.sh`）；`flutter test` **191**（新增 4 条 widget 测试）；
  `clippy -D warnings` / `flutter analyze` 干净；`bash tests/all.sh` **57 通过 / 0 失败 / 264 秒**。
  **反向验证两次都做过**：① 删掉 `entry_done` 的挂起点 → 探针 5 条立红
  （收不到 PAUSED、文件照样解到 300/300、取消也卡在条件变量上）；② 把按钮条件放宽成
  「有任务就显示」→ 单文件那条 widget 测试立红。
- **踩到的三个坑（都写进探针注释了）**：
  · `/tmp` 是 tmpfs，87 MB 解进去只要 0.3 秒 → 暂停根本来不及落在中途，探针样本必须放真盘；
  · 事件行的事件名要认 `kind`（大写 `ENTRY_FINISHED`），`event` 里那个键是 CamelCase，
    拿它比永远不匹配；
  · `hello` 里命令列表的字段叫 `features`，不叫 `commands`。

## [0.9.43] · 2026-11-01（T18-1 · 打包布局 L3「放在文件头部」+ L7「两个包」+ L8「MKV 附件」）

- **是什么**：打包时把 ZIP **包成 `free` 原子插在 `ftyp` 之后**，视频照常能播
  —— DESIGN §15.1 的 L3（「放在文件头部（抗尾部扫描）」）。命令行 `--layout L3`，
  协议 `pack.layout: "head"`，界面布局下拉从三档变四档（free / 接末尾 / **放文件头部** / 插中间）。
- **与 L4 的三处差别**（照抄就把视频写坏，逐条都有单元测试守着）：
  · 插点**固定** = `ftyp` 末尾（不挑 chunk 边界，因为没往 `mdat` 里插）；
  · **`mdat.size` 一个字节都不改**（`mdat` 跟着整块后移，长度没变）；
  · 偏移补偿是**全表**：后面所有字节整体后移 → 每条 `stco`/`co64` 都 `+插入长度`。
- **★ 本项踩到的坑（假绿温床，已写成断言）**：第一版把「插入长度」按 `zip_size` 算，
  忘了 `free` 头还有 8 字节 → 所有 chunk 偏移偏 8 字节。表现极其阴险：
  **`ffmpeg rc=0`、文件能打开**，但 stderr 满屏 `Reserved bit set` /
  `Prediction is not allowed in AAC-LC`（音频全坏）。修法是把头长抽成 `free原子头长()`
  一处定义、插槽与写入共用同一阈值。**反向验证做过**：把头长改回 0（复现 bug），
  探针立刻在「rc=0 但 stderr 有内容」这条上变红；改回来 26/26 全绿。
- **干扰数据（`--decoy`）可以同用**（与 L4 相反）：插点在文件头部，噪声一律落到
  **整份产物的末尾**，不碰任何偏移 —— 视频仍 rc=0 且 stderr 空，包仍解得对。
  同时修掉一处**误报**：`self_check_at` 那条「噪声压到 ZIP 上」的检查假设噪声在
  ZIP 之前（L1/L2 的形态），L3 拿它比必然报「ZIP 起点 36 < 65536」；现在 L3 传 0
  跳过，改由探针用「产物大小 = 基础 + 噪声」+「解出来 sha256 一致」两条盯着。
- **不做的事**：封面不是 MP4 / 找不到 `moov` 或 `stco`（分片 MP4）→ **降级**
  （MP4 → free 原子，其它 → 尾部追加）并说明原因；32 位偏移溢出（>4 GiB）→ **拒绝**。
  降级文案走 `不能插::头部说明()`，不套用 L4 那句「没法插在视频中间」（说错布局比不说更糟）。
- **代价（界面与文档都必须写）**：ZIP 在文件头部，EOCD 离文件尾 100 KB 以上
  → `unzip` / Windows 自带解压 / 手机解压器**全都找不到**（探针反向断言它们必须失败，
  哪天它们突然能解，说明包其实还在尾部），只有 7-Zip 和本工具能解。
- **证据**：
  · 探针 `tests/l3_proto.py` **26/26**（已挂 `all.sh`）：产物结构字节级核对
    （ftyp 原样 / free 紧跟其后 / ZIP 正好起于 36 / `free.size = 8 + ZIP` / 盒子首尾相接）/
    `mdat.size` 未变 / ffmpeg rc=0 且 stderr 空 / `--list` 认得出 / 解出 sha256 一致 /
    7z 能解 / **unzip 必须解不开** / 图片外壳降级 / decoy 允许且噪声在末尾 / 两次打包一致 /
    协议 `head` 回 `HeadFreeAtom`、乱写回 `BAD_PARAM` / `--tail` 冲突 USAGE / L7 L8 未实现；
  · `cargo test` **399 + 31 + 2**（`mp4.rs` 新增 7 条 L3 单元测试）；`clippy -D warnings` 干净；
  · `flutter test` **187 条全过**、`flutter analyze` No issues；
  · 布局下拉与 7 语言文案（每语言 4 条新键，7 个语言包都是 502 键）。

### T18-1 第三项 · L7「一个文件里放两个包」

- **是什么**：封面 → ZIP1（本次输入压出来的）→ ZIP2（用户给的**现成** zip），两个包各自完整、
  都能单独解开。CLI `--layout L7 --second <现成zip>`，协议 `pack.layout:"two"` + `pack.second`，
  界面第五档「两个包」+ 一个「第二个包（现成的 zip）」选择行。
- **★ 这一项最要紧的是「默认解哪一个」**：`locate()` 找的是文件里**最后一个** EOCD，
  也就是**附加包** —— 用户打了包、双击解压，拿到的却是另一个内容（实测撞到）。
  修法在 `job::前一个包的起点`：ZIP 规范规定 EOCD 之后最多 65535 字节注释，而下一个包的
  起点紧挨着上一个包的尾部 → 前一个包的 EOCD 一定落在 `[base-64K, base)` 里，
  一次 64 KiB 的读就能反推出它的起点（`base = eocd - 目录大小 - 目录偏移`），
  **不用全文件扫描**。现在默认解主包，两处候选都报出来。
- **`--list` 也要说**：它列的是第一个包，但文件里还有第二处 —— 现在多打一行
  「ℹ 这个文件里还有另一处包（偏移 X）—— 想看那一处就加 --candidate X」（7 语言）。
- **代价（探针用反向断言钉死）**：7-Zip 只报**第一个**包、`unzip` / `zipfile` 只报**最后一个**，
  而且都不提示还有另一个。只有 7-Zip 扫描模式（`7z x -t#`）和本工具能拿全。
  「它们突然能报两处」＝ 实现坏了（包没接在一起），所以断言写的是「只见一个」。
- **三个不做**：
  · 第二个包不是 ZIP（文件头不是 `PK\x03\x04`）→ **USAGE(2) 且零产物**；
  · `--second` 配别的布局 → USAGE(2)；L7 不给 `--second` → USAGE(2)；
  · L7 + `--decoy` → 拒绝（噪声接在 ZIP2 后面会让它的目录区不在文件尾，
    连 unzip 也看不见第二个包）。
  ★ 这三条都放在 `File::create` **之前**：以前校验在引擎里，拒绝时会留一个 0 字节空文件
    在用户的下载目录里 —— 「拒绝了」就该干干净净什么都不留。
- **自检**（两条，都在回读）：
  · `self_check_first_of_two`：按**已知起点**精确定位第一个包（不能用 `locate()`，
    它会落到第二个包上，误报「ZIP 起点应为 90224，实际 110434」）；
  · `self_check_second`：第二个包起点是 `PK\x03\x04`、结束正好是文件尾、
    文件尾 64 KiB 内有 EOCD（通用解压器能找到它靠的就是这个）。
- **内存**：第二个包走 `拷贝带进度` 流式追加，4 MB 缓冲 —— 几百 MB 的现成 zip 不会读进内存。
- **证据**：
  · 探针 `tests/l7_proto.py` **22/22**（已挂 `all.sh`）：产物结构（ZIP1 起于封面末尾 /
    ZIP2 起于 ZIP1 末尾且到文件尾 / 两包不交叉）/ 默认解压 = 主包 sha256 一致 /
    `--candidate` 解第二处 sha256 一致 / `--list` 报出另一处 / **7z 只见第一个、unzip 只见第二个**
    （反向断言）/ 非 zip 拒绝且零产物 / 两条半配置 USAGE / L7+decoy 拒绝 /
    协议 `second_offset` = 封面 + ZIP1、`second_size` 对得上 / 两次打包逐字节一致；
  · `cargo test` 399 + 31 + **430**（新增长选项总数 53 的同步）；`clippy -D warnings` 干净；
  · `flutter test` 全过、`flutter analyze` No issues；7 个语言包都是 511 键。

### T18-1 第四项 · L8「MKV 附件轨」（T18-1 到此收尾）

- **是什么**：产物是个**真的 MKV** —— 借系统 `ffmpeg` 把封面视频**重封装**（`-c copy`，
  不转码），ZIP 原文塞进 MKV 的 `Attachments/AttachedFile/FileData`
  （`mimetype=application/zip`、`filename=payload.zip`）。CLI `--layout L8`，
  协议 `pack.layout:"mkv"`。配方与实测见 `refs/analysis/05-压缩布局验证.md` §3 L8。
- **为什么是「借 ffmpeg」**：自己写 MP4→MKV 是几千行的活，而 `cover_extract` 早就有
  `try_remux_with_ffmpeg` 的先例。没装 ffmpeg → **如实降级**（改 free 原子 / 尾部追加）
  并在结果里说「没装 ffmpeg」，**绝不写一个假 MKV**。
- **★ 三条关键规矩**（每条都有对应的探针断言）：
  · **判据是「解码 rc=0 **且** stderr 为空」** —— 封装返回 0 不代表能播；
    L3 那次「rc=0 但 AAC 全坏」就是只看返回码漏的。
  · **`-fflags +bitexact` 必须写在输出侧** —— 不写的话 mkv 复用器会写随机的
    `SegmentUID`/`TrackUID`，「同一份输入两次打包逐字节一致」直接挂。
    反向验证做过：把这组参数删掉 → 探针第 ⑩ 条立刻变红。
  · **后缀必须 `.mkv`** —— 内容是 MKV 却叫 `.mp4` 是骗人（播放器、网盘都按扩展名判）。
    `-o x.mp4` → USAGE(2) 且**零产物**；`-o 裸名` → 自动补 `.mkv`。
- **代价（探针反向断言钉死）**：`unzip` / Windows 资源管理器 / 手机解压器**全都找不到**
  （ZIP 的 EOCD 离文件尾 89,121 B，超出它们 64 KiB 的搜索窗）。7-Zip 能（`Offset=540`），
  本工具能。这条是 L8 的定位，必须写进界面。
- **三个不做**：L8 + `--decoy` → USAGE(2) 零产物（噪声会写坏 EBML 结构）；
  协议层同样拒；干扰数据在引擎侧再守一道（防绕开 CLI 直接调库）。
- **取回路径实测三条都通**：本工具 `--list` / 本工具解压（sha256 一致）/
  `ffmpeg -dump_attachment:t:0`（取出的就是原 ZIP，成员对得上）。
  ★ `-dump_attachment` 是**输入侧**选项，必须写在 `-i` 之前（写后面会报
  「you are trying to apply an input option to an output file」——踩过）。
- **实现要点**：`封装成mkv附件` 里先写封面到临时文件（ffmpeg 要路径），封完读回来核
  EBML 魔数 `1a 45 df a3`，再用「ZIP 前 32 字节指纹」在 MKV 里找附件起点
  （`locate()` 在这儿用不了：它只在文件尾 64 KiB 找 EOCD，而 L8 的 ZIP 在文件头）。
- **证据**：探针 `tests/l8_proto.py` **18/18**（已挂 `all.sh`，本机没 ffmpeg 时自动 skip）；
  `cargo test` 399 + 31 + 2、`clippy -D warnings` 干净、`cli_spec_match` 7/7；
  L3 26/26、L7 22/22 不动（回归）。
- **界面**：打包页布局下拉第六档「MKV 附件」—— 选它时「输出文件」的后缀自动变 `.mkv`
  （用户自己改过名字就不动他的），说明里写明「要装 ffmpeg、unzip 找不到包」。
  7 语言各 3 条新文案（`pack.layout.mkv.choice/desc/name`，语言包 514 键）。
  README 补了 ffmpeg 的依赖与许可证说明（用户对「用别人的必须明示来源」要求很硬）。
- **0.9.43 已发并装机**：`dist/yshjy-0.9.43` / `yshjy-full-0.9.43` tar.gz +
  `yshjy_0.9.43_amd64.deb`；`test-packages.sh` **37/37**；`sudo apt install` 后
  `yshjy --version` = 0.9.43、`/usr/lib/yshjy/gui/yshjy_gui` 时间戳 22:47（新界面）。
  **真人验收通过**：下拉六项齐（含 MKV 附件）、选中后说明文案正确、
  输出名自动从 `杨三海解压 体验包.mp4` 变成 `杨三海解压 体验包.mkv`。

## [0.9.42] · 2026-11-01 凌晨（T18-1 · 打包布局 L4「插在视频中间」）

- **是什么**：打包时把 ZIP **插进视频数据中间**（`mdat` 的两条 chunk 之间），视频照常能播
  —— DESIGN §15.1 矩阵里的 L4「✨ 技术亮点」。命令行 `--layout L4`，协议 `pack.layout: "mid"`，
  界面打包页那个「不改封面」开关改成三档下拉（free 原子 / 接在文件末尾 / 插在视频中间）。
- **为什么它难**（`refs/analysis/05-压缩布局验证.md` §4 的对照实验，2026-10-16 现造现测）：
  必须**同时**做三件事，缺一条视频就坏，而且坏法不一样 ——
  · 只插不改 → `ffmpeg rc=183` 打不开；
  · 只改 `mdat` size → **rc=0 但 stderr 满屏 `Decoding error`**（有坏帧）；
  · 只补 `stco` → rc=183（走不到 moov）；
  · 插在 chunk 内部 + 两边都改 → rc=0 但 `marker does not match f_code`（画面损坏）；
  · **插在 chunk 边界 + 两边都改 → rc=0 且 stderr 为空**（干净）。
  ★ 由此钉死一条验收铁律：**`ffmpeg rc=0` 不等于画面没坏**，探针判的是「rc==0 且 stderr 空」。
- **实现**：新模块 `src/pack/mp4.rs`（只做这一件事，`#[deny]` 不 panic：封面是用户随便挑的
  文件，坏结构只能返回错误）。插点从**所有轨 chunk 起点的并集**里取中位 —— 不解析 `stsz`/`stsc`
  的理由写在模块头（MP4 规范不允许 chunk 数据互相重叠，交错写入时一条 chunk 的终点就是下一条的
  起点；2026-10-31 用 stsz+stsc 把三个真实封面的每条 chunk 区间算出来核过：零重叠、插点跨越 0 条）。
- **不做的事（三条，都有测试守着）**：
  · 封面不是 MP4 → **降级**成「尾部追加」并说明（`warnings` + 界面结果卡红字）；
  · MP4 但解析不出 `stco`/`co64`（分片 MP4 用 `moof`+`trun`）→ 降级成 free 原子；
  · 包大到让 32 位 `stco` 偏移 / `mdat` size 溢出（>4 GiB）→ **直接拒绝**，
    不写一个 size 溢出、解不回来的包。
- **两条互斥**：L4 + `--decoy`（干扰数据）→ 拒绝 —— 噪声不管写在 ZIP 前面还是后面都落在视频
  数据中间，会把某条 chunk 切成两段；`--tail` 与 `--layout` 说不同的话 → USAGE(2)（`--tail`
  就是 `--layout L1` 的别名，两套优先级说不清谁赢）。
- **内存**：`准备插槽` 只返回「补好的封面 + 插点」，ZIP 走流式拷贝 —— 打 2 GB 的包不会吃 2 GB 内存。
- **代价（如实写进界面与文档）**：ZIP 的 EOCD 离文件尾远（后半段视频的长度），unzip /
  Windows 资源管理器 / 手机解压器只在文件尾 64 KiB 找 EOCD → **多半认不出**；
  7-Zip（从文件头扫）和本工具一定行。实测：90 KB 封面时尾部只有 44 KB → unzip 会警告但能解；
  498 KB 封面时 unzip 彻底找不到。所以它不是「更隐蔽的默认」，是**高级选项**。
- **证据**：
  · 探针 `tests/l4_proto.py` **19/19**（已挂 `all.sh`）：真打进真封面 → ffmpeg 干净 /
    `--list` 认得出且明说「包藏在文件中间」/ 解出 sha256 逐一相等 / 7z 也能找到 /
    图片外壳降级且能解 / L4+decoy 拒绝 / L5 L6 拒绝、L3 L7 L8 报未实现 / `--tail` 冲突 /
    协议 `layout=mid` 回 `MidChunk`、乱写回 `BAD_PARAM`、`tail:true`+`mid` 也被拦；
  · `cargo test` **392 + 31 + 2**（新增 mp4 模块 9 条）；`clippy -D warnings` 干净；
  · `flutter analyze` No issues、`flutter test` **188 过**（`pack_tail_widget_test` 升级成
    `pack_layout_widget_test` 7 条：默认传 free 绝不传 tail / 两档都真传 / 引擎降级要红字 / 图片外壳 mid 灰掉）；
  · `bash tests/all.sh` **52 通过 / 1 失败（cli_spec_match，补全没同步）→ 重跑生成器后 7/7** / 267 秒；
  · **真机证据（用户本人点过，2026-10-31 21:29）**：`screenshots/t18-1-布局三档.png`
    （下拉展开，三个选项都在：free 原子（推荐）/ 接在文件末尾 / 插在视频中间）、
    `screenshots/t18-1-选中插在中间.png`（选中后副标题换成「ZIP 夹在视频数据中间…」的代价说明）；
    状态栏「引擎 0.9.42」、**无溢出、无报错**。

## [0.9.41] · 2026-10-31 深夜（T18-2 自定义显示名）

- **是什么**：打包时能自己起产物名 —— 伪装成 `电影.2026.1080p.mp4` 这种（DESIGN §14.2
  最后一项「自定义显示名：用户填」）。命令行 `--name <名称>`，界面「输出文件」那格直接编辑。
- **只改名字不改位置**：目录仍由 `-o/--dest`（界面右边那个按钮）决定。两个都给、且 `-o`
  指向的是完整文件名 → USAGE(2) 明确拒绝，不猜用户想听哪个。
- **后缀怎么定（三条规则，判据一律是「内容与后缀必须一致」）**：
  · **先读文件头魔数**：`--cover` / 界面选的外壳是什么类型就认什么（PNG 存成 `封面.dat` 也认得出）。
    布局判断本来就按魔数做（`pack::build` 里那句「封面是 mp4」），这里把同一份判断扩成
    「顺便给出该用的后缀」，CLI 与界面共用（`pack::外壳类型` / `shellExtFromHead`）；
  · 魔数认不出（纯文本外壳之类）才退回文件名扩展名；两者都没有 → 内置视频素材 `.mp4`；
  · 普通压缩 / 自解压 / 带外壳的伪装：后缀必须与模式（或外壳）一致，不一致直接 USAGE ——
    不做「名字是 .zip、内容却是 tar.gz」「名字是 .mp4、封面却是 PNG」的骗人包；
    没带外壳的伪装只认常见外壳后缀，`电影.2026.1080p` 里的 `1080p` **不是扩展名** → 补 `.mp4`。
- **名字校验**（引擎 `pack::校验显示名` + 界面 `nameError` 同一套规则）：空、首尾空格、末尾点、
  `/` `\`、`.` `..`、控制字符、Windows 非法字符 `< > : " | ? *`、Windows 保留设备名
  （CON / COM1 / LPT9…）、超过 200 字节，全部明确报错。判据是「这名字在 Windows 上会不会
  出问题」—— 包是要发给别人的。
- **顺手修掉的老 bug**：`-o <目录>` 对打包**从来不管用**（路径被原样当文件名 →「建不了输出文件：
  Is a directory」），而 CLI-SPEC 一直写着「输出目录」。现在 `-o 目录` 落 `目录/输入名.mp4`。
- **做界面时撞出的两个坑**（widget 测试抓到的，光看代码发现不了）：
  · `_out` 的 setter 会用 `baseName` 回写输入框 → 用户打 `a/b.mp4` 时违规的斜杠被抹成
    `b.mp4`，校验形同虚设、输入框还自己跳字。改成「输入框文本归用户，校验原文」；
  · `.` / `..` 的判定必须排在「末尾不能是点」前面，否则报错理由不对
    （`nameError('.')` 会返回 NAME_SPACE 而不是 NAME_DOT）。
- **证据**：
  · 探针 `tests/name_cli.py` **16/16**（已挂 `all.sh`）：老行为不变 / 名字生效且 `--list`
    认得出 / `1080p` 不被吃掉 / 补 `.zip` / 后缀不一致被拒 / `../evil.mp4` 零产物 / 空名 /
    `-o 目录 + --name` / **`-o 目录` 单独用（老 bug）** / `-o 文件 + --name` 冲突 /
    `--name` 的值不被当成输入文件 / **`--cover 图.png` 后缀跟着外壳走** / 后缀与外壳冲突被拒 /
    `-o 目录 + --cover` 默认名也跟外壳 / **`封面.dat` 内容是 PNG → 补 `.png`（按魔数认）**；
  · `cargo test` **383 + 31 + 2** 全绿（新增 5 条显示名校验 + 1 条外壳扩展名白名单 + 1 条魔数识别）；
  · `flutter analyze` 干净、`flutter test` **181 过**（新增 `pack_naming_test` 12 条 +
    `pack_name_widget_test` 4 条）；
  · `bash tests/all.sh` **52 通过 / 0 失败 / 5 跳过 / 240 秒**；
  · `cli_spec_match.sh` 7/7（对外长选项 49 个，`--name` 已进三份补全 + man 页）；
  · **真机（用户本人点 + 截图）**：`screenshots/t18-2-输出框可编辑.png`（输出名成了可编辑输入框，
    小字提示在位）/ `t18-2-改名电影2026.png`（改成 `电影.2026.1080p`，无红字、输入框不乱跳）/
    `t18-2-后缀跟外壳.png`（切伪装 + 选 png 外壳后自动变 `电影.2026.1080p.png`）/
    `t18-2-结果页.png`（打包成功：1.2 MB / 21 条目 / `/home/…/下载/电影.2026.1080p.png`）。
    产物 `yshjy --list` 实测认得出（zip，偏移 95451 = 外壳大小，21 条目）—— 名字对了、也真能解。
- **契约 golden 的两处修正**（不是本批功能，是回归网之前就红的）：
  · `pack` 的 golden 里 `events` 是空的、`rc: 1` —— 上一轮是在**打包失败**时录的基准，
    等于没守任何东西；本批刷成真实成功态，`PackProgress` 的形状现在被钉住；
  · `caps` 的 `tar.can_create` 随 T17-4 变 `true`，golden 落后一版。
  两处 `--update` 之后重跑契约 **12/12 符合**。
- **下一步**：T18-1 打包布局（L3 → L7 → L8 → L4，最重的一块，按 `docs/TODO.md` 批 3）。

### T18-3 防炸链（同批）

> 提交：`d2f6c13`（引擎+协议+CLI+探针）、`2383a4a`（界面+文案+测试）。

- **是什么**：网盘按内容哈希判重，同一份东西反复上传会秒传 / 炸链。开这个开关后
  每个条目的时间戳被打乱，整包字节变 → 哈希变 → 网盘当成新文件。**默认关**
  （DESIGN §14.2）。CLI `--tweak-time` / 协议 `pack.tweak_time` / 界面开关三处同一个行为。
- **实现**：按「条目路径 + 本次种子」派生时间（不引 rand），落在 **[5 年前, 现在]** ——
  **绝不出现 1970 / 1980**（用户明确讨厌那种假时间）。ZIP 改条目选项；tar 得手动造 header
  （`append_path_with_name` 会自己读文件 mtime，改不动）。
- **刻意保留的约定**：**不开时两次打包仍逐字节一致**；开了才每次不同 —— 这正是防炸链的目的。
- **证据**：
  · 探针 `tests/tweak_time_proto.py` 9 项全过（已挂 all.sh）：默认一致 / 开了不同（zip **和** tar.gz 都得改）/
    年份 2021–2024 合理 / 协议层同样生效；
  · `cargo test` 377+31+2 全绿、`cli_spec_match` 7/7、`flutter analyze` 干净、`flutter test` 162+2 过；
  · 真机（用户本人点 + 截图）：`screenshots/t18-3-tweak-off.png`（默认关，说明小字对）/
    `screenshots/t18-3-tweak-on.png`（点开变蓝）。

## [0.9.40] · 2026-10-31 夜（T17-4 打包格式可选 + T18-3 防炸链）

> 引擎 / 协议 / CLI / 界面四层都做完了，**还差真机截图**（要用户自己点，项目规矩）。
> 提交：`a593942`（引擎+协议+CLI+探针）、`a9184eb`（界面+文档）。

### 探针先行的结论（设计文档要求「验过再承诺」的那条）

- 解压侧 **tar / tar.gz / tar.zst 本来就能解**：造三种包真解压，md5 与源一致。
- 但「MP4 封面 + tar 系」的伪装包 `--list` **三种全报 NOT_ARCHIVE** —— 定位器只认 ZIP 的 EOCD。
  所以 **tar 系只能走普通压缩**，伪装模式在协议层直接拒（不是等用户撞上才说）。

### 引擎

- `PackFormat`（Zip / TarGz / TarZst）+ `PackOptions.format`；`write_tar` 用 tar crate，
  外面套 flate2(gzip) / zstd。目录**显式加条目**（空目录不丢）、`read_dir` **排序**
  （两次打包逐字节一致）、mtime 由 tar 头自带（ZIP 那边要手填）。
- 依赖新增 `zstd = 0.13`（`ruzstd` 只会解压）；zstd-sys vendored 编译，46 秒。
- `self_check_tar`：tar 头就是索引，读回来数非目录条目。CLI 自检按格式分派 ——
  以前只认 ZIP 的 EOCD，tar 包会被自检误报「❌ 自检失败」（包本身是好的）。
- 顺手修：「ZIP 起点为 0，封面没包住数据」这条警告只在真声称有封面时才报
  （普通压缩 zip_offset 就是 0，以前会误报）。
- caps 里 tar 的 `can_create` 改 true（只出 tar.gz / tar.zst，不出裸 .tar）。

### 协议 / CLI

- `pack.format`；tar 系拒绝 `password` / `split` / `sfx` / `decoy`，各回 BAD_PARAM + 人话理由。
- 结果新增 `format` 回显。
- CLI `--format`：一给就是普通压缩（不加外壳、不写说明书），默认输出名跟格式走。
- 帮助 / man / 三份补全全部跟着生成器更新（长选项 49 → **50**）。
- 文案：`pack_done_archive` / `pack_selfcheck_ok_archive` × 7 语言 + `--format` 帮助行 × 7 语言。

### 界面

- 打包页「普通压缩」下新增「打包格式」下拉；**伪装模式不显示**（伪装只能是 zip）。
- 选 tar 系：密码框置灰 + 「tar 格式不支持加密」、分卷开关禁用、输出名换后缀、
  请求里**不带** password / split（用户在 zip 下填过的不能偷偷带过去）。
- l10n 新增 7 条 × 7 语言（485 键，各语言齐）。

### 证据（都是真跑的）

- `cargo test` 全绿 / `cargo clippy --all-targets -D warnings` 干净
- `flutter analyze` No issues / `flutter test` **162 过**（原 158 + 新 4 条）
- `tests/format_proto.py` **20 项全过**（已挂 `tests/all.sh`）：真打真解（Python tarfile + 系统 tar --zstd）、
  矩阵拒绝 5 组、老行为不变
- `bash tests/cli_spec_match.sh` 7/7
- 真机 CLI：三种格式打包 + 三种冲突报错，逐条贴过产物校验
### 发版记录

- 产物：`dist/yshjy_0.9.40_amd64.deb`（11M）、`yshjy-0.9.40-linux-x86_64.tar.gz`、`yshjy-full-0.9.40-…tar.gz`
- `scripts/test-packages.sh`：**37 项全过**（权限 / prerm 区分 upgrade / deb 内二进制版本一致 /
  真打包真解压逐字节一致 / tar.gz sha256）
- 已装机：`dpkg -l yshjy` = **0.9.40**；CLI、GUI（19:33 的 libapp.so）、man、三种补全、14 种格式菜单项一起换新
- **真机确认（2026-10-31 19:4x，用户从应用菜单打开装机版）**：「打包格式」下拉与「防网盘炸链」开关两处都在 —— 发版这版就是新版，不是开发包
- 版本号 7 处同步：`Cargo.toml` / `ffi/Cargo.toml` / `gui/pubspec.yaml` / `README.md`×2 / `docs/PROTOCOL.md`×2

- **真机验收（用户本人点 + 截图，2026-10-31 19:15–19:18）**：
  · `screenshots/t17-4-format-zip.png` —— 普通压缩默认：格式下拉在、值 zip、解压码框可用；
  · `screenshots/t17-4-format-tarzst.png` —— 选 tar.zst：解压码框置灰 + 小字「tar 格式不支持加密」，
    「分成多个卷」「防网盘扫描」「不改封面」三行开关全暗（禁用），只有「生成 SHA-256 校验和」还是亮的（该留的留）；
  · `screenshots/t17-4-format-targz.png` —— 选 tar.gz：提示行变「tar 不支持加密、分卷、自解压和伪装 —— 这几项已经自动关掉」

## [0.9.39] · 2026-10-31（P1 三条小活 + 界面多语言 T14：七种语言全部到位）

> 0.9.38 已出包装机，这一批从 0.9.39 起算；**做完 T14 再统一出包**。

### T14 界面多语言 · 第一批（地基 + 关于页）与第二批（错误码文案）

- **方案**：不引 `flutter_localizations` / `intl`。文案放 `gui/assets/l10n/<code>.json`，
  **键名与占位符跟引擎 `locales/*.toml` 同一套规则**（`段.键` + `{name}`），贡献文档讲一次就够。
  代价是缺 key 不会编译期报错，用 `gui/test/l10n_test.dart` 的全量比对补回来。
- 语言选择：默认**跟随系统**（`Platform.localeName`），中文按地区分繁简（TW/HK/MO/Hant → 繁体），
  其它语言按主语言配对，认不出的退回中文；`[general] ui_language` 可以钉死一种。
- 第一批接的是「关于」页（样板页：纯文本 / 单占位符 / 带子句三种形态都跑过），
  真机由用户自己点开验收 —— 英文页正常。
- 第二批把错误码文案搬进语言包：`err.*` / `diag.*` / `item.*` / `batch.*` 共 44 条，
  `errorText` / `diagText` / `itemResultText` / `batchOutcomeText` 改成查表。
  **查不到仍回退通用人话**（D21：不把码名甩给用户），这条规矩没变。
- 新增 `gui/test/flutter_test_config.dart`：所有测试统一装基准语言表。
  没有它，widget 测试里表是空的 → 界面显示 `err.NEED_PASSWORD` 这种 key → 断言人话的测试假红。
- **证据**：`flutter test` 148 过（原 131 + l10n 16 + humanize 1）；`flutter analyze` 干净；
  l10n 的护栏含「各语种缺/多/空一条 → 红」「译文丢占位符 → 红」。
### T14 界面多语言 · 第三批（设置页 68 条 + 界面语言下拉）

- 设置页文案全部搬进 `settings.*`（68 条）；`_kv` 里那句「界面语言：目前只有中文」是过期说明，删掉；
  「还没做的」那段从「界面多语言切换（401 处硬编码）」改成实话：关于页+错误提示已完成、设置页在做、
  打包页/首页还在后面。
- 新增**界面语言下拉**（设置 → 外观）：选完**立刻生效**（不用点保存），保存只负责记住选择，
  写进 `[general] ui_language`。认不出的值（手改成 `klingon`）当「跟随系统」。
  下拉里每种语言用**它自己语言里的名字**（简体中文 / English / 日本語…）—— 切成看不懂的语言时，
  用户至少还认得出哪个能切回中文。
- **证据**：`flutter test` **151 过**（原 148 + 本批 3）、`flutter analyze` No issues；
  三条测试钉「读回 en-US」「坏值回落」「保存时同段注释与其它键一个字节都不被冲掉」。
- **踩到的坑（已写进项目技能）**：测试体里 `await File(...).writeAsString(...)` 在
  `testWidgets` 的假时钟下永不完成 → 三条测试齐挂 10 分钟超时，报错里没有 Expected/Actual，
  看起来像界面卡死。夹具一律用同步 API（`writeAsStringSync`），点保存这类真写盘操作包进 `runAsync`。
- **真机验收抓到的 bug（用户截图）**：「遇到同名文件 / 解压完原包」两个下拉原本并排，英文
  `Rename automatically (recommended)` 在 576px 真机窗口里把右边挤出去 40px，Flutter 画出黄黑
  `RIGHT OVERFLOWED BY 40 PIXELS` 竖条。改成上下排列 + `isExpanded: true`。
  补一条 576 宽渲染整页的回归 —— **注意 `ListView` 懒加载，必须滚到那两行再断言**：
  不滚动时 320 宽都「通过」，第一版就是条假护栏（改窄到 120 才红，证明机制有效后定稿 576）。
- **Windows 未实测**：系统语言识别在 Windows 上只做了代码层兼容（`zh_CN` 下划线格式、
  繁简按地区），等 Windows 打包那轮在那边实测；细节见 `docs/TODO.md` 的 T14 明细。

### T14 界面多语言 · 第四批（打包页 77 条）

- 打包页整页文案搬进 `pack.*`（77 条）：三种模式、伪装文件说明、输出位置、解压码、压缩级别、
  布局、防扫描、分卷、校验和、说明.txt、进度、三种结果卡片。
- 带插值的 8 条改成占位符：`{reason}` / `{size}` / `{entries}` / `{layout}` / `{cover}` / `{path}` / `{secs}`。
  结果卡片原本用 `? :` 拼三段长文案，现在三种结果各一条完整译文 —— 翻译的人能看懂整句，
  不用对着拼接碎片猜。
- **踩到的坑**：① 局部变量 `final t = ...` 会盖住 `t()`（Dart 局部优先）→ 改名 `text`
  ② 打包页有 `segments: const [...]` / `items: const [...]`，列表里一调 `t()` 就编译失败，
  必须去掉列表上的 `const` 再给内部 `Icon`/`TextStyle` 补回 const。
- **证据**：`flutter test` 154 过、`flutter analyze` No issues。

### T14 界面多语言 · 第五批（外壳导航 + 历史页 24 条）

- 外壳的导航五项（解压/打包/历史/设置/关于）、状态栏（空闲/就绪/正在解压/正在打包/正在处理…）、
  首次上手三步提示、托盘菜单（崩溃日志 / 状态目录 / 引擎版本）全进 `shell.*`。
- 历史页 5 条进 `history.*`（标题 / 条数 / 清空 / 只存本机 / 还没处理过文件）。
- 品牌名「杨三海解压」**不翻译**（`shell.appName` 中英同值）—— 产品名翻过去没人认得。
- **证据**：`flutter test` 154 过、`flutter analyze` No issues。

### T14 界面多语言 · 第六~九批（首页 264 条 + 零散 10 条，中文搬运收官）

- 首页是最大的一块：**264 条**（状态提示 / 错误与炸弹警告 / 高级选项 / 30 种格式名 / 结果页 /
  拖放区 / 加密提示 / 内容树 / 封面提取 / 磁盘不够 / 分卷不全 / 诊断包 / 解压码面板 / 三步进度 /
  检出落盘 / 时长速度），分四批搬完。
- **八处源码里分两段拼的句子合并成整句 + 占位符**：限额提示、批量统计、伪装封面、高级按钮、
  视频已分离、候选提示、检出落盘提示、分享文案。原本是 `'…'  '…'` 相邻字面量拼接，
  翻译者拿到碎片只能瞎猜语序；现在一条完整译文，`{name}` / `{pw}` / `{n}` 这类占位符标清位置。
- 首页之外还清了 10 条：`engine.dart` 的偏移描述（`从开头算 X MB 处`）和三档可信度、
  引擎退出/超时提示、`entry_tree` 面包屑「全部」、设置页选目录按钮「就用这个目录」。
- **有意不翻译的四处**（写进 TODO 定论）：`localeNames` 的语言名（每种语言用自己的写法）、
  `platform_paths` 的 `$HOME/下载` 系统目录、`notify` 的品牌名、`engine_transport` 里给开发者看的
  `StateError` 文本。
- **验收方式**：正则扫全部 `lib/**/*.dart` 的字符串字面量，确认**没有任何用户可见的中文残留**，
  比人工翻代码可靠。
- **踩到的坑**：① `analyze` 报的列号与源码列不一致，脚本自动补 `const` 会把 `child:` 写成
  `cconst hild:` —— **const 只自动删、不自动插**，该补的手工补；② 把 `'${expr}'` 解成 `expr`
  时，int 值也一起被扒了壳，`Map<String,String>` 立刻报 9 处类型错 —— 给 int 值包回 `'${}'`；
  ③ 多行拼接替换后相邻 `t()` 之间缺逗号，一批 7 处编译错误。
- **证据**：`flutter test` 155 过、`flutter analyze` No issues。语言包至此 **479 条**，中英双份齐全。

### T14 界面多语言 · 第十批（七种语言全部到位，T14 完成）

- 新增 5 个语言包各 **479 条**：`zh-TW` / `ja-JP` / `ko-KR` / `es-ES` / `ru-RU`，
  键名与占位符与中文版逐条对齐（`l10n_test` 全量比对兜底）。
- 繁体：`opencc -c s2twp`（台湾正体 + 词汇）转完，再按引擎 `locales/zh-TW.toml` 对齐术语
  （壓縮檔 / 解壓密碼 / 資料夾 / 網路 / 預設 / 儲存），品牌名写繁体「楊三海解壓」。
- 日韩西俄：DeepSeek 分批翻（每批 50 条，带英文对照 + 语气要求 + 占位符规则），
  共 12 万 token。**翻译环节抓到的三个问题**：① 日语第一批把 49 条直接抄了英文
  （高级选项、格式名等），整批重翻；② 格式名这类有固定译法的用**确定性映射**
  （video→動画、image→画像、archive→アーカイブ），比让模型自由发挥稳；
  ③ DeepSeek 用 `response_format=json_object` 时 prompt 里必须出现 JSON 字样，否则 400。
- `availableLocales` 从 2 种扩到 7 种；`settings.language.help` 从「其余五种还在翻」
  改成实话「支持 7 种，选完立刻生效」。
- **测试**：原「ja-JP 没翻完 → 回落」用例换成不存在的 `xx-XX`（机制不变，例子换掉）；
  新增「七种名字都在下拉里」—— 必须 `skipOffstage: false`，默认 `true` 时下拉选项是
  offstage 的、一个都找不到，那条测试就是假绿。
- **证据**：`flutter test` 155 过、`flutter analyze` No issues；5 个语言包逐条校验
  （键集合 / 占位符 / 非字符串值 / 空值 / 抄英文 / 与中文逐字同）全部通过。

### P1-1 崩溃日志脱敏：实测早就做了，补的是护栏

- 待办原话是「崩溃日志**原样复制**进包、不经脱敏器」—— **实测不成立**：
  `v0.9` 第 11 批（`fa9be88`）起就有 `--no-paths` 下过一遍 `抹掉家目录`。
  造一份带家目录的崩溃日志跑 `--report --no-paths`，包里是 `<home>`、**零用户名**。
- 真缺口是**一条测试都没有**：注释写着「已脱敏」，谁改回原样都不会有人发现。
  补 `崩溃日志测试` 3 条（脱敏后无家目录且定位信息还在 / 不带 `--no-paths` 时原样 /
  只挑最新 `crash-*.log` 并跳过目录里的杂物）；顺手把 `最近崩溃日志` 拆出可注入目录的
  `最近崩溃日志_在`，测试不去碰用户真实状态目录。
- 把「进包前整理」抽成 `打包用崩溃日志(内容, hide_paths)`，行为与之前**完全一致**。
- **证据**：反向验证把那个 `if` 短路成恒假 → 当场 `panicked: 崩溃日志泄漏了家目录`，
  改回复跑回绿；`cargo test` 377 过 0 挂 1 忽略、`clippy -D warnings` 干净。

### P1-2 补 E2 / E6 的协议层探针（以前只有 widget 测试守着）

- `tests/tail_proto.py`（15 断言）：不传 `tail` / `tail:true` / 显式 `false` 三档，
  用产物字节证明 —— 尾部模式的 ZIP 头**紧贴封面末尾**（偏移 == 封面大小 1688），
  默认与显式 `false` 都在封面之后（留了 free 盒）。三份产物都必须能按 ZIP 解开、
  内容与原文件一致（正面对照组，防「为了紧贴把布局写坏」）。
- `tests/report_outdir_proto.py`（9 断言）：给 `out_dir` 就落那儿且 cwd 干净、
  `result.path` 指回同一个真实文件；不给就落进程 cwd（老行为，界面靠传参绕开）；
  给一个**不存在**的目录必须报错、且不偷偷在别处生成。
- 两个探针都把 `XDG_CONFIG_HOME` / `XDG_STATE_HOME` 指到临时目录，绝不碰用户真实配置。
- 已挂进 `tests/all.sh`（只放 `tests/` 里没人调用就是死文件）。
- **写探针时踩到的坑**：响应里路径在 `result.path`，我按另一条通道猜成 `data` 读到了空对象，
  差点误报成「引擎没回路径」的真 bug —— 先打一份原始 JSON 再改断言。

### P1-3 中文 Windows 的「下载」目录

- `downloadsDir` 的 Windows 分支以前只试 `%USERPROFILE%\Downloads`；中文 Windows 里
  用户目录下真的叫**「下载」**（Known Folder 的本地化显示名）→ 必然落空 → 退回**家目录**，
  诊断包混在家目录里，用户还是难找。
- 候选改成先「下载」再 `Downloads`，并抽成纯函数 `downloadDirCandidates(isWindows, home:)`
  （getter 读的是真机路径，测试机上验不出 Windows 分支）。
- **证据**：新 `gui/test/platform_paths_test.dart` 4 条（候选顺序 / 反斜杠 / Linux 正斜杠 /
  真机兜底目录一定存在）。

## [0.9.38] · 2026-10-31（T7：把「引擎有、界面没入口」逐项接上）

**要解决的事**：一批能力引擎和协议早就有，界面**一个参数都没传** —— 用户只能命令行用，
在图形界面里等于不存在。这批（`docs/TODO.md` 的 T7）按「真人会撞上的概率」排，
一项一个提交、每项真机点 + 产物复核。本轮做掉 5 项里的前 2 项。

### E4「只检查」—— 完整解码 + 校验，一个字节都不落盘（`a63f2c9`）

- 预览页底部加「只检查」（次级按钮，主入口仍是「开始解压」）。
- 请求里**强制** `cover:false`、不给 `dest`、不给 `select`：切封面是另一条真写盘的路径，
  留着它「不写任何文件」就是假话；校验的是整包，不跟界面勾选走。
- 结果页专属文案「校验通过：N 个文件都完好 / 没写任何文件，包还在原地」，
  并**隐藏** `dest`、`打开目录`、「接下来点这个」—— 引擎在 `verify_only` 下照样回
  `done=2 / bytes=13440 / dest=…`（实测 CLI `--test`），照普通成功页渲染就是又一个假成功。
- 校验完不写历史、不自动开目录、不探嵌套包。
- 顺手补 `docs/PROTOCOL.md` 的 `verify_only`（extract + batch）：实现一直就有，文档没写。
- **证据**：新测试 3 条（发送参数 / 反向验证不许带 / 结果页文案与隐藏项）；
  反向验证把 `if (verifyOnly)` 改成 `if (false && …)` → 当场红，改回即绿。
  真机截图 `screenshots/t7-e4-verify-only-{preview,done}.png`；
  **点击前后目录都是 82259 字节、零新增**，没有解压目录、封面无副本。

### E1「防网盘扫描（撒假数据）」（`6df4be1`）

- 打包页加总开关：撒 **4 段 × 16 KB**（引擎默认，约多 64 KB），协议 `decoy` /
  `decoy_signatures`。
- **普通压缩 / 自解压模式下禁用并写明原因**：引擎里 `decoy` 与 `mode:"plain"`、`sfx`
  互斥（回 `BAD_PARAM`）—— 不做「点了才报错」的假开关。
- 子开关「连文件指纹也伪造（更保险）」= `decoy_signatures`；**关总开关时一起清掉**
  （它只在总开关开着时才渲染，留着 `true` 就是用户看不见的地方还在撒假特征码）。
- 段数 / 每段大小**不给**界面选项：引擎默认已调好，命令行有 `--decoy` / `--decoy-size`。
- **证据**：新测试 6 条（4 条状态机 + 接线 2 条，含「不勾就不许带 decoy」的反向验证）；
  反向验证把 `decoy` 改成永远 `null` → 当场红（Expected 4 / Actual null）。
  真机截图 `screenshots/t7-e1-pack-decoy-{off,on}.png`；
  命令行用同一组参数打包输出「🎭 干扰数据：4 段，共 64 KB（每段 16 KB，假签名：开）」，
  产物 5981 → 71657 字节、自检通过。

### E5「智能解压」—— 不重复套文件夹（`a46ced6`）

这一项的缺口**不在界面，在协议**：命令行 `--no-smart-dir` 一直有，但协议 `extract`
把 `smart_dir` 写死成 `true`（只有 `batch` 有这个字段）。界面只走协议，所以图形界面里
根本表达不了 —— 预览页那行「会解到「内容/」里（包里本来就有这个目录，不会多套一层）」
说了很久，用户却没地方改。

- 协议：`extract` 解析 `smart_dir`（不传 = `true`，与老行为完全一致；`false` = **永远**
  多套一层「包名」文件夹）。`PROTOCOL.md` 补字段，并写明**只有不给 `dest` 才有区别**。
- 界面：「高级选项」加开关「智能解压（不重复套文件夹）」，默认开；选了「输出到指定目录」
  时**禁用并写明不生效**（引擎给了 `dest` 就两种模式都直接铺进去）—— 不做点了没反应的开关。
- **证据**：新探针 `tests/smart_dir_proto.py` 用真进程跑三种情况，实测解出来的树分别是
  `顶层/甲.txt`（默认）、`样本/顶层/甲.txt`（`false`）、`顶层/甲.txt`（显式 `true`）→ 3/3；
  已挂进 `tests/all.sh`（不挂就是死文件）。widget 测试 3 条：默认开 / 关掉真传 `false` /
  选了目录就禁用。反向验证：界面传参改成恒 `null` → 第 2 条当场红（Expected false / Actual null）。
  真机 `screenshots/t7-e5-smart-dir-{on,off}.png`。
- **⚠️ 这一批**动了引擎 `src/api.rs`。虽然 T7 剩下的 E2/E6 是纯界面，但**下次出包必须重编
  引擎并升版本号**，别再按「纯界面小修」处理。

### E2「不改封面，接在文件末尾」（`38bb58a`）

协议 `pack.tail` 引擎早实现（→ `PackLayout::TailAppend`），界面却只**显示**结果、
从不传 —— 想强制尾部追加只能在命令行加 `--tail`。

- 打包页加开关「不改封面，接在文件末尾（更保险）」，**默认关、请求里不出现 `tail` 键**：
  默认传 `true` 会把所有 MP4 伪装包都改成尾部追加，视频反而播不了（本项最大的坑）；
- 普通压缩 / 自解压没有封面 → 禁用并说明；
- **实测同一封面两条路真的不同**：自动模式 ZIP 头在第 2569 字节（封面 2319 之后多插了
  250 字节的 free 盒），尾部模式在第 2319 字节（紧贴封面末尾），封面原有字节两者都没坏；
- 测试 4 条，含「不勾时请求里不许出现 `tail`」这条反向保护；反向验证：改成恒 false → 红。

### E6「诊断包存到哪」（`38bb58a`）

协议 `report.out_dir` 一直有，界面只传 `path` —— 引擎不传就落在**进程当前目录**
（GUI 从哪启动就落在哪，可能是 `/` 或 `~`）。用户点完看到「诊断包已生成」，
回头满电脑找不到文件，比报错还气人。

- `platform_paths.downloadsDir`：中文系统「下载」/ 其它 `Downloads` / 兜底家目录；
- 生成诊断包一律传 `out_dir`，提示改成「诊断包已存到「~/下载」：xxx.zip」并带
  「打开文件夹」；
- **真机**：文件管理器真弹到「主文件夹 / 下载」，那个 zip 就在第一个格子里。

### fix(report)：诊断包 `findings.error` 里的路径没脱敏（`023017c`）

**E6 真机验收撞出来的真 bug。** 界面写着「路径已脱敏」，`target` 也确实换成了
`<path-2>`，但同一份 JSON 里的 `error` 字段是引擎错误原文，原样带出了
`/home/用户名/…/坏文件.bin`。这个包正是要贴群里、发给别人报障的 ——
等于把用户名和目录结构一起发出去。

`脱敏器` 新增 `文本()`（先按映射表替换、家目录兜底），`findings.error` 走它。
3 条单元测试盯着：家目录要脱掉 / 同一路径跨字段占位符一致 /
**关掉脱敏时要原样保留**（别帮倒忙）。修复前 grep 用户名命中 1 处，修复后 0 处。

> 剩下的疑点记在 `docs/TODO.md` 的「下一轮待议」：诊断包里的 `crash/*.log` 是原样
> 复制的，不经脱敏器 —— 要不要在 `--no-paths` 时也过一遍，得先定「脱到什么程度
> 还够定位问题」。

### 顺带修：打包页 RenderFlex 溢出（真机才看得见）

加完上面两行开关后，打包页在 850×736 默认窗口下报 `BOTTOM OVERFLOWED BY 20 PIXELS`
（用户在截图里撞到的黄黑条）。根因是 `Column` 里的 `Expanded` 列表吃剩余空间，
固定内容一长就挤爆。改成整页 `SingleChildScrollView` + 列表给受控高度
（窗口 30%，夹在 140–360），窗口再矮也只是要滚、不会再挤爆。

**没出包**：这一批只是界面改动，引擎 0.9.37 一行未动；按用户口径「小修攒一批再出」。

## [0.9.37] · 2026-10-31（T16：命令行 `--candidate` —— 「解第 N 处」现在命令行也能表达）

**要解决的事**：S10 多候选在 0.9.36 做完了，界面能「换一处解压」、协议能 `extract.candidate`，
但**命令行**没法指定 —— 用户拿 `yshjy` 复核产物时办不到，只能写 `--serve` 手工拼 JSON。
这笔小账记在 `docs/TODO.md` 表尾（T16）。

- **选项**：`--candidate <字节偏移>`（与协议 `extract.candidate` 同名同义），登记进
  `cli_spec.rs` 的 `OPTS` 表 → `--help` / man 页 / 三份 shell 补全**全部自动跟上**
  （对外长选项 → 46，另有 3 个内部选项，共 49）。
- **三条「不许猜」**：
  ① 值不是数字 → **USAGE(2)**，提示里点名这是选项的值（不许当成没给、静默跑默认那处）；
  ② 偏移对不上任何一段 → **NOT_ARCHIVE(3)** 且**零产物**（绝不偷偷解默认那处 ——
     那等于用户点了第 2 处、程序解了第 1 处，是假成功）；
  ③ 一次给两个文件 → **USAGE(2)**（一个偏移不可能同时对上两个包），提示语明说
     「它说的是这一个文件里的第几处」。检查放在「排好 N 个任务」之前：先拒绝，再谈排队。
- **证据**：新探针 `tests/candidate_cli.py` **6/6** —— 不给参数仍解默认那处 · `--candidate`
  真的换到另一段（第B处 → 第A处）· 非法偏移被拒且零产物 · 非数字报 USAGE · 多文件报 USAGE ·
  偏移没被当成待解压的文件名。**已挂进 `tests/all.sh`**；顺手把一直没挂的
  `tests/t5_candidates.py`（8/8）也挂上（以前只手工跑过，等于日常回归没覆盖 T5）。
- **顺手清的两个界面测试小账**（跑全量回归撞出来的，不是本轮目标）：
  `settings_switch_widget_test.dart` 的保险丝判「路径里必须含 `yshjy-test`」，而 `all.sh`
  用的是 `yshjy-all-cfg` → 整组 `setUpAll` 直接炸（`flutter test` 一直是红的）。根子是
  **同一条规则手抄了两份**（`app_settings_test.dart` 判的是「是不是用户真实配置目录」）。
  现已收进 `gui/test/config_guard.dart` 一份实现、两个测试都调它；**反向验证过**：不设
  `XDG_CONFIG_HOME` 时两个测试都被拦，用户真实 `~/.config/yshjy/config.toml` 一个字节没动。
  另清掉两条既有 analyze 噪音（死代码 `?? false`、多余的字符串花括号）→ `flutter analyze`
  现在 **No issues found**。
  ★ 坑：**Dart 不认中文标识符**（第 4 次踩）—— helper 里写了 `final 真实配置`，analyze 报一串 illegal character。
- **回归**：`cargo test` **404 过 / 0 挂**（371 + 31 + 2）、`clippy --all-targets -D warnings` 干净、
  `flutter analyze` 干净、`flutter test` **109 过**、`bash tests/all.sh` **46 通过 / 0 失败 / 5 跳过 / 210 秒**。
- **发版**：0.9.37 已出包并装机（见 `docs/RELEASE-CHECK.md` 顶部记录）。

## [0.9.36] · 2026-10-31（T5 / S10 多候选：一个文件里好几处藏着东西，现在能换一处解；自动找解压码改为默认关）

### 自动找解压码改为**默认关闭**（用户 2026-10-31 口径）

- 引擎 `[password] auto_from_notes` / `auto_from_book`：默认 `true` → **`false`**；
  设置页两个开关默认关、两段说明文案同步改成「默认关闭。打开后…」。
- 理由：这是「程序主动去翻包旁边的文本」「拿你记过的密码本去试」，属于**替用户做决定**。
  关着的时候只报一句「这个包带锁，需要解压码」，判断权留给用户；他自己在设置里打开才生效。
- **★ 顺藤查出的假测试**：`tests/auto_pw_switch.sh` 里「总开关」那两条，**只在描述文字里**
  写了 `--no-auto-password`，参数根本没传。默认开着的时候「不加开关能解开」=`ok`，
  被当成 fail 报出来才暴露。**参数漏写和「默认跳过」一样阴**——它一直在验一个不存在的场景。
  已补参数，并新增**反向断言**：什么都不配时，note / book 两个包都必须解不开（探针 10/10）。
- 回归：`cargo test` 404 过 0 挂、`flutter test` 109 过、`auto_pw_switch.sh` 10/10。
- **真机证据**：`screenshots/auto-password-default-off.png` —— 设置页两个开关都是关（灰、滑块在左），
  文案是「默认关闭。打开后，程序才会…」。


**要解决的事**：网盘资源里常见「一个视频里塞着不止一份包」的玩法。以前引擎的全文件扫描
**本来就算出了所有像归档的位置**（`scan::ScanHit`：格式 / 偏移 / 置信度），但 `locate_zip`
只取第一个就把其余**扔了**（只顺手拿去判 `Truncated` vs `NotArchive`）。所以用户遇到
「解出来是错的那一份」时，界面上一句话都没有，只能放弃。

- **引擎**：新增 `job::Candidate` + `identify_with_candidates`（老 `identify` 变成它的 `.0`，
  调用点一行没改）；`candidates` 按 **chosen 优先 → 置信度高在前 → 偏移升序**，保证
  「第 1 处」就是界面正在显示的那处。候选来自**同一次**全文件扫描，不额外多扫。
- **协议**：`scan` 每个 item 多 `candidates[]`；`list` 在 ≥2 处时也带（预览页才有入口）；
  `extract.candidate = <offset>` 精确解指定那一处（`layout::zip::locate_at_base`：从文件尾
  逐个 EOCD 往前退，退到 `base` 对上为止，复用被验过成千上万次的 `locate_with_window`）。
  **非法偏移必须报 `NOT_ARCHIVE`**，不许静默回退成"自己挑一个"——那是用户点了第 2 处、
  程序偷偷解了第 1 处的假成功。
- **界面**：预览页顶部提示条「这个文件里像是有 N 处藏着东西，现在解的是引擎认为最可能的那处；
  不对就换一处试试」+「换一处解压」；队列行同样入口。**只有 ≥2 处才显示** —— 正常包一个字
  都不多说，否则用户会以为自己的包有问题。换候选解压时**不传挑选序号**（另一段的条目可能完全不同）。
- **★ 真机验才发现的两件事**：① 我第一版把入口只做在**队列**里，而用户拖单个文件进来的主路径
  是 `_open` → **预览页**，队列那次一次都碰不到（等于白做）；② 候选按偏移排序会让「第 1 处」
  指到用户没在看的那段 —— 文案在骗人。两处都改了。
- **证据**：探针 `tests/t5_candidates.py` **8/8**（含非法 candidate 被拒）；样本解压前后产物
  从「第B处解出来的.txt」变成「第A处解出来的.txt」；`cargo test` 404 过 0 挂；`flutter test`
  109 过；`clippy` / `flutter analyze` 干净；真机截图 `screenshots/t5-multi-candidates.png`。
- **★ 坑（记一笔）**：探针默认优先取 `target/release/yshjy`，而那个是旧的 0.9.35 —— 第一次跑
  验的是旧二进制，"candidates 字段不存在"看起来像功能没做。验新代码必须显式传 `target/debug/yshjy`。
  另外 Dart **不认中文变量名**（HANDOFF 里已记两次，这次又踩第三次）。

## [0.9.35] · 2026-10-30（清 `docs/AUDIT-GAP.md` 对账欠账：4 批 9 项，含两个只读代码看不出来的真 bug）

这一版没有新功能，全是「已经承诺过、但实现/界面/文档对不上」的欠账。进度表只有一份：
`docs/TODO.md` 顶部的 T1~T15。

- **引擎**：空间不足真发 `SpaceShortage` 事件 + 错误里带 `need`/`available`（以前界面那段是死代码）；
  删掉死配置 `default_mode`；自动找解压码拆成「说明文件 / 密码本」两档。
- **界面**：空间不足卡显示三个数 + 三条出路（换个位置 / 只解压部分 / 继续试试）；
  设置页「自动找解压码」两个假开关（硬编码字符串）接成真勾选。
- **★ 真机撞出的两个 bug（只读代码看不出来）**：① 协议层失败路径无条件重新 `identify`，
  而空间不足时 identify 是成功的 → 走兜底分支、`detail` 全丢，界面只剩一句空话；
  ② 错误页把好包误报成「认不出的文件」，用户会以为文件坏了跑去重下。
- **★ 静默失效的坑**：界面写 `auto_from_notes = "off"`（带引号的字符串），引擎用 `as_bool()` 读 →
  返回 `None` → 字段被静默忽略 → 保持默认 `true`。用户勾掉了「自动抓解压码」，程序照旧翻说明文件，
  而且永远不报错。三个开关统一改走 `取开关()`。
- **门面 / 工程 / 文档**：AppStream 元数据（`appstreamcli validate` 通过）+ 7 尺寸图标 + SECURITY.md +
  行为准则 + 三个 issue 表单；CI 加界面 job；台账清掉 21 处失真。
- 回归：`cargo test` **402 过 / 0 挂**、`tests/all.sh` 44 过 / 0 挂 / 5 跳、`flutter test` **102 过 / 0 挂**、
  `clippy --all-targets` 干净。

### 2026-10-30 深夜（清 `docs/AUDIT-GAP.md` 对账欠账 · 第 4 批：设置页两个开关接线 + 一个静默失效的坑）

第 3 批把引擎的「自动找解压码」拆成两档，但设置页那两行还是**硬编码字符串**
（`auto_from_notes = true（从旁边的说明文件里找）`），用户看着有、其实改不了。这一批接上。

- **界面（设置页）**：新增两个真勾选「自动从「说明.txt」里找解压码」/「自动试「密码本」里的解压码」，
  默认开（与引擎默认一致）；保存时写回 `[password]` 段；删掉原来那行假开关。
  新增 `_readSwitch()`：同时认 TOML 布尔（`true`/`false`，用户手改）和字符串（`"on"`/`"off"`，本页保存的写法）。
- **不需要新协议命令**：HANDOFF 之前判断「要做成真勾选，得先有读配置/写配置的协议命令」是**保守误判**。
  设置页本来就有 `_parseSection` / `_upsertSection` 逐行读写 `config.toml`（`flush_movable` 从 0.9.34
  起就是这么做的），`app_settings.dart` 的 `open_after` / `close_action` 也是同一套。少加一条协议命令。
- **★ 顺藤查出的真 bug（界面看着关了、引擎还在做）**：`config.rs` 里 `auto_from_notes` /
  `auto_from_book` / `warn_executables` 用的是 `toml::Value::as_bool()` —— 对界面写的字符串
  `"off"` 返回 `None` → 该字段被**静默忽略** → 保持默认 `true`。也就是说：用户勾掉了
  「自动抓解压码」，程序照样去翻包旁边的说明文件，而且永远不报错。
  修法：三个开关统一走已有的 `取开关()`（布尔和 `on`/`off`/`yes`/`1` 都认），与 `[safety]`
  段同一套读法。**这是本轮最值钱的一条**：光看界面截图和界面代码都发现不了，只有把
  「界面写什么」和「引擎读什么」摆在一起比才看得出来。
- **测试**：Rust `设置页写的字符串开关引擎也认`（字符串 off 真关掉、布尔 true 也认，两种写法同一结果）；
  Dart `test/settings_switch_widget_test.dart` 3 条（无配置时默认开 / 两种写法都显示关 /
  保存后写 on-off 且同段用户手写的 `file`、`max_candidates`、注释一个不丢，再进一次能读回来）。
- **真机验证（不是只在测试里跑）**：隔离配置目录启动界面，点设置页两个开关关掉 →
  点保存 → `config.toml` 出现 `auto_from_notes = "off"` / `auto_from_book = "off"`；
  然后同一个加密 zip + 旁边放 `说明.txt`：开关 off 时输出「这个包带锁，需要解压码」（真的没去翻），
  改成 on 后输出「解压码是从旁边的说明里找到的：./说明.txt: 解压码：notePW」并解开。
  截图：`screenshots/settings-auto-password-off.jpg`、`screenshots/settings-save-button.jpg`。
- **踩到的坑（写进测试注释了）**：`testWidgets` 用的是假时钟，`await File.readAsString()`
  这类**真 I/O** 的 Future 在里面**永远不完成** → `_load()` 卡在第一行、界面停在默认值，
  测试会**假绿**（我第一版就是这样绿的）。只有把 `pumpWidget` 整段放进
  `tester.runAsync()` 才连得上真实事件循环；保存按钮同理。
- 回归：`cargo test` **402 过 / 0 挂**、`clippy --all-targets` 干净、`flutter analyze` 只剩一条既有 info、
  `flutter test` **102 过 / 0 挂**。

### 2026-10-30 夜（清 `docs/AUDIT-GAP.md` 对账欠账 · 第 3 批：自动找密码拆两档）

界面 S4 要两个勾选（「先从说明文件找」/「再试试密码本」），可引擎侧**只有一个总开关**
`--no-auto-password` —— 两件事绑死在一起，界面想给两个框也接不了。这一批把引擎拆开。

- 配置 `[password]` 新增 `auto_from_book`（试密码本，默认 `true`）；`auto_from_notes`
  （从说明文件抓）本来就有，只是以前没有单独的开关能关它。
- 命令行新增 `--no-auto-password-notes` / `--no-auto-password-book`；
  老的总开关 `--no-auto-password` 语义不变（两档一起关），已装的配置和脚本不受影响。
- 为什么值得拆：两档代价不一样。密码本是用户**亲手记**的（值得每次先试）；
  说明文件是程序去读包旁边的文本现猜（有人就是不想让程序翻旁边的文件）。
  合成一个开关等于逼人二选一，于是干脆两个都不关。
- 7 个语言包的 `--help` 文案、shell 补全、man 页、`docs/CLI-SPEC.md` 全部同步
  （补全/man 由 `scripts/gen-cli-assets.py` 从 `--dump-cli-spec` + 中文语言包重新生成，
  不是手抄的第二份真源）。
- 探针 `tests/auto_pw_switch.sh`：造两个加密包，分别用「密码本里的码」和「说明.txt 里的码」
  加密，然后把两个开关各关一次 —— **8/8 通过**，证明关一档不会连坐另一档，总开关仍然两档都关。
  用临时 `XDG_CONFIG_HOME`，不碰用户真实密码本。
- **界面还没接**：设置页那几行「自动抓解压码 = true」是**硬编码字符串**（页面自己标了
  「其它（改 config.toml）」，算诚实承认）。要做成真勾选，得先有「读配置 / 写配置」的协议命令，
  排在 T8 界面半。

### 2026-10-30 夜（清 `docs/AUDIT-GAP.md` 对账欠账 · 第 2 批：界面 S5 + 真机撞出的两个协议 bug）

**这一段是"引擎修了、界面还是死路"的收尾。** 第 1 批让引擎真发 `SpaceShortage`
事件并在错误里带 `need`/`available`；这一批让界面真的把它们显示出来 —— 做完真机点了一次，
结果真机当场撞出两个只看代码看不出来的 bug，都在这里修掉了。

- **界面（S5 空间不足）**：`home_page.dart` 新增错误卡片，「需要 3.00 GB / 目标盘只有 1.94 GB /
  还差 1.06 GB」+ 三个按钮：**换个位置**（改输出目录重解）、**只解压部分**（回预览树勾选）、
  **继续试试**（协议 `extract.force`）。第三个按钮以前根本传不进去 —— `engine.dart` 的
  `extract()` 一个参数都没给协议，所以 `--force` 在界面上是死设置。
- **真机 bug ①：协议层把 detail 吞了。** 引擎的 `JobOutcome.error_detail` 已经有了，
  但 `api.rs` 在失败时**无条件重新 identify 一次**再用结果兜底 —— 而空间不足时 identify 是
  **成功**的，于是走进兜底分支、`detail` 全丢。界面只能显示「清一点空间再试」，三个数一个都没有。
  修法：`error_detail` 非空就直接带出去。新增探针 `tests/t2_space_detail.py`（默认跳过，
  设 `YSHJY_SPACE_PROBE_PKG=<3GB包>` 才跑）卡住这条。
- **真机 bug ②：错误页误报「这个文件看起来是：认不出的文件」。** 包本身好好的，这句话会把人
  引去重新下载。修法：空间不足时不给类型提示（和分卷中间块同样的道理）。
- **界面兜底**：`SPACE_SHORTAGE` 事件先到、错误对象后到，两个数会在事件里再存一份，
  协议层哪天又漏 detail 也不至于只剩一句空话。
- **测试**：`gui/test/space_shortage_widget_test.dart` 10 条（三数 / int 与字符串都要认 /
  缺参数不乱猜 / 按钮齐 / force 代价说明 / 不许说「认不出的文件」/ 别的错误码不许出现这张卡）。
  反向验证过：把判断改成 `if (false)`，5 条正向用例当场全红。

真机证据：造「解压后 3 GB、压缩后 3 MB」的包放 `/run/user/1000`（tmpfs 2 GB），点开始解压 →
界面如实显示三个数。截图 `screenshots/space-shortage.jpg`。
回归：`cargo test` 401 过 / `tests/all.sh` **43 过 0 挂 5 跳** / `flutter test` 99 过。

### 2026-10-30 夜（清 `docs/AUDIT-GAP.md` 对账欠账 · 第 1 批：引擎两条）

> 由来：`docs/AUDIT-GAP.md` 逐层对账查出「界面是短板、台账失真、一个真死配置」。
> 用户口径「全搞完」，进度清单在 `docs/TODO.md` 顶部的「🔨 本轮施工进度」T1~T15。
> 这一批只动引擎两层，界面从 T4 开始。

### 修复：空间不足只说一句话，用户不知道要清多少（S5 的另一半）

- **真问题比台账写的更严重**：`Event::SpaceShortage` **全库只有定义、零发射点** ——
  界面里那段 `case 'SPACE_SHORTAGE'` 是死代码，`available` 字段一次都没被用上，
  用户永远看不到「还差多少」。台账记的是「界面没用 available」，实际是引擎根本不发。
- **引擎**：`job.rs` 空间预检失败时真发事件（`needed` + `available`）；
  `JobOutcome` 新增 `error_detail`（结构化键值对，**不是文案**），把 `need`/`available`/`dest`
  带到外壳层。
- **CLI**：新增纯函数 `空间不足说明()` 拼出「需要 3.00 GB / 目标盘只有 1.94 GB / 还差 1.06 GB」；
  缺参数就返回 `None`（不乱猜，宁可只说一句「磁盘空间不够」）。
  语言包 7 份都加了 `result.space_shortage`。
- **顺带修台账 ⑤ 的「0 B，用时 0.0 秒」**：一个字节没写、耗时 <1 秒的失败（识别/预检阶段就被拦下）
  改用 `result.line_short`，不再印那两个零信息量的数。
- **实测**（真跑，`~/.hermes/cache/scratch` 里造了一个「解压后 3 GB、压缩后 3 MB」的 zip，
  目标盘 `/run/user/1000` 只有 2 GB）：
  - 改前：`❌ …—— 磁盘空间不够（0 B，用时 0.0 秒）`
  - 改后：`❌ …—— 磁盘空间不够` + `需要 3.00 GB / 目标盘只有 1.94 GB / 还差 1.06 GB`
  - `--json` 事件流里真有 `{"SpaceShortage":{"job":1,"needed":3221225472,"available":2085396480}}`
  - `--lang en`：`needs 3.00 GB / only 1.94 GB free / short by 1.06 GB`
  - 正面对照组：正常小包照旧 `✅ … 解开（6 B，用时 0.0 秒）`（没被新分支吃掉）
- 单测 2 条：三数算对（含 `还差`）/ 缺参数与坏参数不乱猜。
  ⚠️ 踩坑：`文案()` 是 `OnceLock`，单测里没初始化 → panic 在 `main.rs:148`；
  测试内补 `I18N.set(I18n::new(Some("zh-CN")))` 即可。

### 删除：`[general] default_mode` 是个假开关

- 实测全库只有「字段 / 默认值 / 解析 / 保存模板」四处，**没有任何读取点**；
  而「默认模式」的真机制是**程序名**（`yshjy` 解压 / `yshys` 打包，见 `main.rs`）。
- 处理方式是**删掉**，不是接上：接一个与程序名重复的第二套优先级只会让行为更难解释。
  `src/config.rs` 5 处全清；`docs/BACKEND-SPEC.md` 改成「已删除 + 为什么」。
- 验收：`yshjy --show-config` 里已无该项；老配置文件里写着 `default_mode` 也不会报错（未知键忽略）。

### 顺带发现（**只记录，本轮不动**）

- CLI 的 `文件类型提示()`（「ℹ 这个文件看起来是：ZIP 压缩包」）仍硬编码中文，
  `--lang en` 下这句还是中文。属界面多语言那一条（T14）的范围。

## [0.9.34] · 2026-10-30（移动设备落盘检验：默认关的开关 + 真进度 + 只对U盘生效）

> 用户 2026-10-30 定方案：**默认关**的设置项；打开后解压到 U 盘/移动硬盘时
> 等数据真落盘才报成功，并且给他**真进度条**（原话：「有个东西就不会让他等待了」）；
> 本机硬盘一秒都不多等。由来见 `docs/TODO.md` 表尾那条（2026-10-29 真机实测：
> 屏幕报 ✅ 那一刻 300 MB 数据全在页缓存，拔盘一个文件不剩）。

### 为什么不是"一律 fsync"

热拔 U 盘本来就是错误操作（SuperUser 1718589 / Unix.SE 560908 / LWN 都指向"先安全弹出"），
`cp` / `rsync` / `unzip` 也都不 fsync —— 所以**默认不替用户扛**（慢盘上等 300 MB 要几分钟，
没开开关的人只会以为程序卡死）。但：
① 开关**关着也要提示**「拔盘前请先安全弹出」（不提这一句等于什么都没做）；
② 打开开关的人给真进度，不让他干等。

### 引擎

- 新增 `src/flush.rs`
  - `is_removable()`：`/proc/self/mountinfo` 最长匹配挂载点 → `/sys/dev/block/<maj:min>`
    真实路径含 `/usb` **或** 整盘 `removable=1`。**移动硬盘 removable 通常是 0，
    只看它会漏**，所以两条判据并用。
  - `wait_until_written()`：优先 `sync -f <目标>`（只刷该文件系统，不牵连别的盘），
    老 coreutils 退回全量 `sync`；每 200 ms 报一次真实待写入字节
    （`/proc/meminfo` 的 Dirty + Writeback）；用户点取消立刻返回（`FlushOutcome::Cancelled`）。
  - 7 条单测：最长匹配挂载点 / meminfo 解析 / 造假 sysfs 验三种设备 /
    本机目录不是移动设备 / 端到端真跑 sync / 取消立刻返回 / 目标不存在返回不支持。
- `config.rs`：`[safety] flush_movable`，**默认 false**。解析同时认 TOML 布尔 `true`
  和界面存的 `"on"`/`"off"`（只认一种就会出现"界面开了、引擎不认"这种最难查的错）；
  写错当没写，不连累别的字段。
- `event.rs`：新增 `FlushProgress{job,pending,elapsed_ms}`、`FlushFinished{job,outcome,millis}`；
  新增诊断码 `MovableNotVerified`（开关关着但目标在移动设备上 → 提醒先安全弹出）。
- `job.rs`：三条件**同时**成立才检验（开关开 + 解出东西 + 目标在移动设备）。
  `stats.millis` 改成 **总耗时 − 检验耗时** —— 不扣的话屏幕会写「用时 5.7 秒」，
  而其中 5.6 秒是在等U盘写盘；反过来不开检验时写「用时 0.0 秒」又是在撒谎。
  新增 `stats.verify_millis` / `stats.flush_outcome`。

### 外壳

- 终端：检验中每 200 ms 覆盖一行「正在检验落盘：还剩约 X MB 待写入（已等 Y 秒）—— 请勿拔出设备」；
  结束按 DONE / CANCELLED / UNSUPPORTED **三种说法分开**；结果行下面按结局追加提示。
- 界面：设置页新增开关「解压到U盘/移动硬盘时，等数据真正写完再报成功」（默认关）；
  检验期间单独一屏「正在检验落盘…」，含真实进度条（`1 − 待写入/峰值`，只前进不回退）、
  「还剩约 X MB 待写入」、「已等 X 秒」、「⚠️请勿拔出设备」、以及「停止等待」按钮。
- 7 种语言补 `[flush]`、`[result.hint]`、`diag.movable_not_verified`、`config.flush_movable`。

### 证据（2026-10-30 真机 · 14.6G exFAT U 盘 `/dev/sdb1`）

**铁证对照（读 `/sys/block/sdb/stat` 的已写扇区）**

| 场景 | 程序报「完成」那一刻 U 盘实际写入 | 再等 12 秒 |
|---|---|---|
| 关检验（20 MB） | **0.0 MB** | 20.0 MB（内核后来才刷进去） |
| 开检验（20 MB） | **20.0 MB**（等 5.6 秒） | 20.0 MB |

**命令行（100 MB）**：`落盘检验通过（18.2 秒）` + `解开（用时 0.1 秒）` —— 两个数分开。

**界面（用户亲手点「开始解压」，U 盘上的 100 MB 包）**：结果页显示
`解开了 1 个文件 / 用时 0.1 秒 · 100.0 MB` + `✓ 落盘校验通过（13.2 秒）：数据已写入设备，可以安全拔出`。
检验过程中用户在屏幕上亲眼看到进度条与「请勿拔出设备」提示。

**探针 `tests/flush_movable.sh`（已挂进 `tests/all.sh`）4/4 通过**：
① 本机硬盘 + 开关关 → 0 条 FLUSH；② 本机硬盘 + 开关开 → 0 条（**开关不是万能开关**）；
③ 移动设备 + 开关关 → 不等，但发 `MOVABLE_NOT_VERIFIED` 诊断；
④ 移动设备 + 开关开 → 等完才报成功，且两个数字分开。

回归：`cargo test` **367 通过 / 0 挂**；`cargo clippy --all-targets` 干净；
`flutter analyze` 0 issue；`flutter test` **89 通过**。

### 坑（记一笔，别再踩）

- `--json` 吐的是 serde 默认的**外部标签**格式 `{"FlushFinished":{...}}`，
  而界面走 `--serve` 的 `{"kind":..,"data":..}` —— 探针一开始按后者解析，误报"引擎没发事件"。
- 界面输出目录默认「包旁边」，**不读** `config.toml` 的 `[output] dir`；
  要验证移动设备，得把包放到 U 盘上（真实用户用法也是这样）。
- 同一次验证里同时开两个界面实例时，uinput 点击进不去第二个窗口（第一个实例正常）——
  「点不动」不是功能坏了，别误判。

### 顺藤查同类：打包 / 救援 / 封面也接上了同一个开关

> 用户问「前面讨论的有没有可能不止这一个问题」—— 扫全库发现：一开始只有**解压**那条路接了落盘检验，
> 其余写盘路径全靠"大多数程序都这样"蒙着。最要命的是打包。

- **统一入口** `flush::flush_if_needed(dest, cap, cancel, on_progress)`：
  开关关 / 本机盘 / 平台不支持 → 返回 `None`，**一分钟都不多等**；否则等真落盘。
  `cap` 用来按「本次产物的字节数」封顶 —— 内核读数是**全系统**脏页，
  不封顶会出现「打了个 15 MB 的包，界面显示还剩 472 MB」这种假数字。
- **打包（pack）**：CLI 与界面两条路径都接上，并且**排在自检之前** ——
  自检是把包读回来验证，但读回来命中的是页缓存，**自检通过 ≠ 数据在盘上**。
  先落盘再自检，读到的才是盘上的数据。返回值新增 `flush` 字段。
- **救援（rescue）**：产出的是"刚救回来的数据"，更不能让人以为已经安全。
- **封面提取**：小文件也是写盘产物；只做落盘、不发事件（它是解压的附赠品，别抢界面那一屏）。
- **打包页**：检验期间进度条转不确定态 + 一行「正在检验落盘：还剩约 X MB 待写入 · 已等 N 秒 · ⚠️ 请勿拔出设备」。
- **终端**：进度覆盖行只在**真终端**里画（管道里 `\r` 不生效，`| tail` 会刷成一屏重复行）；
  事件照发，`--json` 与界面都能看到。

**证据**：`tests/flush_movable.sh` 扩到 **6/6 通过**（新增「本机盘打包 → 0 条 FLUSH」
「移动设备打包 → 等完才报成功」）。真机 U 盘：打包 15 MB → `落盘检验通过（2.4 秒）` → `自检通过`（顺序正确）。

**还没修的一笔**（记在 `docs/TODO.md`）：空间不足时终端仍显示「磁盘空间不够（0 B，用时 0.0 秒）」，
`0 B` 不是人话，应改成「需要 X / 只有 Y / 还差 Z」。

## [0.9.33] · 2026-10-30（打包页：输出名不跟着换 + 找不到文件）

> 用户装完 0.9.32 报的两个问题，其实是**一条因果链**。

### ① 换了输入文件，输出名还停在上一个

- **根因**：`_addFiles` / `_addFolder` 里写的是 `_out ??= _suggestOut(...)` ——
  **只在 `_out` 为空时建议一次**。加了第一个文件后 `_out` 就有值了，换文件、加文件永远不更新。
- **后果**：文件按**旧名字**生成，用户照界面上的新名字去文件夹找 → 找不到。
- **改法**：加 `_outUserPicked` 标记 —— 用户没手动挑过输出位置时，输出名跟着当前输入走；
  手动挑过就不再覆盖（尊重用户选择，不硬抢）。

### ② 完成后不告诉人文件在哪

- **根因**：`pack` 命令的返回值里**根本没有输出路径**，界面只能拿自己算的建议路径猜；
  结果卡片也只会说「✅ 完成」，既没路径也没打开按钮。
- **改法**：引擎 `pack` 新增 **`out`** 字段（协议只加不删，老界面忽略它也不会坏）；
  界面结果卡片显示「文件在这儿：<完整路径>」+「打开所在文件夹」+「复制路径」。
  `_parentOf()` 同时认 `/` 和 `\`（Windows 路径）。

### 测试与遗留

- `flutter test` **69 过 / 0 挂**；`flutter analyze` 干净。
- 引擎实测：`pack` 返回 `out = …/产物.zip`，产物 1108 B 真落地。
- ⚠ **打包页这两处还没有专门的回归测试**（要触发系统文件选择器）。
  下一步：把命名逻辑（`_suggestOut` / `_withExt` / `_parentOf`）抽成顶层纯函数再补单测
  ——**已记进 `docs/TODO.md` 表尾**，别再拖成下一个「46 条全绿却漏着」的坑。
  > **同日已补齐**：抽成 `gui/lib/pack_naming.dart` + `gui/test/pack_naming_test.dart` 20 条，
  > 并顺带修出「写死 `/` 导致 Windows 路径算错」的真 bug。`flutter test` 89 过。



## [0.9.32] · 2026-10-30（GUI「加密包是死路」修复 + Windows W1~W10 战役一起发版）

> 仓库搬到 Windows 机器（`E:\yshjy-20261030`，与 Linux 机走移动硬盘同一份 `.git`）后的第一轮代码改动，
> 加上「加密包在图形界面里永远没有解压码输入框」这个真机 bug 的修复，一起发 **0.9.32**。
> 待办来源与验收标准：`docs/TODO.md` 的「2026-10-30 Windows 实测审计」。**装机版现在是 0.9.32。**

### GUI 加密包是死路（用户在 Windows 真机撞上；Linux 同源，已修）

**现象**：需要解压码的包拖进图形界面 → 只显示一句「这个包需要解压码」，
**永远找不到输入解压码的地方**，等于死路。CLI 同一个包能正常要密码、正常解开。
先被当成 Windows 编译问题，实际是界面的判断写错了码名，两个平台一起错。

**根因（三个，同一个病：码名凭印象写）**：

1. `home_page.dart` 的 `_needsPassword` 判的是 `PASSWORD_REQUIRED` / `PASSWORD_WRONG` ——
   实测 `yshjy --serve` 真实返回的是 **`NEED_PASSWORD` / `WRONG_PASSWORD`**，两对全不匹配
   → 「要不要显示输入框」的条件**恒假**。历史上 N1 批修过文案表 `humanize.dart`，
   但这个开关漏改，于是「文案对了、框永不出现」。
2. 漏了 **`HEAD_ENCRYPTED`**（7z `-mhe=on` 连文件清单都加密，预览阶段就失败）→ 这类包同样没有输入框。
3. `camelToCode` 老实现把**单字大写的码逐字符拆开**：`TRUNCATED` → `T_R_U_N_C_A_T_E_D`、
   `CANCELLED` → `C_A_N_C_E_L_L_E_D`，查文案必然落空、用户看到「原因不明」。

**连带查出并修掉的同类问题**：

- `_canRescue` 用了 `PARTIAL_DATA`（那是**诊断码**，不进错误码通道）和
  `CHECKSUM_FAILED`（**引擎里根本不存在**）→ 「坏了但能救」的包反而不给救援按钮。
  改成真码 `DATA_LOST` / `TRUNCATED`（`NOT_ARCHIVE` 仍要 `looks_like=zip` 才给）。
- `errorText` 缺 `VIDEO_UNPLAYABLE` —— 引擎 17 个码里唯一没文案的。
- `docs/PROTOCOL.md`「错误码」一节的示例表写的 `PASSWORD_REQUIRED` / `PASSWORD_WRONG` / `IO`
  **三个码引擎里都不存在**，已换成 17 码全表并加警告；`docs/GUI-PLAN.md`、`docs/TODO.md`
  救援条目同步改正。

**防回归（这次的重点：之前 46 条界面测试全绿却漏着它）**：

- 判断收进纯函数 `humanize.dart::codeNeedsPassword` / `codeCanRescue`，页面只委托；
- 新增 `gui/test/humanize_test.dart` **10 条**：引擎 17 个错误码 / 10 个诊断码逐个断言有专属文案
  （将来引擎加码而界面漏文案会红）＋ 「历史废码必须为 false」显式钉死；
- 新增 `gui/test/password_box_widget_test.dart` **11 条**：给 `HomePage` 加测试注入口
  `debugErrorCode`（与 `debugPreview` 同性质），断言 `NEED_PASSWORD` / `WRONG_PASSWORD` /
  `HEAD_ENCRYPTED` / 驼峰名 → 出现「用这个解压码重试」；非密码码 → 不出现；`DATA_LOST` → 有救援按钮；
- **反向验证**：把 `_needsPassword` 改回 `PASSWORD_REQUIRED` 老写法 → 密码那 4 条当场红，改回即绿；
- `flutter test`（带 `XDG_CONFIG_HOME` 隔离，按 `tests/all.sh` 的跑法）= **67 过 / 0 挂**。

### 预览页直接给解压码输入框（用户第二轮反馈）

用户装完第一版修复后反馈：「界面没有，但我解压它弹出来输密码的框了」——
错误页的框是有了，可**预览页还是没有入口**，必须点一次「开始解压」、失败一次才见到框。

- **根因不是 bug 而是漏用数据**：引擎 `list` 早就返回了 `encrypted_count` + `enc_kind` +
  `hints=[NeedsPassword{entries}]`，界面拿去只显示了一个「N 个加密」标签，
  没拿它问密码。
- **改法**：预览页在 `encryptedCount > 0` 时直接渲染解压码输入框（含眼睛切换 + 密码本常用码
  chips），填好点「开始解压」一次成功；留空也照旧走「失败再问」那条路，不强迫。
- **顺手修一个潜在坑**：`_open()` 打开新包时不清 `_password` / `_pwCtrl`，
  换包后旧密码会静默套到新包上（表现成「没输密码却报解压码不对」）。现在换包即清空。
- **测试**：`password_box_widget_test.dart` 加 2 条 —— 加密包预览时必须出现输入框、
  未加密包不许乱出现；`flutter test` **69 过 / 0 挂**。

**装机版最终验证（2026-10-30，用 `/usr/bin/yshjy` 与 `/usr/lib/yshjy/gui` 本身）**：

| 项 | 结果 |
|---|---|
| `dpkg -l yshjy` / `yshjy --version` | **0.9.32** |
| 桌面菜单认领格式 | **14 种**（按 `build-deb.sh` 的算法数） |
| `bash tests/pick.sh /usr/bin/yshjy` | **11 项全绿** |
| 装机版界面核对（`libapp.so`） | 老废码 `PASSWORD_REQUIRED` **0 次**；`NEED_PASSWORD` 1 次；预览页新文案「这个包有」「留空也行」均在（Dart AOT 里中文按 **UTF-16LE** 存，用 UTF-8 搜会误判成 0，别被它骗） |
| 真打包 | `ys 目录 -o 伪装视频.mp4` → `file` 认成 `ISO Media, MP4` |
| 真解压（带码） | AES 加密包 `-p 123456` 解开，**5 个文件 sha256 与源完全一致** |
| `scripts/test-packages.sh` | **37 项全绿** |



**Linux 侧实跑证据（2026-10-30）**：

- `cargo test` = **390 过 / 0 挂 / 1 ignored**（`359 + 29 + 2`；合并进来的 W1 多了 3 条时区测试，
  基线 357 → 360 属预期）；
- `bash tests/all.sh` = **41 过 / 0 挂 / 5 跳过，182 秒**；
- 顺手清掉一个**测试自己的老账**：`tests/trash_source.sh` 的 Linux 断言还在找
  `Trash/files/包.zip`，可 W7 早把夹具改名成 `pkg1.zip`（Windows 的 powershell 过不了中文名），
  于是 Linux 侧必红「回收站里没有完整条目（2 个文件）」—— 而那 2 个文件正是
  `files/pkg1.zip` + `info/pkg1.zip.trashinfo`，**产品逻辑一直是对的，纯粹是断言名字没跟着改**。
  改成 `pkg1.zip` 后 10/10 绿。
  > 教训：**夹具改名时，所有断言里的名字要一起改**；这类"名字对不上"跟本次的码名错位是同一个病。

### W1 ✅ 修时区偏移（解出的文件时间不再 +8 小时）

- **根因**：`crash.rs` 的 `本地时区偏移秒()` 只有 `#[cfg(unix)]` 分支，Windows 上写死 0 ——
  zip 里存的「本地时间」被当成 UTC，解出的文件时间整体 +8 小时（实测 00:44:56 → 08:44:58）。
- **改法**（照审计定稿方案，比最初设想的 `GetTimeZoneInformation` 简单）：
  `GetLocalTime()` − `GetSystemTime()` 直接得当前偏移，夏令时由 Windows 自己算，零手算
  （思路出自参考项目 `refs/upstream/time` 的 `local_offset_at/windows.rs`）。
- **改动文件**：`src/crash.rs`（Windows 分支 + `民用日` 换算 + 3 条单测）、
  `Cargo.toml`（新增 **仅在 Windows 构建时** 生效的依赖 `windows-sys 0.61`，
  features `Win32_Foundation` + `Win32_System_SystemInformation`；Linux/macOS 依赖树与产物一个字节不变）。
- **与审计的一处出入**：`SYSTEMTIME` 结构体实际在 `Win32::Foundation` 而非
  `SystemInformation`（后者只有函数），所以多启了一个 feature，已核对 windows-sys 0.61.2 源码。
- **验收证据**（2026-10-30 本机实跑）：
  - 真实解包：mtime `00:44:56` 的源文件打 zip 伪装 `.mp4`，`yshjy 伪装视频.mp4` 解出
    mtime **`00:44:56 +0800` 与源完全一致**（修复前是 08:44:58）；
  - 新增单测 3 条全绿（偏移落在真实时区范围 `-43200..=50400` / 同秒两次取样一致 / `民用日` 纪元与闰年锚点）；
  - `cargo test`：lib 341 过 / 14 挂（与基线一致，全是 W2/W6/W8 已知账）、bin 32 过 / 0 挂。
### W2 ✅ 修断点续传（两层根因，第二层是验收时实测挖出来的）

- **根因一（审计发现的）**：`resume.rs` 在 `append` 句柄上 `set_len(0)` 截断 —— Windows 的
  append 只给 `FILE_APPEND_DATA` 权限，`SetEndOfFile` 被拒（os error 5）→ `open_in` 返回
  `None` → 每次从头解，不报错。**修法**：截断分支单独开 `.create(true).truncate(true).write(true)`
  句柄（仍原地截断不删文件，「比删了重建少一次竞态」的原意不变），续传分支继续用 append。
- **根因二（验收时发现的，更深）**：单测修完全绿、真机却**仍然不续传**。追查发现
  `绝对化()` 先走 `canonicalize`：Windows 上目录**存在**时返回 `\\?\E:\...`、**不存在**时才落
  普通路径 —— 输出目录第一次跑还不存在、中断后重跑必然已存在，同一条命令两次算出
  **两个不同的指纹哈希 → 查的是两份不同的记录文件**，续传永远对不上号。Linux 无此问题
  （canonicalize 不加前缀）。**修法**：Windows 分支改纯词法归一（转绝对 + 去 `\\?\` 前缀 +
  分隔符统一），存在与否结果一致；Unix 保持 canonicalize 行为一字不动。
- **改动文件**：`src/resume.rs`（append→truncate 拆分 + `绝对化` 平台分支 + 1 条新单测）。
- **验收证据**（2026-10-30 本机实跑，250MB / 50 个文件伪装包，解压目标为移动硬盘以拖慢写入）：
  - 解到 19/50 个时硬杀（`timeout -s KILL`，退出码 137）→ 续传记录留 **20 行（头 1 + 条目 19）**；
  - 重跑：出现 **「⏩ 接着上次没解完的来（上次已完成 19 项）」**，只解剩余 160MB，**50/50 全齐、0 个重名副本**；
  - `cargo test --lib resume::` **12 条全绿**（修复前挂 10）；全库 353 过 / 3 挂（挂的全是 W6/W8 已知账）。
### W4 ✅ 输解压码不再明文显示（改用 dialoguer::Password）

- **根因**：`读一行不回显()` 只有 `#[cfg(unix)]` 的手写 termios 分支，Windows 落进普通读，
  解压码明文显示在屏幕上。
- **改法**（照 BUILD-Windows.md 定稿）：整个函数换成 `dialoguer::Password`（`--pick` 同族依赖，
  关回显/退格编辑/回车提交跨平台都是它包的），**顺手删掉一段手写 termios unsafe**；
  管道喂码在引擎设计里本就不存在（`main.rs` 只在 stdin 是终端时才问码，脚本/管道绝不挂死等人），
  防御性保留：stdin 非 TTY 或 stderr 被重定向时退回普通读（旧代码注释同样接受「回显关不掉」的降级）。
  顺带的行为变化：raw mode 下 Ctrl-C 不再走全局中断退出，而是与「直接回车」同路按放弃算。
- **改动文件**：`src/main.rs`（`问解压码` / `读一行不回显`）。
- **验收**：编译过、bin 单测 32 条全绿；管道喂码、stderr 重定向两条降级路径走查确认。
  **留老板 10 秒肉眼**：真终端里 `yshjy 加密包.mp4`（无 `-p`），敲密码应当**看不见字符**、
  回车后正常继续 —— winpty 在本会话的无头环境拿不到控制台尺寸（winpty.cc:924 断言），
  这一步自动化不了，dialoguer 不回显是其核心保证（Linux 产线 `--pick` 同库已在用）。
### W5 ✅ 内存预检在 Windows 上恢复生效（外壳注册探针）

- **根因**：`hardware.rs` 只会读 `/proc/meminfo`，Windows 上永远 `None` → 7z 解压前的
  内存预检 `(Some(need), Some(avail))` 拆不开、静默放行 —— 防 OOM 的内存闸形同虚设
  （「总量闸」还在，「内存闸」丢了）。
- **改法**：lib 保持 `#![forbid(unsafe_code)]`，新增 `hardware::注册内存探针(fn) -> Option<u64>`——
  bin 启动时把 `windows可用内存MiB()`（`GlobalMemoryStatusEx.ullAvailPhys`，与 W1 共用
  `windows-sys` 的 `Win32_System_SystemInformation` feature，**零新增依赖**）注册进去；
  预检每次**实时**来取（不存快照：比对的是「此刻」的空闲内存）。读不到仍降级放行，
  与「读不到 /proc」同一待遇，探针失败绝不拦解压。Linux 不注册，`/proc` 路径一字未动。
- **改动文件**：`src/hardware.rs`（探针注册 + 优先取用）、`src/main.rs`（Windows 探针 + 注册 + 2 条单测）。
- **验收证据**（2026-10-30 本机实跑）：bin 单测 2 条全绿（探针读数 0 < m < 2^31 MiB；
  注册后 `available_mem_mib()` 不再是 None）；真 7z 冒烟通过。
  审计验收里「造超内存的包应被拦下来」一条：拦截**比较逻辑**是平台无关代码（Linux 侧已验），
  Windows 侧本次交付的是缺失的「可用内存」读数；造一个字典 ≥ 空闲内存的真 7z 需要压缩端
  实际分配同量内存，代价过大，留给老板想做时再补（`7z a -md=<接近空闲内存>`）。
### W6 ✅ 配置里的 `~` 在 Windows 也能展开

- **根因**：`展开波浪号()` 只认 `HOME`，Windows 用户主目录在 `USERPROFILE` →
  配置里写 `dir = "~/出"` 永远不展开。
- **改法**：走已有的 `paths::home()`（它已按平台优先 `USERPROFILE`）。测试同步改
  （比较从字符串换 PathBuf：Windows 分隔符是 `\`，字符串比对永远不等）。
- **验收**：`config::` 13 条全绿（含原来在 Windows 必挂的 `波浪号会展开`）。

### W8 ✅ 单测在 Windows 清零（353 过 3 挂 → **355 过 0 挂**）

- `resume::` 10 条 → 已随 **W2** 修复全绿；
- `config::波浪号会展开` → 已随 **W6** 修复全绿；
- `paths::xdg_绝对路径优先`：`/tmp/state` 在 Windows 不算绝对路径（没盘符）→
  按审计加 `#[cfg(unix)]`（纯 Unix 语义的用例）；
- `extract_stream::tar_能展开成目录树`：**挖出比 tar 更深的假绿根源** ——
  原生进程 `Command::new("bash")` 撞上 **System32 的 WSL bash**（CreateProcess 搜索顺序
  System32 先于 PATH，`where bash` 看不出来；WSL 里 Windows 路径不存在）。
  这不止挂了 tar 一条：**make_with 系的 gzip/xz/bzip2/zstd 四条在 Windows 一直是
  「静默跳过」的假绿**（bash cd 失败 → 返回 None → 测试打印跳过还显示通过）。
  全部改为直起工具 + `current_dir`（不走 bash），现在这五条在 Windows **真跑真过**；
  Linux 路径行为不变（同样直起工具，无 bash 参与，原本语义一致）。
- **改动文件**：`src/paths.rs`、`src/extract_stream.rs`（仅测试代码）。
- **验收**：`cargo test --lib` **355 过 / 0 挂**（基线 341/14）；bin 34/0。
### W3 ✅ Windows 回收站走系统 shell（引 trash crate）

- **根因**：`trash_dir()` 只认 XDG，Windows 上没 `HOME` → 退到**当前盘根**的
  `.local/share/Trash`，资源管理器不认 —— 等于没删进回收站。
- **改法**（照审计定案）：Windows 引 `trash 5.2`（COM `IFileOperation`，MIT，
  台账 §9 已有真身），`lib.rs` 的 `#![forbid(unsafe_code)]` 自己写不了 COM。
  crate 内部已带 `FOF_ALLOWUNDO | FOF_NO_UI`（不弹窗、能还原）并处理了
  「`PerformOperations` 成功但实际被中止」的坑（`GetAnyOperationsAborted`）。
  Unix 的 XDG 自研实现原样保留（`#[cfg(unix)]`）。**仅在 Windows 构建时拉依赖**，
  Linux 依赖树与产物不变。
- **改动文件**：`Cargo.toml`、`src/trash.rs`（平台分支 + Windows 单测）；
  顺带清了 clippy（extract7z 一处存量 `nonminimal_bool`、`windows可用内存mib` 改小写、
  dialoguer 分支改 `.ok()`—— 现在全库 clippy 干净）。
- **验收证据**（2026-10-30 本机实跑）：`trash::` 单测过 —— Windows 上真删进**系统回收站**
  （测试文件留在回收站属预期副作用，可在资源管理器还原验证）；lib 354 过 / 0 挂、bin 34/0、
  clippy 干净。**资源管理器里「能还原」这一眼留给老板**（回收站里应有「测试包.mp4」）。
### W7 ✅ 回归套件在 Windows 跑全（`tests/all.sh --no-gui`：**35 过 / 0 挂 / 9 明说跳过**）

- **夹具**：造 zip 改 `7z a -tzip`（本机无 `zip`）；`rar`/`unrar` 相关沿用「没有就明说跳过」。
- **修了四个「本机才会撞上」的深坑**（每条都实测复现过）：
  1. **MSYS 的 `[ -x/-e/-f ]` 对无后缀名自动兜底同名 `.exe`** —— `yshjy` 不存在、`yshjy.exe`
     存在时 `[ -x yshjy ]` 也是真，BIN 探测必须**先查 .exe**（三份 python 探针的
     `os.path.exists` 不兜底，之前全靠 Linux ELF 残留「存在」糊弄过去）；
  2. **MSYS2 的 grep 3.0 在 UTF-8 locale 下匹配不了星平面字符**（🔒=U+1F512）——
     `LC_ALL=C` 按字节可解；中文（BMP 内）不受影响；
  3. **Windows 的 7-Zip 按系统代码页（GBK）转码输出里的文件名**（od 验过：`313 265`=说）——
     UTF-8 的 grep 永远匹配不上中文名，改匹配 ASCII 的 `.bin`；
  4. **MSYS 的 `kill -INT` 送不进无控制台的原生子进程**（GenerateConsoleCtrlEvent 无从谈起，
     引擎把包解完才退）—— resume.sh 场景一（优雅取消）在 Windows 明说跳过，
     **硬杀路径（KILL=TerminateProcess）场景二/反证/随机断点全真跑全过**（硬杀后续传 24 项、逐字节一致）。
- **明说跳过的（各带原因，不许假绿）**：pick.sh / progress_multi.sh（python 无 pty）、
  nautilus 右键脚本与只读目录档（Linux 专属 + NTFS 不认 chmod）、自解压 `.sh` 两档（Linux 专属形态）、
  seek_probe 的 `/dev/null` 档、protocol_sfx_linux（.sh 形态）、`script` 交互两态（没装 util-linux）。
- **改动文件**：`tests/{all,smoke,tree_select,errmsgs,decoy,trash_source,resume,pack_progress,cli_spec_match}.sh`、
  `scripts/gen-cli-assets.py`（候选加 .exe）、`tests/seek_probe.py`、`tests/protocol_sfx{,_linux}.py`。
  顺带清掉 `target/{release,debug}/yshjy` 两个 Linux ELF 残留（W10 说过随时可重建——
  它们的旧 mtime 曾骗过 gen-cli-assets 的「二进制比源码旧」检查）。
- **验收**：`bash tests/all.sh --no-gui` **35 过 / 0 挂 / 9 跳**；引擎侧 `cargo test` 354+34 全绿不变。

### W9 ✅ 图形界面在 Windows 落地（flutter build windows 出包 + 托盘 + 关窗口设置）

- **三处写死 Unix 路径改走 platform_paths**（`main.dart` 配置、`home_page.dart` 密码本、
  `engine.dart` 引擎名 → Windows 找 `yshjy.exe`）—— 发布包里引擎跟界面同目录即用（bfb2781）。
- **设置页新增「关窗口时（正忙的话）」**（用户 2026-10-30 拍板）：`tray` 缩到托盘 / `exit` 直接退出，
  **默认分平台且与 Linux 相反**（Windows 默认直接退出、Linux 默认缩托盘）。存 `config.toml`
  的 `[general] close_action`，引擎侧 config.rs 收编该字段（非法值丢弃，单测守着）；
  「直接退出」正忙时先 cancel 引擎、给它 400ms 在数据块边界收尾再放行。
- **Windows 托盘**（C++，`Shell_NotifyIcon`）：实现与 Linux GTK 侧**同一份 `yshjy/tray` 协议**
  （hide_window / set_status / notify / quit_now ← Dart，request_quit → Dart 托盘菜单「退出」
  绕回 Dart 取消引擎再真退），外加气球通知（notify.dart 在 Windows 走这条，不再静默跳过）。
  撤图标/悬停提示/菜单/前台激活都处理了；`OnDestroy` 先撤通道防悬空指针。
- **两个只在 Windows 会撞上的构建坑**（都记进了 CMakeLists/代码注释）：
  1. **MSVC 按 GBK 读无 BOM 源码** —— runner 里出现中文注释/宽字符串后，UTF-8 字节被 GBK
     误配对能把「下一行声明」吞进注释（实测成员集体失踪）；CMake 加 `/utf-8` 解决；
  2. flutter 的 `EncodableValue` 变体里**没有 std::wstring**（MethodChannel 字符串是 UTF-8
     std::string）—— 读出来转宽字符（新增 `utils::Utf8ToWide`）再喂 Win32 API。
- **改动文件**：`gui/lib/{main,app_settings,app_shell,tray,notify}.dart`、
  `gui/lib/pages/home_page.dart`、`gui/lib/pages/settings_page.dart`、
  `gui/windows/runner/{win32_window.h,win32_window.cpp,flutter_window.h,flutter_window.cpp,utils.h,utils.cpp,CMakeLists.txt}`、
  `src/config.rs`（+1 单测）、`gui/analysis_options.yaml`、`gui/pubspec.lock`。
- **验收证据**（2026-10-30 本机实跑）：`flutter build windows --release` 出包（77 秒）；
  flutter analyze 干净（仅 1 条既有 info）、flutter test **43 过 / 3 跳**；引擎引擎 exe 同目录；
  真启动：界面正常渲染（截图核过）、状态目录走 `%LOCALAPPDATA%`、**空闲关窗 → 进程干净退出**；
  `cargo test --lib` **355 过 / 0 挂**（两连跑）、clippy 干净。
  **留给老板肉眼**：拖包进窗口解压/打包、正忙时关窗看托盘、托盘菜单「显示窗口/退出」、
  设置页改「关窗口时」并保存、桌面通知气球。

## [实测记录] · 2026-10-29（实测：磁盘满预检 + 报成功后拔盘丢 300MB，**代码一行未改**）







> 本节只有实测与证据，没有代码改动，**装机版仍是 0.9.31**。
> 完整原始记录见 `~/.hermes/cache/scratch/enospc-unplug-findings.md`。

### 一、环境与做法

- 测试盘：Generic Flash Disk 14.6G（USB 高速口），切 **512MiB exFAT** 分区（可用 510M），
  用 `parted` + `mkfs.exfat` 现做现拆，测完重格回整盘。
- 包：本机 `python zipfile` 的 `ZIP_STORED` 包（20MB × N），内容不影响结论，只为造体积。
- 速率（同一块盘）：`dd` 单大文件顺序写 **6.7 MB/s**（突发）；`yshjy` 解 15×20MB 后 `sync`
  **≈1.5 MB/s**（持续）。廉价盘突发/持续差 4 倍 —— 也是它当系统盘必卡的原因。

### 二、磁盘满预检：3 次全命中，行为正确

| 场景 | 剩余 vs 需要 | 结果 |
|---|---|---|
| 800MB 包 → 510M 分区 | 510M < 800M | ❌ 直接拒，0 字节落地，退出码 1 |
| 300MB 包，另一进程已占 401M | 110M < 300M | ❌ 直接拒，0 字节落地，退出码 1 |
| 两个 300MB 解压并发 | 后者启动时 210M < 300M | A ✅ 成功、B ❌ 直接拒 |

用户可感知的好处：不会解一半才说「空间不够」。
**文案欠一句人话**：`磁盘空间不够（0 B，用时 0.0 秒）` 里的 `0 B` 读不出含义（像「缺口 0 B」），
应改成「需要 X / 只有 Y / 还差 Z」—— 已记进 `docs/TODO.md` 表尾。

### 三、报成功 ≠ 已落盘（差 660 倍）

同一份 300MB 包：

- 屏幕：`✅ mid_300m.zip —— 解开（300.0 MB，用时 0.3 秒）`；实际墙钟 390 ms。
- 解压结束瞬间：`Dirty: 307504 kB`、`Writeback: 0 kB` → **300MB 全在页缓存**。
- 再等 2 秒：`Dirty: 307488 kB`（回写还没启动）。
- 强制 `sync`：180 秒命令超时都没刷完；继续等 45 秒后 `Dirty: 676 kB` 收敛 → 真落盘 **≈200 秒**。

### 四、报成功后立刻拔盘 → 300MB 一个文件不剩

做法：清空 + `sync` → 解压 → **同一条命令里**立刻 `sudo sh -c 'echo 1 >/sys/block/sdb/device/delete'`
（不卸载、不 flush，等同瞬时拔盘）→ 物理拔插一次 → 检查。

- 断电瞬间内核连报 `device offline error, dev sdb, sector NNNN op 0x1:(WRITE)` +
  `Buffer I/O error on dev sdb1 … lost async page write`。
- 重插后设备自动挂载；`fsck.exfat -n`：`/dev/sdb1: clean. directories 1, files 0`。
- **解压目录 `out_d` 不存在，文件数 0，占用 32K** —— 15 个 20MB 文件全灭，程序早已退出码 0。

这次 exFAT 恰好没坏（元数据也没落盘）；**拔晚 20~30 秒会更难看**：半截文件 + 脏标记 + 要 fsck。
风险清单已进 `docs/TODO.md` 表尾（修复方向：解压结束前 `fsync`；验收方式：报 ✅ 即拔后文件应完整）。

### 五、为什么「写到一半才满」在本机造不出来

- 预检准确（见第二节），一启动就拒；
- 页缓存让写入瞬间返回，程序不会真在写盘时被阻塞 → 不会中途报 ENOSPC；
- 要造中途阻塞需一次写超过脏页上限（本机 19.4G 内存，约 3.9G），按 1.5 MB/s 需 40 分钟以上。

### 六、回归

本轮**没改任何代码**（`git status` 干净是改动前状态），因此 `cargo test 388` / `tests/all.sh 41`
的基线不受影响，未重跑。0.9.31 装机版未动。

## [0.9.31] · 2026-10-29（`--pick` 被 Ctrl-C 打断后光标不恢复）

修 `docs/TODO.md` 表尾那条待查项（用户 2026-10-29 点头才动）。

**用户感觉到的变化**：`--pick` 全屏勾选时按 <kbd>Ctrl</kbd>+<kbd>C</kbd> ——
光标不再留在隐藏状态（以前得敲 `reset` 或重开终端），退出码 `130`，屏幕干净；
也不会再冒出一句「❌ 交互勾选出错：IO error: read interrupted」。

### 一、根因是两层（原记录只写了第一层）

1. `dialoguer` 的终端后端是 `console`（不是 crossterm），**它把 Ctrl-C 变成 SIGINT** ——
   上游 `inquire/src/error.rs:27-33` 有原话背书：「If using `console`, pressing Ctrl+C will
   trigger SIGINT.」于是 `dialoguer` 的复原代码（写 `\x1b[?25h` 显光标）根本没有执行机会。
2. **动手才发现**：一旦我们接管了 SIGINT（信号不再杀进程），`dialoguer` 的阻塞读会被
   `EINTR` 打断，`interact_opt()` 返回 `IO error: read interrupted`。不拦的话屏幕上会打一条
   「交互勾选出错」—— 修一条坏一处。所以**主线程那条 `Err` 路也得收**。

### 二、落地（没加新依赖，没走「自己写按键循环」那条备选）

- `src/main.rs` 新增 `装中断接管()`：Ctrl-C **全进程只装一次**、按阶段分派 ——
  `--pick` 在屏 → 复原屏幕 + 退 130；解压中 → 转成引擎「取消」（老行为不变）；
  其余阶段 → 退 130（等价于被 SIGINT 杀掉，shell 看到的都是 130）。
  **顺带收掉一个隐患**：`ctrlc` 的 `set_handler` 全进程只允许成功一次（第二次
  `MultipleHandlers`），原来那句写了 `let _ =` —— 如果 `--pick` 先装过，解压那边的
  取消接管会**静默失效**，退出时留半成品。
- `src/pick.rs`：`在屏中` 标志 + `屏幕守卫`（RAII，提前 return / 出错都会复位）、
  `中断复原()`（只在界面里时写 `\x1b[?25h`，重复写幂等）、`是中断()`（**标志 或 错误文本**
  双保险 —— ctrlc 的等待线程与主线程谁先醒没有保证）。
- 退出码 `130` 统一成 `main::中断退出码` 常量，不再散落字面量。
- **termios 一行没碰**：实测 `console` 的 raw mode 是 RAII 的，被信号打断时自己复原
  （pty 里前后对比 `lflag` / ICANON / ECHO / ISIG 完全一致）。原方案留的「unsafe 存 termios
  快照」因此不需要 —— 少一层 unsafe。

### 三、实测与验收

- `tests/pick.sh` 新增第 ⑤ 组（5 项）：发 `\x03` → 退 130 / 输出流里有 `\x1b[?25h` /
  不出现「交互勾选出错」/ termios 与进屏前一致 / 一个文件都没解。
  该脚本 **11 通过 0 失败**。
- **反向验证**：把 `装中断接管()` 注释掉重编 → 当场红 2 条（`-2`、无 `\x1b[?25h`），
  证明这两条断言真的能抓到原来的 bug。
- **解压中的 Ctrl-C 没变味**（额外实测，2 万碎文件 40 MB 包，解到一半发 SIGINT）：
  退 130、打印「⏹ 收到 Ctrl-C —— 正在收尾（已解出的文件会保留…）」、
  保留已解 **6890** 个文件、残留 `.yshjy-part` **0** 个。
- 回归：`cargo test` **357 + 29 + 2 全绿**；`cargo clippy --all-targets -- -D warnings` 干净；
  `bash tests/all.sh` **41 通过 / 0 失败 / 5 跳过 / 203 秒**
  （日志 `~/.hermes/cache/scratch/all_pick_fix.log`）。
- 文档同步：`docs/CLI-SPEC.md` §4.5 从「Ctrl-C 只是硬中断，光标留在隐藏状态，敲 reset」
  改成「干净退 130」。

## [0.9.30] · 2026-10-29（第三期 B+C：干扰数据/假签名 · 移动端 seek 三档）

> 依据 `docs/THIRD-PHASE-PLAN.md` §B / §C。这一轮把第三期剩下的两条清完，**方案里有两处
> 被实测推翻的假设**（见下面「实测修正」），改的是方案不是代码行为 —— 都记在这里免得
> 以后有人照着旧方案「修回去」。

### 一、干扰数据 / 假签名（`--decoy`）

`src/pack/decoy.rs`（新模块）+ `pack::build_ctx_decoy` + CLI + 协议三处接线。

- 新参数（`src/cli_spec.rs` 唯一真源，补全/man 已重生成）：`--decoy <N>`（段数，0 = 不打，
  最多 64）、`--decoy-size <大小>`（4K–1M，默认 16K）、`--decoy-signatures`（每段夹一枚假特征码，默认关）。
- **位置铁律**：只写在「压缩数据之外」—— L1 在封面之后、ZIP 起点之前；L2 在 `free` 原子内
  pad 之后、ZIP 起点之前。绝不进 MP4 盒子本体（会破坏播放），绝不进压缩流（会破坏解压）。
- **可复现**：随机量全部由 `种子(输入路径, 盐)` 的 FNV-1a 派生，**不看系统时间**；
  `--decoy` 开启时 `free` 原子的随机 pad 也改用稳定种子。实测同一份输入两次打包
  `sha256` 完全相同（和隐写者「每次哈希都变」相反 —— 用户要能校验自己分享的东西）。
- **不破坏默认路径**：`PackOptions` 一个字段都没加（那个结构体在测试里有十几处逐字段
  构造），新增 `build_ctx_decoy`，`build_ctx` 转调它并传空选项。
- 打包流程内置自检新增两条断言：ZIP 起点必须等于写出来的预期偏移；起点必须大于噪声总量
  （否则说明噪声写进了压缩数据）。任一不过 → 自检判失败（CLI 退出码 2）。
- 互斥都**明确报错**不静默：`--sfx` / `--sfx-linux` 靠写死偏移找数据，插噪声会弄坏；
  普通压缩（标准 ZIP）前面塞垃圾会让别人的 `unzip` 报警告 —— 协议侧同样拒绝。

**实测修正 ①：方案写的「噪声总长 ≤ 外壳大小的 10%」不可行。**
内置封面 `mini.mp4` 只有 **1688 字节**，10% = 168 字节，连一段（最小 4 KiB）都放不下。
实测（`~/.hermes/cache/scratch/decoy_probe`）噪声长度对 `file` 判定、7-Zip / unzip 列目录
**完全没有影响**（16 KiB → 1 MiB 全通过，`file` 一直认成 `ISO Media, MP4`），
故改成绝对上限 **1 MiB**（够糊扫描器，又不会让产物以噪声为主）。

**实测修正 ②：假签名后面「跟随机字节」会随机性地骗坏 7-Zip。**
方案 §B 决策 4 要求「后面跟随机字节（不是零）」。五种填充 × 20 个种子 × 2 种长度的实测：

| 段内填充 | 7-Zip 列不出内容 / 40 |
|---|---|
| 全零 | 0 |
| **签名 + 30 字节零 + 其余随机**（最终采用） | **0** |
| 签名 + 40 字节零 + 其余随机 | 0 |
| 签名 + 64 字节随机、其余零 | **17** |

结论：签名后面紧跟随机字节时，7-Zip 会把它当成真的 ZIP 起点 → 整个文件列不出内容
（unzip 和我们自己的引擎不受影响）。30 个零正好是一个「匿名空条目」local header 的形状，
7-Zip 会确定性地跳过。**`src/pack/decoy.rs` 模块头写了这张表，别改回纯随机。**

验收：`tests/decoy.sh` **14 项全绿**（挂进 `all.sh`）：对照组、可复现、ZIP 起点在噪声之后、
7-Zip / unzip 能列、`file` 仍认 MP4、`--test` 通过、解出内容与无干扰版**逐文件 sha256 一致**、
假特征码真的在里面、L1 布局同样通过、三条错误路径退出码 2。
**反向验证**：把引擎里 `FreeAtom` 的写噪声挪到拷贝 ZIP 之后 → 当场 12 通过 / 2 失败，改回全绿。
单元测试 8 条（默认值、越界、种子稳定性、逐字节可复现、签名位置、关签名后零命中、总量、不打时零写）。

### 二、移动端 seek 三档（探测 + 块缓存 + 协议字段）

`docs/ARCHITECTURE.md` F1 的三档方案，本轮**只做前三件**，落地验收留 Android 批。

- `src/seek.rs`：`SeekAbility { Full, BlockCache, CopyFirst }` + `seek_ability(path)` 探测。
  判定：普通文件试 `seek(End(0))` → `Start(0)`，都成功 = `Full`；seek 失败 = `BlockCache`；
  FIFO / 字符设备 / socket = `CopyFirst`（流不能回退，重读只能靠复制）。
  **打不开的普通文件返回 `Full`** —— 那是「文件不存在」，不该伪装成「不能跳着读」。
- `src/io/blockcache.rs`：`BlockCache<R>` 实现 `Read + Seek`，32 KiB 块 / 8 MiB LRU。
  往前跳顺序推进底层流；**往回跳到已被淘汰的块时明确返回 `Unsupported`**（错误里直接写
  「需要先复制一份本地副本」），绝不静默返回 EOF —— 那会让解压少解几个文件还不报错。
- `src/seek.rs` 的 `复制到本地`：`CopyFirst` 档的兜底，**先 `safety::check_space` 预检**，
  不够就报 `NotEnoughSpace`（带字节数，句子归外壳）；1 MiB 块流式复制，抄完核对字节数，
  不完整就删掉副本报 `Truncated`。
- 协议：`hello.caps.seek_probe = true`；`scan` 的每个候选加 `seek`（full / block_cache /
  copy_first）与 `seek_label`（展示用，不参与机器判断）。**只加不删**，协议版本不升。

验收：`src/seek.rs` **6 条** + `src/io/blockcache.rs` **7 条**单测（随机 seek+read 300 次与
「整读进 Vec 再切」逐字节一致、12 MiB 走过 8 MiB 缓存真的淘汰块、回退到淘汰块报错、
未知长度不能 `SeekFrom::End`、`with_len` 截断多余字节、默认参数、上限小于一块按一块算）。
`tests/seek_probe.py` **7 项**（hello 能力位、老前端能力位没丢、每个候选带 seek、取值只三档、
普通文件 full、`/dev/null` copy_first、label 非空）。

**踩到的坑（记档）**：探针一开始把 FIFO 塞进 `scan` 的 paths —— 引擎对每个候选都做
`job::identify`，那一步会 `open` FIFO，没有写端就**永久阻塞**（探针 120 秒超时）。
FIFO 的判定改由单测覆盖（只调探测函数，不打开文件）。真实用户不会拿 FIFO 去扫。

**没做的事（不冒充）**：本机没有 Android 环境（Wayland 桌面 + 无 SDK 设备），
「SAF 的 `content://` 到底算哪一档」**没有验证**。落地验收（SAF 走哪档、复制兜底的预检数字）
已写进 `docs/TODO.md` 的平台节。

## [0.9.30] · 2026-10-29（第三期 A：预览树 + 部分解压）

> 依据 `docs/THIRD-PHASE-PLAN.md` §A。**引擎一行没改**：条目序号 `index` 从预览到解压
> 全程不变（`src/select.rs:8`），选择性解压早就能用，缺的是界面怎么让用户挑。
> 竞品范本是 PeaZip 的 smart treeview + 面包屑 + 状态栏（`refs/analysis/07-竞品界面调研.md:616-631`）。

### 一、树：平铺清单 → 目录树（新增 `gui/lib/entry_tree.dart`）

树**在前端算**，不放引擎：引擎只给「带完整路径的平铺清单」，树是纯展示派生数据；
写成纯 Dart（不 import Flutter）就能直接 `flutter test`，改样式也不用动协议。

- 节点字段：`name / path / level / isDir / index / size / packed / fileCount /
  encryptedCount / children`；目录的 `size`、`packed`、`fileCount` 全是**子树合计**。
- `packed` 只要有一个子项未知就整体记 `null` —— **不拿 0 冒充「压得很小」**（测试钉住）。
- 归档里的目录条目带尾斜杠（`子目录/`，真二进制 `--list --json` 实测），统一剥掉再聚合；
  同名中间目录用 `putIfAbsent` 合并成一个节点，不重复造。
- 空目录条目（归档里真有、底下没东西）**显示但不可勾**：没有内容可解。
- 另给 `flattenTree`（摊平成一行行给 ListView）、`breadcrumbs`、`fileIndexesOf`、
  `pickStateOf` / `applyPick`（三态）、`allDirPaths`。

### 二、界面：三态勾选 / 面包屑 / 状态栏 / 列标题

- **三态**：目录全选 / 半选（横线）/ 未选；勾目录 = 勾整棵子树；取消任一子项父变半选；
  **半选时点一下 = 全选**（半选状态下用户点它的直觉是「我要全要」）。送引擎的仍只有
  文件序号，目录不参与 `select`。
- **状态栏**一行常驻 `已选 N / M 个文件 · 合计 X`，按**文件条数**算不按目录条数算。
- **面包屑**：点哪一级只显示那一层的下一级（真过滤，不是滚动定位）。补了一个方案里没写、
  但真机会卡住的入口 —— 目录行右侧「只看这一层」小箭头：原来只能从面包屑**往上回**，
  没有任何办法**往下进**（点目录名是展开/收起）。截图验收时发现的。
- **列标题**「大小 / 压缩后」：两个数字光秃秃并排会看不懂哪个是压缩后（真机截图发现的）。
- 树 / 平铺两档可切，默认树；平铺把条目包成「无子项的单层节点」走同一套渲染，
  免得两套渲染各自长歪、状态栏数字还不一致。

### 三、压缩后大小：引擎早就发了，是界面没解析

查证结论：`EntryView.packed` 一直在（`src/event.rs:112`），`api.rs` 把 `entries` 原样
序列化，真二进制 `--list --json` 输出的每一条都带 `"packed":6`。缺的只是 Dart 侧
`EntryItem` 没有这个字段 —— 补上解析即可，**不改协议、不升版本**。
（记一笔：`EntryItem.packed` 是 `int?`，裸流等未知情况是 `null`，界面显示空白不是 0 B。）

### 四、测试与验收

- `gui/test/entry_tree_test.dart` **19 条**：空包 / 单层 / 深嵌套 / 同名中间目录 / 加密计数 /
  空目录不可勾 / packed 未知，加三态 5 条、面包屑与过滤 6 条、真实包形状 1 条。
- `gui/test/preview_tree_widget_test.dart` **7 条**（真正点界面）：默认全选 / 点目录取消整棵 /
  只勾一个看半选 / 半选点父变全选 / 点文件行本身也能勾 / 面包屑下钻与回根 / 空目录无复选框。
  为此给 `HomePage` 加了 `debugPreview` 注入口（生产代码永传 null，和 `autoStart` 同类）——
  真机手点验不了这套状态机（实测一次点击把「已选 1001」点成「已选 1」，红绿说不清是手滑还是 bug）。
- `tests/tree_select.sh` **7 项**（挂进 `all.sh`）：界面勾目录 = 传一串序号，
  验「序号串」与「等价通配表达式」产物**逐字节一致**、只勾一个不多解、挑出来的文件与全解
  版本 sha256 相同、空目录不被解成文件。**反向验证**：故意把序号串改成只一个 → 当场红 2 条。
- 回归：`flutter test` **46 条全绿**（必须给隔离的 `XDG_CONFIG_HOME`，`app_settings_test`
  自带安全闸拒绝跑在真配置目录上）；`flutter analyze` 仅 2 条既有 info。
  `bash tests/all.sh` 结果见 `~/.hermes/cache/scratch/all_tree.log`。
- 真机实测 **1026 条目的包**（1001 文件 + 25 目录）：树形缩进、目录行「N 个文件 + 合计大小」、
  列标题、面包屑、状态栏数字全部正常，无重叠/截断（截图 + 目视核对）。
- 踩坑记档：widget 测试默认视口 800×600，预览页头部（摘要/勾选卡/状态栏/面包屑/列标题）
  一挤，列表只剩一两行，而 **ListView 看不见的行不会被 build** → `find.text('另一个')` 报
  「找不到」，看着像功能坏了。测试里把视口设成 1200×2000 才稳。

## [0.9.30] · 2026-10-29（表尾待查三条收掉 + 新能力 `--test` 只测不写）

> 用户口径「先 1 后 2」：先把 `docs/TODO.md` 表尾「待查/改进」那批小条目清掉，再做第三期结构优化。
> 本轮清掉表尾 4 条（1 个真 bug + 1 个新能力 + 2 个对账探针），`--pick` 光标那条按用户 2026-10-28
> 的口径**仍然不做**（记在原处待以后）。

### 一、bash 补全：带空格的文件名被劈成两个候选（真 bug，已修）

`scripts/completions/yshjy.bash` 的 fallback 原本是 `COMPREPLY=($(compgen -f -- "$cur"))`。
命令替换的结果会**再按默认 IFS 分词**，于是 `有 空格.zip` 变成 `有` + `空格.zip` 两个候选。
没装 `bash-completion` 包时必现（装了才走 `_filedir` 躲过去），所以只在部分机器上暴露。
文件、目录、末尾兜底三处同一族问题一起改（`gen-cli-assets.py` 的三处锚点），
生成物里统一成 `mapfile -t COMPREPLY < <(compgen …)`。

**实测排除掉的另一种写法**：`IFS=$'\n' COMPREPLY=($(compgen …))` 这种前缀赋值虽然不劈词，
但会把**全局 IFS 改掉并泄漏到函数返回之后** —— 紧接着 `set -- $(echo "a b")` 只得到 1 个词，
等于把用户交互 shell 里后续命令的分词全搞坏。`local IFS=$'\n'` 才安全，但那要在函数里多一层。
`mapfile` 最省事且无副作用，选它。

新探针 `tests/completions.sh` **11 项**：真在 bash 子进程里摆好 `COMP_WORDS`/`COMP_CWORD`、
调用补全函数、断言 `COMPREPLY`；覆盖「装了/没装 bash-completion」「带空格文件名」「带空格目录」
「`--password` 等位置仍然零候选」「IFS 没被污染」「值候选前缀过滤」「complete 注册项」
「不改调用者的 COMP_WORDS」。**反向验证**：拿 `git HEAD` 的旧补全脚本跑，正好红在劈词/目录/注册三条。

### 二、`--test`：只测不写（完整解码 + 校验，一个字节都不落盘）

对标 7-Zip 的 `t` 与 Ark 插件元数据的 `SupportsTesting`（`refs/tech/ark-fileroller.md` §2）。
**不解字节到磁盘**，所以磁盘满时照样能查包、查完不留任何目录。

- **开关收在 IO 层**：`io/atomic.rs` 的 `PartFile` 加 sink 模式（`create_with(path, verify_only)`），
  写进空设备（`/dev/null` / Windows `NUL`），`finish` / `rename_to` 不改名、`Drop` 绝不 remove 空设备。
  七条落盘路径（ZIP / 7z / 单流 / xz / tar / 救援 / RAR）**解码逻辑一行没动**，只把「写去哪」换了。
- **配套三类动作在只测模式下跳过**：不建输出目录（12 处）、不还原时间戳/权限（2 处）、
  **不做磁盘空间预检**（不解字节，磁盘够不够与这次无关）。
- **断点续传必须停用**：开着续传时「上次解过的条目直接跳过」会让 `--test` 什么都没测到，
  而且会往用户配置目录里写记录。
- **与「处置原包」互斥**：`--test` + `--trash-source`/`--delete-source` 直接用法错误（退出码 2）——
  只测不写时没有产物可以对照，删原包等于让用户丢掉唯一的材料。
- 成功文案改成「校验通过」而不是「解开」，并且不打印产物目录（用户会去找不存在的东西）。
- CLI `--test` 进了 `src/cli_spec.rs`（长选项 42 → **43**，钉桩测试同步更新），
  `--help` / man 页 / 三份补全由生成器自动跟上；协议 `extract` 与 `batch` 都接受 `verify_only`。
- 7 个语言包补 3 条：`cli.test_mode` / `cli.test_source_conflict` / `result.phrase.verified`，
  `--help` 各语言各加一行说明。

**验收**：`tests/verify_only.sh` **8 项全绿** —— 好包零产物且不建目录、数据损坏的包退出码非 0
（证明校验真在跑）、互斥拒绝、源包不动、`--json` 也零产物、tar.gz（tar + 单流两条路径）同样零产物，
另加两条**正面对照**（同一个包不加 `--test` 能正常解出文件，证明样本与路径都是好的）。

### 三、两个「第二份真源」对账探针（同 man 页 / 补全漂移那类病）

- `tests/ffi-header-sync.sh` **4 项**：`ffi/include/yshjy.h`（手写）与 `ffi/src/lib.rs` 的
  `extern "C"` 函数集合**双向** diff（实现有头文件没有 = 宿主调不到；头文件有实现没有 = 链接会炸）、
  ABI 版本宏与常量一致、头文件函数都带 `yshjy_` 前缀、`ffi/Cargo.toml` 版本与主线一致。
  反向验证：临时删掉头文件里一行声明，探针当场变红。
- **顺带抓到的真漂移**：`ffi/Cargo.toml` 的 version 停在 **0.9.20**，主线已 0.9.29 ——
  规则 16 的「五处版本号」清单漏了这第六处；已改 0.9.29（`ffi/Cargo.lock` 同步，`cargo check` 通过）
  并把这条做成探针断言，以后发版漏改会红。

### 四、`complete` 补齐 `-o nosort -o bashdefault -o default`

`-o nosort` 要 bash ≥ 4.4（老版本会把 nosort 当非法选项直接报错），所以生成器里加了版本判断。
`-o default` 会让「空 COMPREPLY」回落到 readline 的文件名补全 —— 那会毁掉
「`--password`/`--select`/`--split`/`--level` 故意不给候选」的设计，所以在那些分支里
`compopt +o default` 显式关掉回落（探针 4 项盯着这个行为）。

### 五、7z 头加密（`-mhe=on`）：说清「连清单都加密了」，并修掉一个真 bug

头加密的包**连文件清单都读不到**，之前一律报「这个包带锁，需要解压码」——用户会以为
「我给了密码怎么还不行」或者「给了密码就能列」。现在拆成独立错误码 `HEAD_ENCRYPTED`：

- **无密码 + 读头失败** → `HEAD_ENCRYPTED`（「必须先给解压码，否则连里面有几个文件都看不到」）；
- **给了密码仍读不出头** → `WRONG_PASSWORD`（「解压码不对」，别把「给错了」说成「没给」）；
- **内容加密（清单可读）** → 维持原样（能列清单 + 标出 N 个带锁条目），不许误报成头加密。

**顺带修掉的真 bug**：`main.rs` 的 `do_list` 调用 `preview(&path, &ctx, None)` 时把密码写死成
`None`，于是 `yshjy --list -p 密码 头加密.7z` **永远**报「需要解压码」—— 用户手里的密码
根本没被用上（只有 `--list` 这条路径如此，解压路径一直是对的）。

新码同步了四处（`exit_code` / `cli_exit_code` / `as_str` 穷尽 match + 7 个语言包的
`error.head_encrypted`）+ 界面码表 `gui/lib/humanize.dart`。
探针 `tests/head_encrypted.sh` **5 项全绿**：无密码→HEAD_ENCRYPTED、**正确密码→能列清单**（bug 回归）、
错误密码→WRONG_PASSWORD、内容加密→不误报、正确密码→真解出文件。没装 7z 时自动跳过。

### 六、`caps` 按格式声明（协议 `hello.formats`）+ 界面不再 spawn 子进程

TO-DO 原文是「`caps` 按格式声明（Ark 的做法，协议面升版，影响 GUI + FFI 两侧）」。
查下来缺的不是能力位本身（`InArchive::caps()` 一直是按 handler 返回的），而是**协议面**：
`hello.caps` 只有全局五项，前端想知道「这个格式能不能选条目」只能 spawn 一个子进程跑 `--caps`，
而 **iOS 禁止 fork/exec** —— 手机端那条路物理上跑不了。

- **格式清单升成引擎里的唯一真源**：`unpack::能力组合` + `unpack::能力行()`。
  CLI `--caps` 与协议 `hello.formats` 读同一份（`main.rs` 里原来那份硬编码组合清单删了）。
  这是本项目第 4 次治「手抄第二份真源」（前三次：man 页版本号、bash 补全选项、FFI 头文件）。
- **协议加 `hello.formats`**：12 条，每条 `format` + `shape` + 6 个能力位。
  `format` / `shape` 都是**机器标识**（`zip` / `rar5` / `volumes` / `single`…），展示名归前端。
  **只加不删 → 协议版本不升**；老前端不认识 `formats` 就忽略，行为不变。
- **界面改成走传输层**：设置页「各格式能力」原来 `Process.run(--caps --json)`，
  现在是 `EngineClient.instance.hello()` 读 `formats`（老引擎没有就如实说「版本太旧」）。
  这一改同时解决了 iOS 端根本读不出能力表的问题。
- `--caps --json` 的输出多一个 `shape` 字段、`format` 变成机器标识（原来混着「zip（分卷）」这种展示名）。

**验收**：`tests/caps_protocol.py` **5 项全绿** —— 协议与 CLI 逐字段一致（同源）、
zip 能选条目而 gzip 不能（证明能力真按格式）、7z 声明头加密而 zip 不声明、标识全是 ASCII 机器名、
全局 `caps` 五项仍在（老前端兼容）。`flutter analyze` 只剩两条既有 info。

### 七、打包：补上 `prerm`（卸载前钩子）

`build-deb.sh` 一直只有 `postinst` + `postrm`，缺 `prerm`。症状不是装不上，而是**卸载不干净**：

- 界面「关窗口 = 藏到托盘」，所以卸载后托盘里可能还挂着一个**已经被删掉的进程**；
- `postinst` 给用户设过 14 种格式的「双击用我打开」，卸载后用户配置里留着这些条目
  （GNOME 会兜底，但重装或换路径时会莫名其妙）。

`prerm` 的两条纪律：

- **只在 `remove|purge` 时动手**；`dpkg` 在升级时传的是 `prerm upgrade <新版本>`，直接早退 ——
  用户可能正让它在解一个 8 GB 的包，升级把他的活杀掉是不可接受的（这条做成了 `build-deb.sh`
  与 `test-packages.sh` 的断言：prerm 里没有 `upgrade` 字样就拒绝出包）。
- 停进程用 **TERM 不用 KILL**：强杀会让正在解压的包留下 `.yshjy-part`；卸载软件的副作用
  不该包括「砸坏他正在做的事」。清 mime 配置前先备份成 `mimeapps.list.yshjy-uninstall.bak`，
  写不动就算了（卸载本身不该因为这个失败）。

验收：`bash scripts/build-deb.sh` 自查通过（含新增的「prerm 在 / 755 / 语法 / 区分 upgrade」四条），
`bash scripts/test-packages.sh` **37 项全绿**（原 33 项 + prerm 4 项）；从包里取出 prerm 真跑了
`upgrade` / `remove` / 空参数三种调用，都是 0 退出、无副作用。
顺带按铁律重编了界面（`flutter build linux --release`）再出包 —— 因为本轮改过
`settings_page.dart`，不重编的话包里会是旧界面。

### 八、回归

`bash tests/all.sh` —— **35 通过 / 0 失败 / 5 跳过 / 约 185 秒**
（第一次跑红在 `cargo test --release` 的「长选项总数」钉桩：42 → 43，正是它该拦的）；`cargo test` 336+29+2、`cargo clippy -D warnings` 干净。

## [0.9.29] · 2026-10-29（引擎：条目 packSize 校验 —— 多写不落盘、少写报截断）

> 这条在台账里挂了一个月，语义出自 libarchive 的 `entry_bytes_declared`
> （`refs/tech/libarchive.md` §4）：**不是拒绝解压，而是拿头部声明的解压后大小给输出封顶**。

### 一、zip：从「写完再比对」改成「边写边封顶」

原来的写法是先把压缩流全读出来写盘、写完再拿 `file.size()` 比对。问题是：声明 1 KB、
实际吐 10 GB 的坏包/恶意包，**磁盘先被写满再报错** —— budget 只管总量，管不到「这一条
明显在超发」。现在在写循环里按声明值算「这一块允许写多少」，超出的字节直接丢弃并判数据
错误，半成品照旧由 `.yshjy-part` 的 Drop 删掉。

- 声明为 0 的老式「数据描述符」包**不封顶**：那种包头里本来就没有可信大小，硬封会让
  正常包解不出来，只能靠 budget 兜底；
- 「少写」的判定保留（原来那条 `written != expected`），现在注释写明它只管少写。

### 二、7z：`entry.size()` 本来就是声明值，之前没人用

`decompress_with_extract_fn_and_password` 的回调里 `entry` 带 `size()`。补上和 zip 同款的
封顶 + 少读判错（`Ok(0)` 时 `written != declared` → 该条判数据错误）。

### 三、RAR 与裸流：拿不到可信声明值，如实不做

- **RAR**：`rars 0.11` 的回调结构 `ExtractedEntryMeta` 字段是 `name_is_unicode / name /
  file_time / file_attr / attr_source / mtime_refinement / is_directory`
  （`lib.rs:328-344` 逐个核过），**没有 unpacked_size**。绕道用 `members()` 预扫描建
  「名字 → 大小」表也能做，但 RAR 允许同名条目，错配会把好包误判成损坏 —— 风险大于收益。
  RAR 侧仍有库自身的 CRC 校验 + budget 总量限额。
- **裸流（gzip/xz/zstd/bzip2）**：格式头里没有解压后大小；gzip 的 ISIZE 在末尾且 >4 GB 取模，
  不可信。tar 家族走 tar crate 自己的 size，不在这条范围内。

### 四、验收

`tests/pack_size_cap.sh` **10 项**（zip 正常 / 多写→不落盘 + 人话原因 / 少写→报「声明 1000、
实际不足」/ 7z 正常 / 7z 截短→不许成功 / rar 正常回归）；fuzz 写盘路径 60 秒零崩溃；
`bash tests/all.sh` 见 `scratch/all14.log`。

## [0.9.29] · 2026-10-29（第二期收尾：原包处置 / 右键脚本 / 补全 + 两条「查出来不用做」）

> 第二期原计划 7 条。做完 4 条真活，另外 2 条**核实后发现根本不用做**（一条早做好了、
> 一条与项目的安全设计冲突），剩下 1 条（packSize 校验）决定单独一轮做 —— 理由见下。

### 一、CLI 新增 `--trash-source` / `--delete-source`（唯一不可逆的一类功能）

选项进唯一真源 `src/cli_spec.rs`（长选项 40 → 42，护栏单测 `长选项总数是_42` 同步改），
`main.rs` 新增 `源包处置` 三态（不动 / 回收站 / 永久删除），两条都只在**成功 / 部分成功**时
动原包 —— 失败时原包是用户重试的唯一材料，与 `job.rs` 里 `on_done == "trash"` 同一口径。

`--delete-source` 的处理是本条的重点：

- **开工前**问一次（只认 `y` / `yes`），不是解完再问 —— 解完再问，用户拒绝就白解了；
- 非交互（`--quiet` / `--json` / stdin 不是终端）**直接拒绝、退出码 2**，并提示改用
  `--trash-source`。破坏性动作绝不能在人没看着的时候静默通过；
- 命令行优先于配置文件里的 `output.on_done`（那是长期偏好，这是「这一次」的口子）。

7 种语言文案全同步（4 条运行时 + 2 行 `--help`），补全与 man 页重新生成。
探针 `tests/trash_source.sh` **10 项**：回收站里真有 `files/` + `info/`（可还原）、
非交互拒绝且原包不动、输 `y` 真删、输 `n` 取消。用 `script -qec` 造 pty 才能走到交互分支。

### 二、右键脚本：只读目录不再静默失败（方案与原计划不同）

原计划「抄 FileRoller：能写→解压到此处，不能写→只给解压到…」。实测**做不到**：
Nautilus script 的菜单文字就是脚本文件名，没法按权限动态变；真正的动态菜单要写
Nautilus 扩展（C/Python + 编译安装），是另一个量级的工程。

退一步做等效的事：能写 → 原地解（原行为不变）；不能写（U 盘、`/mnt`、别人的目录）→
解到「下载/解压-<包名>」，通知里写清「原文件夹不能写，已解到下载」。
探针 `tests/nautilus_script.sh` **7 项**（假 `notify-send` 拦通知，别真弹给用户）。

### 三、补全两处（抄 clap）

- zsh：目录补全 `_directories` → `_files -/`（clap 生成器的写法，跨补全主题更稳）；
- fish：目录补全 `-r -F` → `-r -a "(__fish_complete_directories)"`。
  **坑**：必须双引号 —— 单引号里 fish 不执行命令替换，写进去就是字面量。

### 四、两条「查出来不用做」（台账失真，别再排期）

1. **界面「解完把原包移回收站」开关**：`settings_page.dart:265-283` 早就有下拉（读 `:65` /
   写 `:155` / 副标题 `:279`），引擎 `config.rs:244-246` 解析、`job.rs:845-855` 执行。齐了。
2. **界面密码卡「这次不保存」勾选**：前提不成立。`config.rs` 里有一条保险丝测试
   `读密码本绝不改动文件本身`，注释写明「密码本只允许**用户主动**写」，也就是说界面现在
   **根本不保存任何密码**（`_recentPw` 只是只读 chips）—— 「这次不保存」这个布尔等于
   「永远不保存」，做了就是个假开关。真要「记住密码」是新功能，得先定存不存明文、
   怎么让用户看见并删掉，等用户点头再排期。

### 五、packSize 校验：单独一轮（不是不做）

要动 `zip.rs` / `extract_stream.rs` 的解码主循环，属于「会写坏文件」的一类改动，
必须配 fuzz 写盘路径（`--slow` 十分钟）+ 全回归。和别的条目挤在同一轮里，
出了问题分不清是谁的锅。

**验收**：`cargo test` 336 + 29 + 2 全过；`clippy -D warnings` 干净；
`cli_spec_match.sh` 7 项、`trash_source.sh` 10 项、`nautilus_script.sh` 7 项全绿；
`bash tests/all.sh` 见 `scratch/all13.log`。

## [0.9.29] · 2026-10-29（界面：窗口尺寸记忆真的生效了 —— 挖出两个 bug）

> 用户口径是「每次开都要手动拉窗口」。查下来不是「没做」，是**做了但从来没生效**，
> 而且生效路径上还埋着一个会「越开越大」的基准错位。引擎一行没改，只动界面原生层。

### 一、保存挂在 `destroy` 上，而界面根本不走 destroy

`gui-window.txt` 的读写代码 0.9.26 那批就写进了 `gui/linux/runner/my_application.cc`，
但用户用了一个月，`~/.local/state/yshjy/gui-window.txt` **一次都没生成**（目录里只有
`crash/` 和 `gui-history.json`）。原因不是代码写错，是**这条路的入口没了**：

- 正在解压时点关闭 → `didRequestAppExit` 返回 `cancel`，走 `Tray.hideWindow()`（藏到托盘，不销毁）
- 托盘「退出」→ Dart 收拾完 → `quit_now` → `g_application_quit`（不保证发 `destroy`）

两条主路都绕开了 `destroy` 信号，保存函数形同虚设。

**修法**：把「写盘」从 `on_destroy_save_size` 里抽成 `save_window_size()`，改成
`on_configure` / `on_state_track` 之后 **800ms 防抖写盘**（拖动时 configure 一秒几十次，
不能每帧写文件），`destroy` 时保留一次兜底 flush。定时器句柄存进 `WindowSizeState`，
`destroy` 里先 `g_source_remove` 再 free —— 否则定时器会在 state 释放后拿到野指针。

### 二、存的是「含 CSD 装饰的 surface 尺寸」，载入却当内容区 → 越开越宽

第一版改完跑实测，立刻看到雪球：

```
预置 880 700 → 启动后存回 992 849   （宽 +112、高 +144）
预置 992 849 → 启动后存回 1104 849  （宽又 +112）
```

根因：Wayland 下 `configure-event` 给的 `event->width/height` 是**含 CSD 装饰/阴影的
surface 尺寸**，而 `gtk_window_set_default_size` 吃的是**内容区尺寸** —— 存进去再读出来
喂回去，一轮加一圈。修法：改用 `gtk_window_get_size()`（GTK 文档明确「不含装饰」，与
`set_default_size` 同基准），并把 surface 与 content 的差值一起打进 trace，方便以后换主题
/换桌面时一眼看出基准又漂了。

**试过的错路（别重复）**：先怀疑是 Flutter 的 `FlView` 撑大窗口，打 `child-request` 出来是
`0x0`，排除；又想给恢复的宽度留 140px 装饰余量（`decor_margin`），实测把 1254 clamp 到
1140 后窗口**照样**被合成器最大化 —— 触发条件是「窗口几个边都贴着工作区」的启发式，
不是单纯的宽度比较，索性回退这两处，保持代码最小。

### 三、探针：只能靠界面里的 trace 打点（几条外部观测路全部堵死）

- Wayland 协议不许客户端查询别人的窗口几何；
- GNOME 48 的 `org.gnome.Shell.Screenshot` 对非 shell 进程回 `AccessDenied`（实测）；
- 本机没装 `xdotool`/`wmctrl`，装了在 Wayland 会话里也拿不到窗口；
- `GDK_BACKEND=x11` 在本机没有 X11 会话（`cannot open display`）。

所以界面里加了 `YSHJY_WINDOW_TRACE=1` → stderr 三行打点
（`load raw WxH -> apply WxH` / `configure surface … content …` / `save WxH`），
新探针 `tests/gui_window_state.sh` 断言这条线，**8 项**：读回原尺寸、随手落盘、
**再开一次尺寸不变（不滚雪球）**、超大值 clamp、clamp 后写回、坏文件走默认比例、
坏文件被覆盖、无 Gtk-CRITICAL。

探针默认**不跑**（`all.sh` 里是 skip）：它会真开界面窗口，桌面闪几下 —— 用户明确讨厌
回归时往桌面弹窗（被文件管理器弹窗抓到过两次）。要跑：`YSHJY_GUI_VISIBLE_TEST=1`。
等待方式也从固定 sleep 改成「轮询到 `window: save` 出现」：debug 构建首帧要 6 秒上下，
固定 4 秒会拿到空日志误判（第一版就是这么假红的）。

### 四、位置记忆：Wayland 下做不了，如实记下来

`loginctl` 显示本机会话 `Type=wayland`。Wayland 协议**不允许客户端给自己定位窗口**
（`gtk_window_move` 无效），所以「记住窗口位置」在这是不可能完成的需求，不是没做。
X11 下可做，但本机没有 X11 会话可验收，不做。

`window_manager`（MIT / 1126 赞 / 30 天下载 89 万 / 0.5.2 活跃）评估过、**不引**：
它同样是在 GTK 侧调 `gtk_window_move`/`set_default_size`，Wayland 下一样做不了位置，
接进来只是给已有的实现再加一份真源。

**验收**：`tests/gui_window_state.sh` 8/8；`flutter test` 20 条；`flutter analyze` 只剩 2 条
既有 info；`bash tests/all.sh` 见 `scratch/all12.log`。

## [0.9.29] · 2026-10-29（测试基建：探针该用哪档二进制 + 自解压探针会弹文件管理器）

> 引擎代码**一行没改**，只改 `tests/`。两件事都是被同一类症状骗过之后挖出来的：
> 一个是「看着像偶发、其实是确定性」，一个是「跑测试会弹出用户的文件管理器」。

### 一、探针用 debug 而 `all.sh` 只建 release → 假红，且被误记成「偶发失败」

**现场**：`docs/TODO.md` 表尾一直挂着「`cargo test --release` 在 all.sh 里偶发失败」。
2026-10-29 复跑整轮当场失败 1 项，日志只有一行：

```
⚠️  源码比 target/debug/yshjy 新 —— 先 cargo build，否则测的是旧行为
```

**根因（确定性，不是 I/O 也不是 OOM）**：`tests/rar_atomic.sh` / `pack_progress.sh` /
`resume.sh` 的默认 `BIN` 是 `target/debug/yshjy`，而 `tests/all.sh` 第 1 关**只跑
`cargo build --release`，从不建 debug**。于是「改过 `Cargo.toml` 或 `src/` 但没手动
`cargo build`」时必然红、没改过时绿 —— 昨天 all7 红 / all8 绿，看着就像偶发
（实测时间戳：`Cargo.toml` 22:31 比 debug 二进制 22:26 新 5 分钟，正是改 0.9.28 号那一下）。
探针自带的那道自检是对的，它拦住了「拿旧二进制测出旧结论」。

**试过的错路（记下来别再走）**：先把三个探针改成 release，想着「测发布产物更真实、还能少编一档」。
实测**不可行**，因为这三个探针的判定都建立在「debug 慢」上：

- `rar_atomic`：中断点 = 完整解耗时 ÷ 3；release 解 81 MB 只要 **151 ms** → 0.15 秒硬杀
  打不中，进程早就正常退出了，2 项断言当场假红
- `pack_progress`：release 下 80 MB 只发得出 **1 条**进度事件 → 3 项断言全红

**最终修法**：`all.sh` 第 1 关补一条 `cargo build --quiet`（debug，增量约 8 秒、无改动时近 0 秒），
三个探针保持 debug，并在文件头写明「为什么必须是 debug」+「all.sh 两档都会建」。

**验收**：`bash tests/all.sh` → **30 通过 / 0 失败 / 4 跳过 / 188 秒**；
三个探针在 debug 下单独跑也全绿（`rar_atomic` 7 项、`pack_progress` 7 项、`resume` 15 项）。

### 二、`pack_progress.sh` 跑一次就弹一次文件管理器（用户当场抓到两次）

**现场**：跑整轮回归时桌面弹出文件管理器窗口，用户问「你刚刚跑的这个为啥打开了我的文件管理器」，
紧接着又弹一个。journal 抓到铁证：

```
dbus-daemon: Activating service name='org.gnome.Nautilus' requested by ':1.30376'
             (uid=1000 pid=1983534 comm="gio open ????????")
```

**根因**：`src/pack/mod.rs` 生成的 Linux 自解压存根，解完默认会 `xdg-open "$TARGET"` 打开目标目录
（给真人用的贴心行为，开关是 `YSHJY_SFX_NO_OPEN=1`）。`pack_progress.sh` 为了验证「产物没写坏」
会真执行那个 `.sh`，但**没设这个开关** —— 其它四个 sfx 探针（`protocol_sfx.py` /
`protocol_sfx_linux.py` / `sfx_stub_test.py`）早就设了，只有这一处漏了。

**修法**：执行处加 `YSHJY_SFX_NO_OPEN=1`（不放松存根行为，只让探针别弹窗）。

**验收**：跑前后各数一次 journal 里 `org.gnome.Nautilus` 的激活次数 —— 修前每跑一次 +1，
修后**增加 0 次**，探针仍 7 项全绿。

### 三、自解压 `.sh`：只对「真人自己跑」打开输出目录（引擎）

**用户问的原话**：「这个文件管理器你觉得是解压完自动打开好还是不打开好」。查过业界：
Windows 资源管理器「全部解压」有「完成后显示解压的文件」复选框（默认勾）、WinRAR / Bandizip
都有这个开关、7-Zip 干脆不做、NanaZip 明确做成**选项**。结论是**按场景分**，不该一边倒：

| 场景 | 该不该自动开 | 为什么 |
|---|---|---|
| 自解压 `.sh`（真人双击） | **开** | 动作意图明确「我就要这里面的东西」；用自解压的多是新手，解完找不到文件在哪儿 |
| 图形界面 | **不开**（给按钮 + 可选开关） | 批量解 20 个包会弹 20 个窗口 |
| 命令行 | **永远不开** | 脚本里跑 100 个包弹 100 个窗口是灾难 |

**改法**（`SFX_SH_STUB`）：原来只有「`YSHJY_SFX_NO_OPEN=1` 就关」，现在四条判定：
① 显式 `YSHJY_SFX_NO_OPEN=1` → 关（最高优先，CI / 脚本用）；
② `stdout` 是终端（真人在终端里敲 `./包.sh`）→ 开；
③ `stdout` 被重定向/管道，但**父进程不是 shell**（`ps -o comm= -p $PPID`，
= 在文件管理器里双击）→ 开；
④ 其余（`sh 包.sh > log`、批量脚本调用）→ 安静收场。

**验收**（假 `xdg-open` 记录调用，不真弹窗口，四个场景全中）：
脚本调用+重定向 0 次 / `NO_OPEN=1` 0 次 / 双击（父进程 `python3`）1 次 / 终端里跑 1 次。

### 四、界面：设置页新增「解压完自动打开输出目录」（默认关）

- 存在 `config.toml` 的 `[general] open_after = "on"/"off"`，**默认 off** ——
  不另起 state 文件，用户在设置页看到的就是唯一真源；引擎读配置逐字段宽容，多一个键不报错
- 只在「解压/救援/输密码重试」**完成**时生效，批量解压只开**第一个**有产物的目录（不逐个弹）
- 完成页的「打开输出目录」按钮照旧在 —— 想主动点的人不用动设置
- 副标题直接写明「批量解一堆包时不要开」，别让人自己踩

**验收**：`gui/test/app_settings_test.dart` **7 条**（无配置→关、没写键→关、on→开、off→关、
键写在 `[output]` 段不算、注释掉不算、配置写坏→当关着且不炸）；
`flutter analyze` 仍只有一条既有 info；`flutter build linux --release` 通过，引擎已 cp 回 bundle。

**回归**：`bash tests/all.sh` **30 通过 / 0 失败 / 4 跳过 / 179 秒**，且整轮跑下来
文件管理器激活次数 **+0**（`flutter test` 现在带隔离的 `XDG_CONFIG_HOME`，不许碰用户真实配置）。

### 五、Windows 交叉编译阻塞解除（`extract.rs` 符号链接的 cfg 隔离）

**背景**：`docs/ROADMAP.md` 一期第二条 —— Windows 路线唯一的实锤阻塞，实测
`error[E0433]: cannot find unix in os`。这条不修，「全平台」第一步就走不出去。

**先扫后改，扫出来的东西和预想不一样**：全仓扫 `std::os::unix|std::os::linux|libc::|PermissionsExt|
MetadataExt|CommandExt|symlink|/proc/|/sys/` 共 **48 处、13 个文件**，看着像要改一大片。
逐处看上下文才发现：**47 处早就 cfg 隔离好了**（`main.rs` 的 SIGPIPE / termios、`crash.rs` 的
`localtime_r`、`extract_stream.rs` 的 symlink、`tools.rs`/`resume.rs`/`api.rs`/`restore.rs` 的
`PermissionsExt`/`MetadataExt` 全带 `#[cfg(unix)]`；`hardware.rs` 的 `/proc`、`/sys` 是 `.ok()?` 降级；
`pack/mod.rs` 的 `xdg-open` 只是 shell 存根里的字符串，不参与编译）。
**「扫到 N 处」不等于「要改 N 处」**，先看每处有没有 cfg 再动手。

**真正没隔离的只有 `src/extract.rs:499`**（解压时建符号链接）。修法：

- `#[cfg(unix)]`：直接 `std::os::unix::fs::symlink`
- `#[cfg(not(unix))]`：Windows 上普通用户建符号链接要管理员权限，所以退回**「目标是什么就复制什么」**
  （目录 → `create_dir_all` 同名目录；文件 → `fs::copy` 一份），并记一条
  `DiagnosticCode::SlowPath` 让界面/日志看得见 —— **不静默丢条目**
- 诚实交代代价：脚本类链接在 Windows 上不保持「跟随」，但内容一定在（写进注释，别假装是完整还原）

**验证三步（都是真跑的，不是推断）**：

1. `cargo check --all-targets --target x86_64-pc-windows-gnu` → **rc=0**（7 秒）
2. `cargo build --release --target x86_64-pc-windows-gnu` → 出 `yshjy.exe`
   **13 MB · PE32+ console · 19.9 秒**
3. **wine 真跑**：`yshjy.exe --version` 正常；拿一个真 zip 解压
   —— 中文内容逐字节一致（`diff` 过），时间戳也还原了
4. Linux 侧回归：`bash tests/all.sh` **30 通过 / 0 失败 / 181 秒**，`zip_symlink.sh` 探针照旧绿

**顺手抓到一个 Windows 侧的真 bug（记进 TODO 平台节，不在这批修）**：`crash.rs` 里非 unix 的
`本地时区偏移秒()` 还是 `return 0` 占位 → zip 里的**本地时间被当 UTC** 解析，文件名时间差 8 小时、
**跨日会显示成明天**（wine 实测：今天 10-29 的文件解出来显示 10-30）。这正是本项目那条
「全项目时间差 8 小时」老坑的 Windows 版，跟时区一起修。

### 六、进度条补「已用时」（indicatif 的 `{elapsed}`）

**为什么做**：`docs/ROADMAP.md` 一期第三条。参考项目（clap / indicatif）里 `{elapsed}` 是标准字段，
我们只有「速度 / 剩余 / 第几项」，用户看一个跑了 20 分钟的大包时不知道已经花了多久。

**放在哪**：引擎的进度事件**不带 elapsed**（不为一句话升协议）；外壳自己记 ——
`终端出口` 增加 `开始时刻: Mutex<HashMap<u64, Instant>>`，按 job 记第一帧时刻
（`-j 2` 时两个任务各有各的起点，不能共用一个）。渲染统一放在 `进度消息()`：
**单任务手绘和 `-j 2` 的 indicatif `{msg}` 两条路径共用同一份算法**（跟 ETA 一样，只写一处），
`画进度行()` 只做透传。纯函数 + 参数注入 `Duration`，单测不用等真实时间流逝。

**加完才暴露的一个真 UX 坑**：`已 xx` 一加，**100 列终端上「当前文件名」被挤掉了** ——
而文件名恰恰是用户最想看的（「现在在解哪个文件」）。单测当场抓到（`要有当前文件` 失败）。
修法：已用**最后才加**，而且必须先给文件名留 **8 列**（跟文件名的准入线 `剩余宽 >= 8` 是同一个数，
少于 8 列的文件名只剩省略号＝没显示），放不下就**牺牲已用、保住文件名**。
顺带把「不足 1 秒不显示」也给了已用 —— 刚起来的任务写「已 0 秒」是纯噪音。

**验收**：

- 单测 27 → **29** 条：`已用不足一秒不显示已零秒`、`窄终端先丢已用再丢剩余`
  （后者带**属性断言**：任何宽度下①不超宽②「已用出现 ⇒ 文件名也一定在」—— 比钉死某个列数下丢谁更稳）
- **真实终端抓帧**（`script -qec "stty cols 120; …"`，2 GB 单包，24 秒）：
  **102 帧**带已用、**0 帧超宽**，前两帧正确没有已用（<1 秒），最后一帧
  `98.4% 1.99 GB/2.00 GB 1/1 项 61.1 MB/s 已 23 秒 in/big2.bin`
- `bash tests/all.sh` **30 通过 / 0 失败 / 198 秒**

## [0.9.28] · 2026-10-28（终端/界面收尾：只取封面的落点修复 + 救援模式接进协议与界面 + man 版本号漂移根治）

> 这一轮**不加新功能**，治的是「文档写着有、实际没有 / 有了但行为不对」的三处
> —— 都是实测撞出来的，不是猜的。回归：`bash tests/all.sh` **29 通过 / 0 失败**（179 秒）。

### 一、`--extract-cover` / 协议 `cover` 不认 `-o` / `dest`（真 bug）

**现场**：`tests/data` 里堆了 **36 个** `A1_steg_modeA_plain.封面 (N).mp4`。
不是谁手抖，是 `tests/contract/run.py` 的封面用例每次跑都往样本目录落一个 ——
`-o` 明明传了，封面却落在**源包旁边**。

**用户会怎么撞上**：源包在只读目录 / 网盘挂载点 → 切封面直接失败（说「没权限」，
用户以为是包坏了）；源包在下载目录 → 副本越攒越多，占空间还分不清哪个是新的。

**修法**（老调用点零改动，都是薄包装）：
- `cover_path_in(input, dir)` / `extract_zip_cover_to(input, base, dest)`；
  旧的 `cover_path` / `extract_zip_cover` 转调新函数（`dir = None` = 包旁边，历史行为不变）
- 落点目录不存在就建（`-o 新目录/` 是常见用法）；切失败把**自建的空目录收回来**，不留空文件夹
- 顺手改成 `.yshjy-part` → `rename` 的原子写（与解压六条落盘路径同一套）：
  半截 mp4 顶正式名落地时，用户会把它当成「切出来的原片」，其实打不开
- CLI `do_extract_cover` 接 `-o`；协议 `extract` 接 `dest`

**验收**：`tests/protocol_cover_only.py` **25 项**（已挂 `all.sh`）—— 只取封面时
`entries_done=0`、dest 里只有封面一个文件、封面是合法 MP4（`ftyp` 头）、源包目录零副本、
不给 dest 时仍落包旁边、同名自动改名不覆盖、解压 + 取封面同时要时两者共存。另加 2 条单测。

### 二、救援模式接进协议与界面（`rescue` 命令）

界面里搜 `rescue` 一直是零命中 —— CLI 有 `--rescue`，图形界面里根本没有这条路。
现在补上，且**不让 `extract` 自动兜底**（救援是慢路径，用户得知道自己在用它）：

- **协议**：新增 `rescue` 命令（`api::救援`），移植 CLI 那条链
  （`scan_whole_file` → `rescue_by_locals` → `extract_by_locals_ctx`），
  参数 `path` / `dest?` / `list_only?` / `encoding?`；
  `list_only:true` 只重建清单、**一个字节都不落盘**（界面可以先说「能救回 N 个」再动手）；
  不给 `dest` 落到 `<包名>.rescue`（与 CLI 一致）；`hello.features` 加上它
- **`serve.rs`**：`rescue` 走与 extract 同一套「一次一个任务」调度（慢路径，抢磁盘只会互相拖死，也能 cancel）
- **界面**：失败页在**救得回来**的错误上才给「试着救出来（索引坏了也能抢救）」按钮 ——
  `DATA_LOST` / `PARTIAL_DATA` / `CHECKSUM_FAILED`，以及 `NOT_ARCHIVE` 但 `looks_like=zip`
  （拖进来的真就是个视频时不给按钮，免得白等一场）；密码错、缺分卷不给。
  用 `OutlinedButton` 不用 `FilledButton`（首屏「唯一最强入口」有 widget 测试钉着）
- **错误码语义**：认不出压缩包一律回 `NOT_ARCHIVE`，**绝不折成 `DATA_LOST`** ——
  后者会让用户去重下那个其实完好的包

**验收**：`tests/protocol_rescue.py` **22 项**（已挂 `all.sh`，样本是现造的「中央目录签名被打乱的 zip」）：
`hello.features` 含 rescue / `list_only` 不落盘 / 真救 2 个文件内容逐字节一致 / 原包一字节未动 /
非压缩包回 `NOT_ARCHIVE` / 不给 dest 落 `<包名>.rescue`。

### 三、man 页版本号永远停在旧版（根因在生成器的比对逻辑）

`cli_spec_match.sh` 报「man 页版本号与 Cargo.toml 不一致（应为 0.9.27）」，而 man 页里写着 0.9.26。
根因：`scripts/gen-cli-assets.py` 比对 man 页时把整行 `.TH` 归一化成 `.TH NORMALIZED`，
**版本号也在这一行里，于是被一起抹掉了** → 版本升号后生成器认为「一模一样」直接跳过写盘，
man 页从此停在旧版本；只有探针会发现（生成器自己说「本来就是最新的」）。

**修法**：新增 `norm_man()`，**只抹 `.TH` 行里的日期**，版本号与其他字段参与比对。
修完生成器正常输出 `✅ 已生成：yshjy.1`，man 页变 `yshjy 0.9.28`，`cli_spec_match.sh` 7 项全绿。

### 四、出包与装机（2026-10-28 夜）

- 版本抬到 **0.9.28**（Cargo / pubspec / AGENTS / man 四处同步；0.9.27 已装机，按规矩必须抬号）
- 三档产物齐全：`yshjy_0.9.28_amd64.deb`（9.8M）+ `yshjy-0.9.28-linux-x86_64.tar.gz` +
  `yshjy-full-0.9.28-linux-x86_64.tar.gz`（各 3.7M + sha256）—— 顺带补上 0.9.27 只出了 deb、
  漏掉的那两档 tar.gz
- `scripts/test-packages.sh` **33 项全绿**；`sudo apt install` 覆盖装机后实测：
  `dpkg -l yshjy` = 0.9.28、干净 PATH 里 `yshjy --version` = 0.9.28（`~/.local/bin/yshjy`
  只是软链到 `/usr/bin/yshjy`，没有旧副本抢路）、菜单项 **14 种格式**、
  `Exec=/usr/lib/yshjy/gui/yshjy_gui`、装机版「只取封面落 `-o`」生效且包旁边零副本、
  `--help` 里 `--pick` / `--rescue` 都在

### 五、本轮其它

- 补提交四个平台脚手架（`gui/android|ios|macos|windows`，106 个文件）：多平台适配的底子，之前一直未入库
- `.unshallow.log`（参考库拉取脚本的临时日志）加入 `.gitignore` 并删除
- `docs/CLI-SPEC.md` 新增 **§4.5 `--pick` 交互键位**（8 组按键 + 边界行为），
  键位逐条对着 `dialoguer` 源码抄；顺带 pty 实测了 Ctrl-C：**SIGINT 直接中断、光标留在隐藏状态**，
  已在文档写明「正常退出请用 Esc / q」，修法（装 SIGINT handler）进了 TODO 表尾
- 清理 `tests/data` 里那 36 个编号副本（测试临时产物，样本本体未动）

## [0.9.27] · 2026-10-28（终端收口：选项表成真源 + 补全/man 自动生成 + --pick + --list 表格 + 多行进度）

> 由来：用户定「核心功能已完整，发布前把终端和图形界面优化好」。第一步不是加功能，
> 是治**结构病** —— 同一份 CLI 选项清单存在于三处手抄副本（`main.rs` 两个常量 / 三份补全 / man 页）。
> 施工清单见 `docs/CLI-GUI-PLAN.md` §1，状态见 `docs/TODO.md`。

### 一、三方对账：手抄副本**已经漂了**（不是"可能会漂"）

用真二进制实测（2026-10-28）：

| 处 | 长选项数 | 备注 |
|---|---|---|
| `yshjy --help` 文案 | 35 | 用户在帮助里看到的那份 |
| `main.rs` 两份常量表 | 37 | 含内部选项 `--serve` / `--debug-crash` / `--verbose` |
| 手写补全三件套 | **18** | **缺 17 个** |

两处真问题：

1. **`--import-passwords` 会取值却没登记进「带值的选项」** —— 与当年 `--lang en 包.mp4`
   把 `en` 当成要解压的文件是同一个坑。当前因为它提前 `return` 所以没炸，属**隐患**不是活 bug；
2. `--verbose` 是真选项但 `--help` 只写了 `-v, -vv`（文案不全，无害）。

### 二、`src/cli_spec.rs`：39 项的唯一真源

一张纯数据表（长名 / 短名 / 值名 / 是否内部），`main.rs` 的 `带值的选项()`、`无值选项()`
改成从表推导（`OnceLock` 缓存，4 处调用点）。**加选项以后只加一行表项**。

自带 10 条表内自检：长名唯一、短名唯一、取值与开关不重叠、内部选项不外露、
总数钉桩（39，故意逼你确认补全/man 也跟着更新）、`--import-passwords` 必须在带值清单。

### 三、补全从表生成（手写三件套退休）

- 新增内部命令 `--dump-cli-spec`：让二进制自己吐 JSON（**生成器不许解析 Rust 源码**，正则一漂就错）
- `scripts/gen-cli-assets.py`：生成 bash / zsh / fish 三份；**说明文字从 `locales/zh-CN.toml`
  的 `help.text` 提取**（与 `--help` 同一份文案，不会出现第二份文案）；值补全动作单独登记
  （目录 / 文件 / 枚举 `1 2 4 8`、`L1 L2`、7 种语言 / 密码不给候选）
- `tests/cli_spec_match.sh`：五条对账探针（补全与表同步、无孤儿、无漏登记、`--help` 无假选项、
  `bash -n` + `zsh -n` 语法），已挂进 `tests/all.sh`

实测（真跑，不是估的）：

```
bash: yshjy --de<TAB>     → --dest
bash: yshjy --imp<TAB>    → --import-passwords   ← 就是之前漏的那个
bash: yshjy -j<TAB>       → 1 2 4 8
bash: yshjy --lang<TAB>   → zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES
三份补全：18 项 → 36 项对外选项全在
```

### 四、验收

- 单测 334 + bin 27 + doctest 2 全过；`cargo clippy --all-targets -D warnings` 干净
- `tests/cli_spec_match.sh`：通过 5 · 失败 0
- `bash tests/all.sh`：见提交记录
- **行为零变化**：`--help` 仍 35 个长选项（`help.text` 一个字未动）、`-extract-cover` 仍提示拼写、
  `--lst` 仍报错

### 五、踩到的两个老坑（别再踩第三次）

1. **`cargo test` 不重建 `target/debug/yshjy`** —— 第一轮实测跑的是旧二进制，自报 `0.9.26-c` 露的馅；
2. **bash 不认中文变量名** —— 探针第一版写成 `通过=$((通过+1))`，直接 "未找到命令"。

### 六、man 页也接进生成器（22 个选项 → 36 个）

- `docs/yshjy.1` 不再手写，改由 `docs/yshjy.1.tmpl` + 生成器产出；说明**优先取 `help.text`**，
  help 里只出现在「用法」段的 6 个选项（`--list/--rescue/--report/--no-paths/--help/--version`）
  由 `MAN_DESC_OVERRIDES` 补
- `.TH` 的版本号从 `Cargo.toml` 读（**不再手写** —— 历史上落后过三个版本）
- 选项按用途分组（解压 / 打包 / 输出与配置 / 诊断 / 帮助），共 36 项全部覆盖
- `scripts/build-deb.sh` 打包前自动重新生成（不做人工核对）
- 探针扩到 7 条：补全三份 + man 页的选项集合、man 版本号与 `Cargo.toml` 一致
- 实测：man 页 22 → 36 个选项；`groff -man -Kutf8` 只剩 3 条「cannot adjust line」排版微告警（中文长行，无害）

### 七、老命令文案校准（用户要求：功能更新了，老命令也得跟）

逐条拿**代码真实行为**跟文案对，10 处不一致，其中 3 处是真说错了：

| # | 选项/位置 | 原文案 | 实际行为 |
|---|---|---|---|
| 1 | `--dest` | 默认「当前目录」下的同名文件夹 | 默认「**包所在目录**」下的同名文件夹（D19 改过，文案没跟） |
| 2 | man 环境变量 | `YSHTY_LANG` | `YSHJY_LANG`（拼写 bug） |
| 3 | `--encoding` | help 只写 `big5/sjis/gbk`；报错文案又说只认 `auto/gbk/big5/sjis/utf-8` | 实际认 `auto/gbk/gb18030/big5/sjis/shift-jis/utf-8`，三处已统一 |
| 4 | `--sfx` / `--sfx-linux` | 只说「双击自己解开」 | 还会**跳过 `.说明.txt`**、**不能与 `--split` 同用**、**不需要 `--cover`** |
| 5 | `--import-passwords` | 只说「每行一个，# 注释」 | 还支持 **Tab 分隔取第一列**（兼容隐写者 PW.txt）、自动去重、中文不过滤 |
| 6 | `--tail` | 只说「尾部追加布局」 | 封面是**图片/文本**时会**自动**用它 |
| 7 | `--verbose` | help 只列 `-v, -vv` | `--verbose` 是 `-v` 的长写法（已补进 help） |
| 8 | `--split` | 只说「2G / 500M」 | 也认**纯字节数**（如 `1048576`） |
| 9 | man 选项段 | 22 项 | 36 项全列（见上一节） |
| 10 | 5 种语言 help | `--force` / `--show-config` / `--import-passwords` 整行都没有 | 已按中文顺序补齐 |

**7 种语言全部同步**（zh-CN / zh-TW / en-US / ja-JP / ko-KR / ru-RU / es-ES），不是只改中文 ——
否则外国用户看到的还是错的。复核脚本确认 7 份 help 都不再缺任何对外选项。

### 八、`--list` 改表格（comfy-table，MIT）

- 引 `comfy-table 8`（只多带一个 `unicode-segmentation`；`cargo deny check licenses` 通过）
- 四列「序号 / 大小 / 加密 / 路径」，中文按**显示宽度**对齐（不是字节数）
- **只在真终端且 ≥50 列时画表格**：
  - 管道 / 重定向 → 保持原来一行一条（脚本靠这个格式解析，`--json` 一个字段都没动）
  - 窄于 50 列 → 也退回老格式（实测 40 列时表格会把 `大文件.bin` 折成 `大文件.b` + `in`，不如老格式好读）
- 表头 4 个 i18n 键 × 7 语言
- 实测：80 / 100 列中文对齐正确；40 列自动退回一行一条；管道输出与改前逐行相同

### 九、`--pick` 全屏勾选（dialoguer，MIT）

用户点名要的「档 3」。**不手搓 raw mode** —— 键位照 `refs/upstream/dialoguer/src/prompts/multi_select.rs:245-305`：

```
↓ / Tab / j   下移      ↑ / BackTab / k  上移
← / h         上页      → / l            下页
空格          勾选      a                全选 / 全不选
Esc / q       取消      Enter            确认
```

- 新增 `src/pick.rs`：预览用 `NullSink`（勾选界面不混进度噪音）
- `--pick` 优先于 `--select`；非交互终端（管道 / cron）报 `cli.pick_no_tty` 并以 **USAGE(2)** 退出
- Esc / q 取消算成功（退出码 0，一个文件都不解）
- 3 个 i18n 键 × 7 语言；help 里补一行 `--pick`
- 新探针 `tests/pick.sh`（用 python 开 pty 真发按键）**6 项全过**：
  - **验收①** 勾第 1、3 项的结果与 `--select "1,3"` **逐字节一致**
  - **验收②** 管道里退出码 2 且提示改用 `--select`
  - **验收③** 中文文件名（`遊戲數據/存档.sav`）在界面里完整不错行
  - **验收④** Esc 取消零产物

### 十、踩到的新坑（记下来）

**TOML 值里带引号必须转义**：`pick_no_tty` 的值里有 `--select "1,3,5"`，第一次写进 toml 时没转义，
7 个语言包全部解析失败 → 程序**启动即 panic**（崩溃日志里能看到 `unexpected key or value`）。
改完还要 `cargo build` 才生效 —— 语言包是 **编译时嵌入**的，改 toml 不重编等于没改。
（我第一轮就是这么被骗的：tomllib 校验通过了，跑起来还是崩 —— 因为二进制还是旧的。）

### 十、`-j 2` 多行进度（indicatif，MIT）

- 把进度行的**文字段**抽成共用函数 `进度消息()`：单任务手绘和 `MultiProgress` 的 `{msg}` 用同一份算法
  （否则两条路径的「剩余时间」会算出不同结果 —— 台账里的硬约束）
- 单任务路径**一个字没改**（那 7 条单测原样全绿）；只有真出现第二个任务才切 `MultiProgress`
- **踩到的坑（重要）**：indicatif 在 `TERM=dumb` 下会**静默退化成 hidden**
  （`draw_target.rs` 的 `is_dumb()` 一票否决，不报错、不提示、什么都不画）→ 「一点进度都没有」。
  已加判断：`TERM=dumb` 时退回单行手绘，宁可单行也不沉默。
  顺带发现 Hermes 自己的终端执行环境正是 `TERM=dumb`，本地测试特别容易撞上这个坑。
- 实测（pty 真跑）：`-j 2` 包A 14 帧 / 包B 21 帧；单任务等价；管道模式零进度字符
- 新探针 `tests/progress_multi.sh`（4 项）已挂进 `tests/all.sh`

### 十一、测试环境踩的坑（写下来别再犯）

**① `/tmp` 是 tmpfs（内存盘，9.8G）**
我在 `/tmp` 里造了两个 700MB 测试包，把它吃到 **97%**，直接导致「磁盘空间不够」的假故障，
还占掉 9.4G 内存。造测试样本一律放项目内 `target/`（机械盘）。

**② 内存盘上解压太快，进度测试会假阴性**
`/tmp` 上解压等于纯内存拷贝：release 版解 200MB×2 只要 **0.26 秒**，一次进度绘制都赶不上。
探针一度报「单任务没进度 / -j 2 两行没画出来」，其实是样本太快。
现在样本放机械盘 + release 版跑，才测得准（debug 版慢，侥幸有帧，反而掩盖问题）。

**③ 全零数据会触发我们自己的 zip 炸弹防护**
用 `b"\0" * 300MB` 造包，压缩比 230:1，被 `safety.zip_bomb_ratio` 拦下并提示「中间有一段数据丢了」。
我一度当成解压 bug 去查（`unzip` 能解、我们解不出），实际是**防护正确生效**。
测试样本必须用随机数据。

**④ 首次绘制被 200ms 节流吃掉（真缺陷，已修）**
`last_draw` 初始化成「刚刚」，于是构造后 200ms 内的第一条进度事件被丢弃 ——
短任务（内存盘上 150MB 只要 0.2 秒）会**一帧进度都不显示**。
改成 `Option<Instant>`：`None` 表示还没画过，第一帧无条件画。

### 十二、出包与装机（0.9.27）

- `dist/yshjy_0.9.27_amd64.deb`（9.8M），`build-deb.sh` 自查 11 项全过
- 装机后实测：`dpkg -l yshjy` = 0.9.27；菜单项认领 14 种格式；
  `xdg-mime query default application/zip` 与 `application/x-7z-compressed` 都返回 `yshjy.desktop`；
  用户配置 `~/.config/mimeapps.list` 里 14 条（不是 root 的 —— `su` 事故点没复发）；
  三种补全 37 个长选项、与源码树逐字节一致；man 36 项含 `--pick`

### 十三、下一步

界面侧：只取封面按钮、救援模式按钮；再往后是 `--pick` 的中文键位速查表。

---

## [0.9.26] · 2026-10-28（第二十一批 · 阶段 0 T5：装包与发布检查）

> 由来：阶段 0 体检的 T5「装包与发布检查」是最后一项（用户要求放最后）。这轮做了三件事：
> **对外版本号去掉 `-c`**、**deb 逐项核对并把两个真事故固化成断言**、**AppImage 评估**（结论本轮不做）。
> 完整核对表与 AppImage 配方见 **`docs/RELEASE-CHECK.md`**。

### 一、对外版本号 `0.9.26`（用户定：就在现号上去掉 `-c`，保留老习惯）

五处同时改，少一处就会出现「dpkg 说一个号、界面状态栏说另一个号」：
`Cargo.toml` / `gui/pubspec.yaml` / `docs/yshjy.1` 的 `.TH` / `docs/PROTOCOL.md` 两份示例的 `engine` /
`CHANGELOG` + `HANDOFF`。改前实际状态是**三处落后**：man 页与 PROTOCOL 示例还停在 `0.9.24-c`。

### 二、deb 逐项核对（`scripts/test-packages.sh` 重写，32 项通过 / 0 失败）

原来的验收脚本把版本号写死成 `0.9.21-c` —— 跑起来「全 PASS」，验的却是三个月前的老包。
现在自动取 `dist/` 里最新的 deb 与 tar.gz，并把这些固化成断言：

- 元数据六项齐全、包名/架构对得上、Depends 含 `libc6` 与 `libgtk-3`
- 文件表 11 项（CLI / GUI / `libapp.so` / `icudtl.dat` / 桌面项 / 图标 / man / 三种补全 / Nautilus 右键脚本）
- 属性：全部 `root/root`、man 页必须 `644`
- **依赖是否真够**：`libgtk-3-0t64` 递归闭包 279 个包 vs GUI 实测 70 个 `.so` 逐个反查归属 → 无缺口
  （唯一报警的 `libdl.so.2` 是 dpkg 把 `libc6-i386` 的 32 位副本认错了，干净机器不会缺）
- 功能：用**包里**的二进制真打包成伪装视频 → 真解回来 → `cmp` 逐字节一致
- tar.gz：sha256、解包可运行、**版本必须与 deb 同源**

### 三、修掉两个真事故（都加了断言，不许再犯）

1. **装完 deb，双击压缩包还是被 Nautilus 抢走**：`postinst` 里
   `su -s /bin/sh "$SUDO_USER" -c "xdg-mime default …"` —— 不带横杠的 `su` **继承 root 的 HOME**，
   默认项写进了 `/root/.config/mimeapps.list`，用户账号里一条都没写成。
   修法：先 `getent passwd` 取真家目录，再在 `su -c` 里显式设 `HOME=` / `XDG_CONFIG_HOME=`。
   断言：postinst 里有 `xdg-mime default` 就必须同时出现 `HOME=`。
2. **0.9.25-c 的桌面项只认领 7 种格式**（源码早已 14 种）——多出来的 7 种用户根本摸不到，
   说明那个包是拿旧 desktop 打的。修法：`build-deb.sh` 自查加死断言「包内 MimeType 计数 = 14」，
   数不对拒绝出包。

顺带：man 页打进包是 `664`（gzip 重定向跟 umask 走）→ 显式 `chmod 644`；
`build-deb.sh` 的自检从 5 项扩到 11 项 + 三条事故断言（含「包内引擎自称版本 = 包版本」——
它当场抓到 `dist/` 里躺着的 tar.gz 还是 `0.9.23-c`）。

### 四、AppImage 评估：本轮不做（配方已留档）

不是做不出来，是**在本机（Debian 13，glibc 2.41）构建出来的 AppImage 基本没人能跑** ——
glibc 向后兼容不向前兼容。要做必须用容器（Debian 12 bookworm，glibc 2.36）当基线，
外加按 KDAB 的实战结论排除图形栈（`libGL*/libEGL*/libGLES*/libgbm*/libdrm*/libwayland-*`、
`libc6/libstdc++6/libgcc-s1/zlib1g`）而只捆 GTK3 + gdk-pixbuf loaders，体积约 60–80 MB
（deb 才 9.7 MB，因为用宿主 GTK）。用户和朋友的机器都是 Debian/Ubuntu 系，收益对不上代价。
完整配方写进 `docs/RELEASE-CHECK.md` §4，将来要做照着做，不用重新调研。

## [0.9.26] · 2026-10-27（第二十批 · 阶段 1 梯队四·第 12 项：RAR 路径接原子写入）

> 由来：`docs/TODO.md` 梯队四表尾那条 —— 「RAR 暂未接 `PartFile`，库自己管 writer 生命周期」。
> 后果和梯队一第 1 项修的那个老毛病一样：解到一半被 `kill -9` / 断电 / 磁盘满时，磁盘上留下的是
> **顶着正式文件名**的半截文件（外观与好文件无异），重试还会因为「同名改名」策略绕开它、
> 长出 `(1)` 副本。这是七条落盘路径里**最后一条**没走原子写入的。

### 先做了调研：为什么当时没敢接

`rars` 的 `ExtractedEntryMeta`（回调参数）只有这些字段：

```
name_is_unicode / name / file_time / file_attr / attr_source / mtime_refinement / is_directory
```

**没有条目大小，也没有 CRC** —— 所以回调这一层根本拿不到「这个条目应该有 734003200 字节」，
也就没法在 `Drop` 里判断「库是写完了还是中途出错」。`Drop` 只能当作「库对这个条目的写入结束了」。

### 做法（以及有意保留的局限）

- `CountingWriter` 的 `inner: fs::File` 换成 `part: Option<PartFile>`，回调里
  `fs::File::create(&out_path)` → `PartFile::create(&out_path)`
- `Drop` 时：取消/限额中止 → 直接丢掉 part（**正式文件名从头到尾没出现过**）；
  否则 `finish()` 原子改名
- **有意接受的局限**：库因为 CRC / 解密失败而提前 `drop` writer 时，已经写出的字节仍会落地
  （改之前同样会落地，**不是新增的退步**）。换到的是「进程被杀 / 断电不再留半截正式名」。
- **没做的方案（记账，不硬上）**：`rars` 有 `extract_with_control_and_errors(decide, on_error)`，
  `on_error` 能拿到逐条目错误，理论上能精确判断哪一条失败。但那要重写整个回调层
  （`decide` 收的是 `ArchiveMember` 而不是 `ExtractedEntryMeta`，写出机制也不同），
  RAR 是主力解压路径之一，风险和收益不匹配。

### 验证

| 项 | 结果 |
|---|---|
| 真实样本 `rar_mtime_sample.rar` | yshjy 解出的文件 sha256 清单与 `7z x` **完全一致**，退出码 0，**0 个 part 残留** |
| 分卷样本 `rar_vol_sample/`（4 卷） | 正常解出 `split.bin`，0 个 part 残留 |
| 单测 `rar写入器_写的过程中只有part_结束后才出现正式名` | 写的时候断言正式名不存在、part 存在；Drop 后正式名出现、part 消失、内容一致 |
| 单测 `rar写入器_取消时既不留正式名也不留part` | 取消后两样都不存在（改之前是「先写坏正式名再删」，中间被杀就永久留半截） |
| 原有 `无密码_rar_能解出` | 追加断言：解完目录里不许有 `.yshjy-part` |

### 真打断实测（装 `rar` 之后补上的，2026-10-27 晚）

一开始这台机器没有 `rar` 命令、仓库样本只有几 KB，只能靠单测。用户问「缺啥都可以装」之后
`sudo apt install -y rar`（仅用于**造测试样本**，不进发布物、不进运行时依赖），
于是有了新探针 `tests/rar_atomic.sh`：`rar a -m0` 造 150 MB / 10 文件的包，
按「完整解耗时 ÷ 3」定位中断点，`timeout -s KILL` 硬杀。

```
中断后 yshjy 退出码=137（SIGKILL）
中断现场：part 1 个，正式名文件 0 个
  ✓ 中断后没有任何「顶着正式名的半截文件」
  ✓ 中断后正式文件名一个都没出现
  ✓ 中断后重试：文件全齐、内容全对、零副本
  ✓ 重试完成后零 part 残留
  ✓ 删掉的文件重跑后补回来了（自愈）
RAR 原子落盘探针：7 通过
```

**意外拿到的「改之前」对照**：第一次跑探针时用的是**旧二进制**（改完 RAR 只跑过 `cargo test`，
它不会重建 `target/debug/yshjy`），现场是 `f2.bin` 71565312 字节（参照 104857600）+ `f2 (1).bin` 副本
—— 正是修复前的症状「半截正式名 + 重跑长副本」。这条反过来证明了探针抓得住这个病，
也让「修好了」有了前后对照。探针因此加了一条自检：**源码比二进制新就直接拒跑**。

顺带发现一个「不是缺陷」的现象并写清：**完整解完之后再解一遍**会产生 9 个 `f2 (1).bin` ——
那是默认 `Rename` 策略的正常行为（想覆盖就把配置 `on_conflict` 改成 `replace`）；
而**中断后**重跑是零副本（因为正式名一个都没出现过），这才是本项要保证的。

回归：单测 **334** 通过 / 0 失败（新增 2 条 + 1 处断言）、协议 12、doctest 2；
`clippy --all-targets -D warnings` 干净；`tests/all.sh` **24 通过 / 0 失败 / 171 秒**（新增 RAR 探针）。

## [0.9.26] · 2026-10-27（阶段 1 梯队四·第 11 项：伪装 / 自解压打包也报进度）

> 由来：`docs/TODO.md` 梯队四第 11 项 ——「打 2 GB 伪装包时进度条在动」。
> 实测（真机械盘）：打 2 GB 伪装包用了 **183 秒，全程零进度事件**，
> GUI 的打包页进度条和 GNOME 顶栏的「正在打包 N%」整段是死的，跟卡死没区别。

### 病根：三条打包路径只有一条接了进度

| 路径 | 修之前 |
|---|---|
| 普通压缩 | **有**进度（`build_plain_ctx` + 回调） |
| 伪装包（`.mp4` 等） | `write_zip(..., &mut \|_, _\| {})` —— 空回调；拼装用 `std::io::copy`（一次拷完、零进度） |
| 自解压（`.exe` / `.sh`） | 同上 |

而且 **CLI 和界面走的是两条完全不同的代码路径**：界面走协议（`src/api.rs` 的 `pack`），
CLI 走 `src/main.rs` 的 `do_pack` 自己调 `pack::build*`。第一次只改了 `api.rs`，
拿真二进制一跑还是 **0 条事件** —— 这才发现 CLI 那条路压根没有进度上下文。

### 改法

- 新增 `pack::PackTick`：`Wrote(n, Option<名字>)` + `TotalHint(n)`
  - **为什么要 `TotalHint`**：打包是两段式的（先压进临时 ZIP，再把 ZIP 拼到外壳后面），
    两段的分母不是一个东西 —— 压缩段分母是「输入总字节」，拼装段是「中间 ZIP 大小」。
    只给 `(已写, 名字)` 的话外壳没法知道「该换分母了」，进度条会卡在压缩段的百分比上。
- `build` / `build_sfx` / `build_sfx_sh` / `build_plain` 各加一个 `*_ctx` 版本
  （旧的公开签名保持不变，`main.rs` 里其它调用点不用动）
- 新增 `拷贝带进度()`：替代拼装阶段的 `std::io::copy`，4 MiB 一块，逐块上报
- 新增 `pack::输入统计()`：CLI 与协议两条路共用的「进度分母」
- 协议侧（`api.rs`）与 CLI 侧（`main.rs` 的 `do_pack`）**都**接上同一个上报闭包；
  CLI 的打包参数结构体补 `json` 字段，`--json` 时事件按行输出（脚本能消费）
- 打包节流 **500 ms → 200 ms**：和解压进度的 `PROGRESS_INTERVAL` 一个手感，
  打大包时不再一卡一卡地跳

### 实测（真机械盘，2 GiB 输入，`--level 0` 让两段都有活干）

```
PACK_PROGRESS 事件数：82（修之前 0）
   5%   108.0 / 2048.0 MiB   当前=输入/大文件.bin
  25%   512.0 / 2048.0 MiB   当前=输入/大文件.bin
  95%  1960.0 / 2048.0 MiB   当前=输入/大文件.bin     ← 压缩段
 100%  4096.0 / 4096.0 MiB   当前=（拼装输出）        ← 分母自动放大到 4096 MiB
出现过的分母：2048.0 / 4096.0 MiB      同分母段内进度回退：0 次      最终 100%
```

产物 2.1 GB、退出码 0、用时 183 秒 —— 进度真的走完全程。

### 新增探针 `tests/pack_progress.sh`（已挂进 `tests/all.sh`）

三条路径各验一遍，盯四件事：**分母变过（`TotalHint` 生效）、终态到 100%、
事件数够（修之前是 0 条）、产物没被写坏**。另外把自解压 `.sh` 真跑一遍解开验证。
150 MB 输入跑完约 10 秒，结果 7 项全过。

### 踩到的三个坑

| 坑 | 现象 | 记住 |
|---|---|---|
| **只改一条代码路径** | 改完 `api.rs` 拿真二进制实测仍是 0 条事件 | **CLI 与 GUI 是两条路**：协议命令在 `api.rs`，CLI 在 `main.rs` 的 `do_*`。改「用户能感觉到的行为」必须两边都改，并且用**真二进制**验，不能只看单测 |
| bash `local` 不认中文变量名 | `local 事件文件="$1"` 报「不是有效的标识符」，整个函数静默失效 | 探针脚本函数内变量**一律英文**（AGENTS.md 硬规矩第 2 条，这是第三次踩） |
| CLI `--json` 的事件格式 | 以为是 `{"type":"event","kind":…}`，统计脚本一条都认不出 | CLI 打的是**裸事件** `{"PackProgress":{…}}`；带 `kind` 包装的是 `--serve` 管道。解析时两种都要认 |

回归：单测 **332** 通过 / 0 失败（新增 2 条进度用例）+ 协议 12 + doctest 2；
`clippy --all-targets -D warnings` 干净；`tests/all.sh` **23 通过 / 0 失败 / 143 秒**。
界面侧不用改：`pack_page.dart` 与 `app_shell.dart` 早就订阅了 `PACK_PROGRESS`，
只是以前没有数据可收。

## [0.9.26] · 2026-10-27（阶段 1 梯队四·第 10 项：断点续传）

> 由来：`docs/TODO.md` 梯队四第 10 项。真人会撞上的场景是**大的包解到一半被打断**
> —— 关机、断电、关窗口、误点取消。以前只能从头再来一遍（14 GB 的包 = 40 分钟白干）。
> 验收标准是「中断后重开能接着解，且已验证字节一致」。

### 做法：条目级续传（不碰解码器内部状态）

```
解压 ──每个条目落盘成功──→ 往 <配置目录>/resume/<指纹>.jsonl 追加一行
重开 ──读记录 + 三重校验──→ 命中的条目直接跳，从断点接着解
```

为什么不是「字节级续传」（记住某个文件写到第几字节、下次续写）：

- ZIP / 7z / RAR / tar 的条目本来就是独立可寻址或独立可解码的，真正费时间的是
  「一条一条地解」，不是「单个条目解到一半」
- 条目级**不需要任何格式协作**：库里每个条目解完都会通知 `ctx.entry_done`，
  我们只在那一个时刻落一行记录，不碰任何解码器的内部状态
- 代价（有意接受）：**最大的那个文件**解到一半被打断，下次要重解它。
  换来的是「任何格式、任何库、零侵入」

### 业界对照（这块**一开始没查**，是用户追问后补的 —— 记下来当教训）

| 项目 | 它怎么做 | 我们的差别 |
|---|---|---|
| **itchio/savior**（Go，itch 客户端解压库） | 把续传能力定成三级：`none` / `entry`（条目之间）/ `block`（条目内部块边界） | 我们做的正是 **entry 级**，方向一致；但只给了 `resume: true` 一个布尔。已补 `resume_level: "entry"` —— 借它的命名，将来真做块级就改成 `"block"`，前端不用改代码 |
| **peel**（Rust，流式解压器） | frame 级 checkpoint + `write+fsync+rename` + 每块指纹；测试是「每格式 **100 个随机 kill 点**，每次断言与干净跑字节一致」 | 粒度我们只到条目级（不做条目内部）；但**「随机 kill 点」这个测法必须学** —— 已照抄进 `tests/resume.sh`，第一次跑就抓到一个真 bug（见下） |
| **7-Zip** | 官方**没有**解压续传，命令行只有 `-aos`（跳过已存在文件） | 那正是「半截文件也算已存在、永远修不好」的老毛病 —— 我们做原子写入就是为了治它。7-Zip 连**打包**续传都明确不支持（文件名在包尾，加了就没法接） |

**教训**：做「中断 / 并发 / 时序」这类功能，先花二十分钟看别人怎么踩坑，比自己推演省一个下午 ——
下面那个攒批 bug 和临界区 bug，本来都是别人已经踩过的。

### 记录长什么样（JSONL，append-only）

```text
{"t":"head","v":1,"input":"/data/a.zip","size":1234567,"mtime":1698400000,"dest":"/data/a"}
{"t":"item","n":"剧集/01.mkv","k":"f","s":734003200,"m":1698400123456789000}
{"t":"item","n":"剧集","k":"d","s":0,"m":0}
```

- 路径：`<配置目录>/resume/<包绝对路径 + 输出目录 的 sha256 前 16 hex>.jsonl`
  （同一个包解到两个不同目录 = 两份独立进度；混一份会互相误跳过）
- 为什么 append-only：中断时文件可能停在半行，逐行容错读取（坏行跳过）就能
  保住前面所有记录 —— 不用写索引、不用原子替换

### 「接着解的文件是对的」靠三重保证

1. **包没变**：头行记着包的绝对路径 + 大小 + 修改时间；任一不符 → 整份作废重来
   （用户换了包 / 重下了同名包，绝不能拿旧记录去跳过新包的文件）
2. **文件还在、大小对**：跳过前 `stat` 一遍，是不是文件、字节数是否与记录一致
3. **当初真的解完过**：只有解码循环走到「原子改名成功（+ 属性还原）」之后才落记录；
   取消 / 写盘失败的条目**不会**被记成完成

### 新增 `src/resume.rs`

- `ResumeLog::open` / `open_in`（后者给测试注入根目录）、`should_skip`（带磁盘校验）、
  `record`、`finish`（成功删记录）、`discard`（失败且一条没攒下时清掉）
- 单测 11 条：跳过判定、大小不符不许跳、文件被删不许跳、换包整份作废、
  不同输出目录独立、`finish` 清空、半行损坏容错、半行之后的新记录不被吞、目录/链接按类型校验

### 踩过的六个坑（都写进代码注释 / 探针里了）

| 坑 | 现象 | 现在怎么做 |
|---|---|---|
| 攒批写记录 | 小包解到三分之一被 `kill -9`，缓冲区里几十条**一条都没落盘** —— 恰好是最需要续传的场景。而 SIGINT（点取消）能走 `Drop` 所以没事，于是「点取消能续、关窗口不能续」 | **一条记录一次 `write`，不攒批、不 fsync**。数据进了内核 page cache 就跟进程死活无关，进程被硬杀也不丢；断电最多丢最后几条（重解，不产生坏文件）。10 万小文件多约 0.1~0.3 秒 |
| 测试想用环境变量隔离 | `forbid(unsafe_code)` 下 `set_var` 根本不能用（而且环境变量是进程级的，测试并行会互相踩） | `ResumeLog::open_in(根目录, …)`，测试显式注入临时目录，用例可并行 |
| 跳过检查放在同名策略之后 | 「同名跳过」策略会把续传要跳的文件当成「已存在冲突」记进跳过列表，用户看到一堆莫名其妙的「已跳过」 | 跳过检查**必须在同名处理之前**（ZIP / 7z / RAR / tar 四处一致） |
| 半行粘连 | 断电后文件尾部是半行，下次的记录直接粘上去，一起变成解不出来的垃圾行 | `open` 时检测末尾不是换行就补一个 |
| **「改名成功 → 记录没写」的临界区**（随机 kill 压测抓到） | 磁盘上已有完好的 `第033集.bin`，记录里却没有它 → 续传按默认的 `Rename` 策略又解一遍 → 多出 `第033集 (1).bin` 副本。固定断点（三分之一处）永远测不出来，8 轮随机 kill 里撞到 1 次 | 新增 `ctx.同名策略()`：**续传会话里一律按「覆盖重解」**（记录里没有的文件就是来历不明的，重解一遍最安全）；探针加了**定向复现**（手工删掉记录里一条已落盘的条目） |
| **测出来之前先怀疑了工具** | 第一次看到「续传一个文件都没解出来」，其实是调试命令 `\| grep -m1` 提前关管道把引擎 SIGPIPE 杀了 | 诊断时**别把被测进程放进会提前退出的管道里**，重定向到文件再看 |

### 接线范围

| 位置 | 改动 |
|---|---|
| `src/ctx.rs` | 新增 `resume` 字段 + `with_resume` / `resume_skip` / `resume_record`；跳过时**顺手补进度**（否则进度条永远停在断点） |
| `src/extract.rs`（ZIP） | 目录 / 符号链接 / 普通文件 三处跳过 + 三处记录 |
| `src/extract7z.rs` | 目录 + 文件；注释写明固实压缩只能省写盘、省不了解码 |
| `src/extract_rar.rs` | 目录 / 文件（手动登记进报告）；完成登记在 `CountingWriter::drop` |
| `src/extract_stream.rs` | tar 目录 / 链接 / 文件 + 单流整包（解完过一次就 0 秒通过） |
| `src/job.rs` | 空间预检之后开记录；`Success` → 删记录，`Partial`/`Cancelled` → 留着，一条没攒下 → 清掉 |
| `src/event.rs` + `src/api.rs` | 新事件 `RESUMED {job, done}`；`caps.resume` 由 `false` 改 `true` |
| `src/main.rs` + `locales/*.toml` | CLI 打一行「接着上次没解完的来（上次已完成 N 项）」，7 种语言齐 |
| `gui/lib/pages/home_page.dart` | 收到 `RESUMED` 在备注区说明一句（否则「一上来就有进度、前面文件没动」会被当成 bug） |

### 实测（真机械盘，`bash tests/resume.sh`）

新增探针 `tests/resume.sh`（已挂进 `tests/all.sh`，默认 30 MB / `--slow` 120 MB）。
它证明的是一条链：中断是真的 → 续传是真的 → 内容字节是真的 → 而且不是「看见文件就跳过」。

| 场景 | 证据 |
|---|---|
| **SIGINT（点取消）** | 120 个文件解出 83 个时打断 → 重开报「上次已完成 86 项」→ `diff -r` 与参照**逐字节一致**，sha256 清单完全相同 → 无残留 `.yshjy-part`、无残留记录 |
| **SIGKILL（关窗口 / 断电等价物）** | 解出 41 个时硬杀 → 重开报「上次已完成 43 项」→ sha256 清单完全相同 |
| **反证（磁盘校验真在起作用）** | 打断后手动删掉一个已解文件 → 重解把它补回来且内容与参照一致 |
| **随机 kill 点压测**（学 peel） | 每轮随机挑 10%~80% 的进度点硬杀，续传后必须与参照字节一致：**80 MB 包 12/12 轮全过**、40 MB 包 8/8 轮全过 |
| **临界区定向**（随机压测抓到的真 bug） | 手工删掉记录里一条已落盘的条目（等价于「改名成功、记录没写」）→ 续传**覆盖重解、不留 `(1)` 副本**、内容仍一致 |

回归：单测 **330** 通过 / 0 失败（新增 11 条）+ 协议 12 + doctest 2；
`clippy --all-targets -D warnings` 干净；协议契约 12 项符合 golden；
`flutter test` 13 条全过；`flutter build linux --debug` 成功。

## [0.9.26] · 2026-10-24（阶段 1 梯队一·第 1 项：解压原子写入）

> 由来：阶段 0 体检 T9-P1 / T8-P6 实测（证据见 `docs/AUDIT-PHASE0.md`）——
> 解压写到一半被中断（磁盘满 / 拔盘 / 取消 / 崩溃 / SIGKILL）时，磁盘上留下的是
> **顶着正式文件名**的半截文件；重试又因为「同名改名」策略绕开它、生成 `(1)` 副本，
> 坏文件永远修不好。实测 1 GB 包被杀一次再重试：目录变 12 个文件、1.15 GB，
> `part1.bin` 永远是 49 MB 的坏文件。

### 新增 `io::PartFile` / `io::PartWriter`（原子写入守卫）
- 先写 `<名字>.yshjy-part`，全部写完再 `rename` 成正式名（同目录改名是原子的）
- **进程被杀 / 断电**：磁盘上只留 `.yshjy-part`，一眼能看出「没写完」
- **正常失败**（读错 / 校验不过 / 限额中止 / 取消）：`Drop` 自动删 part，目录保持干净
- **重试**：part 直接覆盖重写，坏文件被替换，不再产生 `(1)` 副本
- 文件名过长时按字符边界截断再拼后缀（不 panic、不超 `NAME_MAX`）
- `rename_to()`：救援模式用它把残片直接改名成 `.partial`，不先占正式名

### 六条落盘路径全部接上
| 路径 | 改法 |
|---|---|
| ZIP（`extract.rs`） | `PartFile` + 写完后 `finish()` |
| 7z（`extract7z.rs`） | 同上；5 处「drop + remove_file」折成一行 `drop(part)` |
| 单流 gz/zstd/bz2/lz4/brotli（`extract_stream.rs` 普通分支） | `PartFile`；**顺带修好「写盘失败直接 `?` 返回、留半截文件」** |
| xz（writer 交给 `lzma-rs`） | 新 `PartWriter`（实现 `Write`），成功后 `finish()` |
| tar（`extract_stream.rs` tar 分支） | `PartFile` + 收尾三分支重排 |
| 救援模式（`rescue.rs`） | `PartFile` + `rename_to()`；残片仍存成 `.partial`，但不再先占正式名 |

### 实测（真机械盘 TOSHIBA MQ01ABF050）
- 1 GB 包解到 0.4 秒时 `SIGKILL` → 目录里**只有** `big/part0.bin.yshjy-part`（32 MB），
  没有顶着正式名的半截文件
- 重试一次 → 10 个文件全部完整、**0 个 `(1)` 副本**、0 残留 part、总大小正好 1 GB
- 五种格式正常解压：zip 5 / 7z 2 / tar.gz 2 / xz 1 / gz 1 个文件，全部 rc=0、零残留、字节数一致
- 单测 312 通过（新增 7 个原子写入用例）；`clippy -D warnings` 干净；`tests/all.sh` 全绿

### 阶段 1 梯队一·第 2 项：错误原因说人话
> 由来（同一份体检）：`docs/AUDIT-PHASE0.md` 的 T9-P3 / T8-P5——
> 「没权限」被说成「中间有一段数据丢了」、「-o 选到文件 / 路径超长」一律「内部错误」、
> 截断的包一边说「这不是压缩包」一边又说「看起来是：ZIP 压缩包」。用户被误导去重下好包。

- **`ErrorCode::from_io()` 统一分类**：没权限 / 空间不足 / 路径太长 / 目标不是文件夹 /
  文件不存在，各自成码；用 `ErrorKind` 判断（不用 errno），Linux / Windows / Android 都成立
- **新增两个码**：`PATH_TOO_LONG`、`DEST_IS_FILE`；7 个语言包文案补齐，界面
  `humanize.dart` 同步（引擎给码、外壳查文案的 D21 架构不变）
- **修掉误判的根子**：以前「一个文件都没解出来 + 有跳过记录」一律判 `DataLost`
  （「中间有一段数据丢了」）。现在 `ExtractReport` 会记下第一个可归类的 IO 错误码
  （`io_error`），`job.rs` 按它分流 —— 只读目录不再被说成包坏了
- **截断包**：扫到 ZIP 结构却凑不出可用窗口时判 `Truncated`（「文件不完整，常见于下载没下完」）。
  判据是「命中 ≥2 处，或文件 >64 KB 且开头是 PK」——只按开头签名判会把伪装包
  （`.mp4` 里放个 PK 头）误判成「没下完」，`serve.rs` 的既有测试盯着这条，已保留
- **新增探针** `tests/errmsgs.sh`（已挂进 `tests/all.sh`）：四场景 + 对照组，
  断言退出码与关键词，还专门断言截断包不能再出现「这不是压缩包」

实测（`tests/errmsgs.sh` 全过）：
| 场景 | 改前 | 改后 |
|---|---|---|
| 输出目录只读 | 中间有一段数据丢了 | 这个位置不让写 |
| `-o` 是普通文件 | 内部错误 | 目标位置是个文件，不是文件夹 |
| 路径超长 | 内部错误 | 路径太长，这个位置放不下这么长的文件名 |
| 截断的 zip | 这不是压缩包（+看起来是 ZIP） | 文件不完整（常见于下载没下完） |
| 对照组正常包 | 解开 | 解开（无回归） |

### 阶段 1 梯队一·第 3 项：重名策略接线
> 由来（体检 T9-P2）：设置页「遇到同名文件」的下拉（自动改名 / 跳过 / 覆盖）
> 以前**只存不用** —— 引擎从来没读过 `config.output.conflict`，三种策略实测输出
> 完全一样（永远改名），等于骗人。

- `Overwrite::from_config()`：认 `rename` / `replace` / `skip`，大小写与前后空格不敏感；
  **认不出的值一律按 Rename**（宁可多一个副本，绝不误删用户文件）
- `job.rs` 真正把 `config.output.conflict` 接到 `UnpackOptions.overwrite` 上
- **补上单流格式**：gz/xz/zst/bz2/lz4/br 以前直接覆盖已有文件，策略设了也不作数；
  现在与其它格式共用同一套策略，跳过时也写同一句原因
- **「全部按设置跳过」不再算失败**：以前 `job.rs` 见到「一个没解出 + 有跳过」就报
  `DataLost` / `NotArchive`；现在识别出这是用户自己选的策略，返回「部分成功」

实测（`tests/conflict.sh`，14 项全过）：
| 策略 | ZIP（同包解两遍，中间改一个文件） | 单流 gz |
|---|---|---|
| `rename` | 10 文件、5 个 `(1)` 副本、原文件不被覆盖 | 生成 1 个副本、原文件不被覆盖 |
| `replace` | 5 文件、0 副本、原文件被真覆盖 | 原文件被真覆盖 |
| `skip` | 5 文件、0 副本、原文件保留、退出码 5（部分成功） | 同左 |

### 阶段 1 梯队二·第 1 项：自动抓密码收窄
> 由来（体检 T8-P4）：自动找解压码的规则太宽 —— 同目录和**上一级**里任何
> `.txt/.md/.log/.ini` 都会被读来抠「密码」。实测把父目录里覆盖率转储的源码注释
> 当成了「解压码」，白试一遍还可能解错包。

- 判定从「名字像说明 **或** 扩展名像文档」收紧成「**且**」
- **同目录**：名字有说明性（说明/密码/readme/必看…）或与包同名，且扩展名是文档类
- **上一级**：只认「与包同名」（父目录的 `README.md` 不再被读 —— 那是最容易误伤的）
- 「与包同名」= `旅行照片.zip` 认 `旅行照片.zip.说明.txt` / `旅行照片.说明.txt`，
  且包名后面必须紧跟 `.` `_` `-` 空格（否则 `a.zip` 会把 `abc.txt` 认成同族）
- 用户密码本、`--password-file` 的优先级不变

实测（端到端，真加密包）：
| 场景 | 结果 |
|---|---|
| 同目录 `旅行照片.zip.说明.txt` | ✅ 自动用上密码，解出 5 个文件 |
| 只有父目录 `README.md` 含密码 | ✅ 不采用，如实报「这个包带锁，需要解压码」 |
| 同目录通用 `说明.txt`（不带包名） | ✅ 仍认（真实场景常见，没误伤） |

### 阶段 1 梯队二·第 2 项：设置页「界面语言」文案改实话
> 由来（体检 T7）：设置页「其它」区并列写着 7 个语言代码，看着像「改 config.toml
> 就能切界面语言」，实际界面文案 401 处全是硬编码中文；页面底部「还没做的」里
> 自己都写着「界面文案还没抽成语言包」，前后矛盾。

- 改为直说：`界面语言：目前只有中文（引擎支持 7 种语言，界面文案还没抽成语言包）`
- 「还没做的」清单同步写清工作量：要先把界面里 401 处硬编码中文抽成语言包

### 阶段 1 梯队三·第 1 项：协议契约测试
> 由来：界面的每一行字、每一个进度条都来自引擎 `--json` 的事件流。这套结构
> 以前只靠人肉探针（`tests/protocol_*.py`）守着，改了字段名界面就静默失效。

- 新增 `tests/contract/run.py` + `tests/contract/golden/*.json`（11 份）
- 覆盖 11 个真实场景：解压 zip / `--list` / 7z / tar.gz / 正确密码 / 错误密码 /
  截断包 / 救援 / 取封面 / `--select` / 打包
- **只固化结构不固化值**：路径、耗时、字节数每次不同，但「事件名 + 字段名 + 字段类型
  + 退出码」是契约
- **错误码按实际值固化**：`JobFinished.outcome.Failed` 存的是 `WrongPassword` /
  `Truncated` 这类码（界面的 humanize 靠它查文案），码一改名就会被测出来
- 已挂进 `tests/all.sh`；`--update` 用于「真改了协议」时重新生成

验收（故意改坏，两种都能被测出）：
| 改坏方式 | 测试输出 |
|---|---|
| golden 删掉 `StageStarted` 事件 | ✗ `.events.StageStarted 是新加的字段（契约变更）`，退出码 1 |
| 把 `JobStarted.size` 类型改成 str | ✗ `.events.JobStarted.size: 类型 str → int`，退出码 1 |
| 恢复正常 | 11 项全部符合 golden，退出码 0 |

### 阶段 1 梯队三·第 2 项：fuzz 写盘路径（`extract_target`）
> 由来：原有 3 个 fuzz target（`detect_stream` / `list_zip` / `locate_zip`）**全是只读的**，
> 而落盘恰恰是最容易出事的一环 —— 路径穿越、写到沙箱外、失败留半截文件、解压炸弹。

- 新增 `fuzz/fuzz_targets/extract_target.rs`：把任意字节当一条流，对 8 种
  `(格式, 形状)` 组合**真解压**到 `/tmp/yshjy-fuzz-write`
- 限额压到最小（单轮 4 MiB、比率 50、无人值守），炸弹输入早早被挡
- 每轮结束断言三件事：① 不许留 `.yshjy-part` 半成品 ② 总写出量不超限额
  ③ 递归清点沙箱（解出的文件必须都落在沙箱里）
- 语料喂了真实样本（zip/7z/tar/tar.gz/tar.xz/加密 zip/截断 zip）

实测：
| 时长 | 执行次数 | 崩溃 |
|---|---|---|
| 60 秒（试跑） | 91,436 | 0 |
| 600 秒（正式） | **71,661** | **0** |

正式跑共 601 秒、71,661 次执行、语料从 7 个长到 769 个，全程 0 崩溃 0 panic。
（速度比试跑低是因为语料里混进了几十 MB 的真实包，每轮都要真解一遍再清空 —— 这正是这个
target 存在的意义：只读 fuzz 再快也碰不到落盘。）

### 阶段 1 梯队三·第 3 项：`caps` 能力位如实
> 由来（体检时列的 TODO 第 8 项）：`Caps` 以前只是引擎内部的说法
> （`InArchive::caps()`，连单测都在守着「7z 还原不了时间戳别写 true」），
> 但**界面完全拿不到** —— 界面只能硬写「都支持」，用户点到不支持的格式才撞墙。

- 引擎新增 `yshjy --caps`（带 `--json` 给外壳消费）：12 个 `(格式, 形状)` 组合的
  六项能力位（密码 / 密文件名 / 分卷 / 可选解 / 还原时间 / 可打包）
- 设置页新增「各格式能力（问引擎要的，不是写死的）」区块：点一下问引擎，
  引擎不认 `--caps` 就如实说「老版本引擎没有 --caps」
- 契约测试加 `caps` 用例（第 12 项），能力位结构变了会被测出来

实测输出的核心事实（跟单测断言一致）：
| 格式 | 密码 | 密文件名 | 分卷 | 可选解 | 还原时间 | 可打包 |
|---|---|---|---|---|---|---|
| zip | ✓ | - | ✓ | ✓ | ✓ | ✓ |
| 7z | ✓ | ✓ | ✓ | ✓ | - | - |
| rar4 / rar5 | ✓ | - | ✓ | ✓ | - | - |
| tar | - | - | - | ✓ | ✓ | - |
| gzip/xz/zstd/bzip2/brotli/lz4 | - | - | - | - | - | - |

### 阶段 1 梯队四：RAR 还原修改时间
> 由来（体检时列的 TODO 第 9 项）：`extract_rar.rs` 顶部注释一直写着「现在能补时间戳和
> 权限位了」，但 `caps()` 里是 `restores_mtime: false` —— 注释是张空头支票，
> 解出来的 RAR 文件时间永远是「解压那一刻」。

- 回调里从 `ExtractedEntryMeta.file_time` 取原始时间，按 RAR 版本换算
  （RAR4 = DOS 时间、RAR5 = Unix 秒 + `mtime_refinement` 亚秒补位），
  统一交给库的 `rars::timestamp::extracted_system_time()`
- **整包解完再统一 `set_modified`**：解压途中改 mtime 会被库后续的写入冲掉
- 文件创建成功才登记时间（创建失败的条目不留登记，免得解完冒假警告）；
  还原失败只追加一条 warning，不影响「解出来了」这个结论
- `caps().restores_mtime` 改成 `true`，跟实现对齐；那条「RAR 还原不了时间戳，
  别写 true」的老断言同步改掉
- 权限位**仍然没做**（Unix 权限要从 `attr` + `host_os` 推，Windows attr 是另一套语义），
  模块注释里改成如实说明，不再暗示已经支持

实测：
| 检查 | 结果 |
|---|---|
| 解 `stored_recovery_1.rar` 出的 `protected.bin` | 时间 **2012-07-13 19:01:20**（当时是 2026-10-24 23:47） |
| 新增单测「rar 解出的文件带包内原始修改时间」 | 通过（断言必须早于 2013 年、又不能是 1970） |
| `--caps` 输出 | rar4 / rar5 的「还原时间」位由 `-` 变成 `✓` |

### 顺带修好：契约测试自己的漏洞
> `--caps` 吐的是 JSON **数组**，而契约采集器只认 `{` 开头的行 —— caps 这项以前
> 只验了退出码，能力位怎么变都测不出来，等于白挂。

- 采集器现在也解析数组行，且对 `--caps` 这种表**按原值固化**（布尔值也是契约：
  能力位一改，界面显示的能力表就错）
- 效果验证：改完 `restores_mtime` 后契约测试立刻报
  `· .events._caps 是新加的字段（契约变更）`，`--update` 后 golden 里
  `rar5.restores_mtime` 确实是 `true`

### 阶段 1 梯队四：RAR「最后一卷没下完」判得出
> 改之前的样子（实测）：
> ```
> ❌ stored_volume_0.rar —— 这不是压缩包（没找到可识别的压缩数据）（0 B）
>    ℹ 这个文件看起来是：RAR 压缩包（2 KB）
> ```
> 自相矛盾，而且完全没提「你少下了一卷」—— 用户只会以为文件坏了。

- 判据：RAR 文件头里的 `split_after` 位（「本条目内容续到下一卷」）。
  用 `Archive::members()` 的最后一个成员看这个位——**只有它能在卷不齐时说出真相**，
  因为前面每一卷单独看都是「可读、像完整包」
- 两个入口都加了自查：
  - 多卷入口：最后一卷的末成员 `split_after == true` → 后面还有卷没给
  - 单卷入口（目录里只有第一卷）：同样判定 → 明确说这是分卷、要整组放齐
- 为什么要提前拦：不拦的话 `rars` 解压**不报错、只是静默不写任何文件**，
  最后被兜底归类成 `NotArchive`（"这不是压缩包"）—— 用户被彻底误导
- 拿不到成员列表时一律返回 false：**宁可漏报，绝不乱报**
  （乱报会把好好的完整包拒收，比漏报更糟）
- 文案：「分卷不齐 —— 后面还有卷没下回来（分卷包要整组放齐再解）」
  （原来是两个字"缺分卷"，等于没说怎么办）

实测（`tests/rar_volume.sh`，8 项断言全过）：
| 场景 | 改之前 | 改之后 |
|---|---|---|
| 四卷齐全 | ✅ 解开 | ✅ 照样解开（`split.bin` 5000 字节） |
| 缺最后一卷 | ❌「这不是压缩包」 | ✅「分卷不齐」+ 错误码 `MissingVolume` |
| 只给第一卷 | ❌「这不是压缩包」 | ✅「分卷不齐」 |
| 缺卷时是否写文件 | — | 一个文件都不写 |

### 阶段 1 梯队四：zip 里的符号链接
> tar 早就支持（`extract_stream.rs` 一直有），zip 却一直当普通文件解 ——
> 于是装 Linux 软件包（很多包里有 `link -> target`）时，链接会变成一个**内容为目标
> 路径的小文本文件**，装出来的程序是坏的，而且不报错。

- zip 没有「这是符号链接」的类型位，只能看 external attributes 里的 Unix mode：
  `mode & 0o170000 == 0o120000` 就是 S_IFLNK
- 链接目标**直接写在条目内容里**（zip 的约定，不压缩），读出来就是目标路径
- 安全规则跟 tar **共用同一套**（`链接目标安全` 改成 `pub(crate)` 复用）：
  只接受「相对路径 + 字面规范化后仍在解压目录内」；绝对路径、`..` 爬出去的一律拒绝
- 拒绝时说清原因（「链接指向「/etc/passwd」，会跑到解压目录外，已拒绝（防逃逸）」），
  不静默、不假装解出来了
- 符号链接也走**重名策略**（rename/replace/skip 都在它之前生效）

实测（`tests/zip_symlink.sh`，9 项断言全过）：
| 条目 | 结果 |
|---|---|
| `note.txt`（普通文件） | 正常解出 |
| `good_link.txt -> note.txt` | **真符号链接**（`lrwxrwxrwx`），`cat` 穿过去能读到内容 |
| `abs_evil -> /etc/passwd` | 拒绝，说明「会跑到解压目录外」 |
| `rel_evil -> ../../etc/passwd` | 拒绝，同上 |

### 阶段 1 梯队四：五态审计（空 / 加载 / 成功 / 失败 / 部分成功）
> 目标不是「好看」，是**每种结局界面都有话说** —— 不许空白、不许假装没事、
> 不许把程序员看的错误码直接甩给用户。

审计方法：逐页读状态分支，把五态逐个对照（页面状态都是私有字段，没法从测试里注入，
所以这一项靠代码审计 + 现有 widget 测试兜底，不靠截图）。

| 页面 | 空 | 加载 | 成功 | 失败 | 部分成功 |
|---|---|---|---|---|---|
| 主页 | ✅ 引导选文件 | ✅ `Phase.extracting` + 进度 | ✅ 结果区 | ✅ `_errorCode` + 人话 | ✅ `_notes` 逐条 |
| 历史页 | ✅ 「还没有历史」 | ✅ `_loading` | ✅ 列表 | ⚠ 有意降级（见下） | — |
| 打包页 | ✅ 「还没选文件」 | ✅ `_busy` 全按钮禁用 | ✅ 提示「打包完成」 | ❌→✅ **裸错误码，已修** | — |
| 设置页 | — | ✅ 「读取中…」 | ✅ 能力表 | ✅ 「读不到能力表：…」 | — |
| 关于页 | — | ✅ 「生成中…」 | ✅ 报告路径 | ❌→✅ **裸错误码，已修** | — |

**改掉的两处**（失败态把程序员的话甩给用户）：
- `pack_page.dart`：`打包失败：${e.code}` → `打包失败：${errorText(e.code)}`
- `about_page.dart`：`生成失败：${e.code}` → 同样翻成人话（并补 `import '../humanize.dart'`）

修前用户看到的是 `打包失败：SPLIT_SIZE_INVALID` 这种；修后是「打包失败：<人话>」。

**唯一一处「失败态静默」是有意的**：`HistoryStore.load()` 内部 try/catch 把所有异常
降级成空列表，注释写着「绝不能因为历史读不出来让应用打不开」——
这是产品的正确取舍（历史坏了不该让工具打不开），所以保留，但记在这里，
免得以后有人当 bug 又改回去。

验证：`flutter analyze` 只余 1 条预存 info（`engine.dart:253`，非本次改动）；
`flutter test` 4 项全过。

### 阶段 1 梯队四：首屏单一入口（补测试钉住）
> 审计结论：**这项其实早就做对了** —— 首屏空态里「选择文件」是唯一的 FilledButton，
> 「选整个文件夹」是弱化的 TextButton，注释还引着调研结论（第一屏只该有一个最强入口）。
>
> 但它只是**注释里的规矩**，没有任何东西拦着以后有人顺手再塞一个 FilledButton 进去。
> 所以这项的实际交付是「把它变成会失败的测试」，不是重新设计界面。

新增 widget 测试 `首屏只有一个最强入口`，三条断言：
1. `选择文件` 是 FilledButton（主入口在）
2. 空态里 **FilledButton 总数 == 1**（多一个新手就不知道该点哪个）
3. `选整个文件夹` 必须是 TextButton（次入口不许抢）

**验证这条测试不是摆设**：临时把次入口改成 `FilledButton.icon`，测试立刻报
`Found 2 widgets with type "FilledButton"` 并按 reason 说明原因；改回后全绿。
顺手在源码那条注释下补了一句「别改成 FilledButton —— widget_test 里有测试钉着」。

验证：`flutter test` **5 项全过**（新增 1 项）；`flutter analyze` 只余 1 条预存 info。

### 阶段 1 梯队四：默认打开方式（双击 zip 直接是我们）
> 现状：`.desktop` 里**早就写了 MimeType**，但装完 `xdg-mime query default application/zip`
> 返回的还是 `org.gnome.Nautilus.desktop` —— 因为 GNOME 的文件管理器默认就抢着处理
> 压缩包，光声明「我能打开」不等于「默认给我打开」。

- `gui/linux/yshjy.desktop` 的 MimeType 补全：原来 7 种 → **14 种**
  （补了 `x-zip-compressed`、`x-rar`、`x-rar-compressed`、`x-tar`、`x-compressed-tar`、
  `x-brotli`、`x-lz4`）
- `scripts/build-deb.sh` 的 postinst：给**装包那个人**（`$SUDO_USER`）设默认，
  不用 system 级、不动其他用户
- `scripts/install-gui.sh`（用户级安装）也会调一次
- 新增 `scripts/set-default-mime.sh`：可手动设、`--status` 看现状、`--restore` 一键还原
  （卸载前想恢复就用它）

**踩到的坑（重要）**：desktop 里的 `Exec` 必须指向**真实存在的可执行文件**。
第一次测试时我拿模板直接拷（`Exec=__EXEC__`），`update-desktop-database` 虽然把它
收进了 `mimeinfo.cache`，`mimeapps.list` 也写对了，但 `xdg-mime query` 依旧回退到
Nautilus —— GLib 判定 desktop 无效就静默忽略。换成真实的 `/usr/bin/true` 后立刻生效。
这条已写进探针注释里，免得以后有人以为脚本坏了。

实测（`tests/mime_default.sh`，7 项断言全过，全程隔离 `XDG_DATA_HOME`/`XDG_CONFIG_HOME`，
不碰用户真实配置）：
| 检查 | 结果 |
|---|---|
| desktop 声明 zip / 7z / rar | ✅ |
| `xdg-mime query default` 三种格式 | ✅ 全部 `yshjy.desktop` |
| `gio mime application/zip` | ✅ 「默认应用程序：yshjy.desktop」 |
| `--restore` 之后 | ✅ 退回 `org.gnome.Nautilus.desktop` |

### 阶段 1 梯队四·第 9 项：托盘 / 后台（2026-10-27）

> 由来：解大包时用户想干别的，最小化或关窗口就整个任务没了。目标是「关窗口 = 藏到托盘继续跑」。

四批提交（`ba64554` / `0b052e4` / `e11c16f` / `2e9b048`）：

| 批次 | 做了什么 |
|---|---|
| 第十三批 | GNOME 顶栏图标 + 右键菜单（显示窗口 / 退出）；`libayatana-appindicator3` 从 CMake 引入 |
| 第十四批 | 解压中关窗口 → `didRequestAppExit` 里改成藏到托盘 + `AppExitResponse.cancel`；新增 `gui/lib/tray.dart` |
| 第十五批 | 托盘状态行显示**真实百分比**（`trayStatusForEvent` 纯函数，涨 2% 才刷一次，100% 必发，跳过的进度不推进基线） |
| 第十六批 | 托盘「退出」不再硬掐：先调 Dart `request_quit` → 引擎 `cancel()` → 等 400ms 在数据块边界收尾 → `quit_now` 真退；`caps.background` 改 true |

技术要点：
- **通道**：`MethodChannel("yshjy/tray")` 双向 —— Dart→C++ 的 `hide_window` / `set_status` / `quit_now`，
  C++→Dart 的 `request_quit`。`quit_now` 里 `g_application_quit()` 放在 `fl_method_call_respond` **之后**，
  否则响应还在路上进程就没了。GApplication 存成 `static g_app`（activate 里赋值）。
- **进度节流**：`kTrayThrottleStep = 2.0`，变化幅度够 2% 才发；100% 无条件发；
  被跳过的中位事件**不推进基线**（否则几次小步会被吞掉）。
- **退出为什么绕一圈 Dart**：引擎是子进程，进程一没它读 stdin 撞 EOF 就自己退，
  正在解的那项被硬中断（原子写入只保得住 `.yshjy-part`，不算干净）。

坑（都记在代码注释里）：
- `GtkMenuItem` 没有 `"clicked"` 信号，得连 `"activate"`
- `app_indicator_new` 在 0.5.94 已弃用，`-Wall -Werror` 下会直接编不过 → 局部 `#pragma` 压掉
- 判断托盘注册**不能** grep DBus `ListNames` 找 "yshjy"（ayatana 用匿名连接），
  要读 `org.kde.StatusNotifierWatcher` 的 `RegisteredStatusNotifierItems`

验收：界面测试 **13 通过 / 0 失败**（含节流规则、JOB_FINISHED→空闲、mock 通道）；
`flutter build linux --debug` 成功；`cargo test` **333 通过 / 0 失败**；`cargo clippy -- -D warnings` 干净。
用户实机确认：顶栏图标在、右键三个菜单项都对。

### 已知缺口（已记进 `docs/TODO.md` 表尾）
- **RAR 路径尚未接上**：`unrar` 的解压回调由库自己管理 writer 生命周期，无法在
  「条目写完那一刻」安全 `finish`（改错会删掉好文件）。需要先确认库的写结束语义，
  或改成「`written == entry.unpacked_size` 时立即 `finish`」，单独一批做。
- 改名之前没有 `fsync`：真断电（不是进程被杀）仍可能得到 0 字节正式文件。
  彻底防断电要接受「10 万碎文件从 8.6 秒涨到几十秒」的代价，暂不做。

## [0.9.25-c] · 2026-10-24（L 批 1–2：RAR 彻底修 + 文案审计第一刀）

> 由来：2026-10-24 实测盘点（`docs/LINUX-FULL-PLAN.md` §1 的 X1–X5），用户拍板
> 「其他平台先不管，先按核心功能往下修，发现的新问题记下一轮」。

### RAR 分卷支持（X1，原来**完全解不了**）
- 识别层新增 `as_rar_volumes`：认三种命名 —— `.partN.rar` / `.rar`+`.r00`… / 数字后缀（`名字_0.rar`、`名字-0.rar`）
- 从**中间任一卷**点进去也能补回第一卷；单卷不会被误拼成一组（按 RAR 魔数逐卷校验）
- 引擎改走 `rars` 的多卷流式 API（`extract_volumes_to`）；坏卷/缺卷报 `MissingVolume` 并指明「第几卷读不出来」
- 实测：RAR5 四卷标准样本（4 卷拼 5000 B）→ 解开，`sha256 = 0ea1fda2…`，与系统 `unrar` 逐字节一致
- 顺带：缺卷自检能处理 `.part1` / `_0` 这类「数字在扩展名之前」的命名了

### RAR 去掉 256 MiB 硬上限（X2，原来大于 256 MB 直接拒）
- 有文件路径时改走 `ArchiveReader::read_path`（按需读载荷，不再整包进内存）
- 实测：`314,572,943 B`（300 MiB）的 RAR 解开，`314,572,800 B` 字节数一致，耗时 6.32 秒
- 只有**不可 seek 的流**（管道/stdin）才回退整读，上限 256 MiB，文案改成
  「请直接把这个 RAR 的文件路径交给程序（给路径时没有大小限制）」

### RAR 预览（顺带修好）
- 预览改 `preview_rar_paths`（流式 + `rars::volume_members`）：分卷也能列清单
- 现在能填**真实大小 / packed / 加密标记**，`complete` 从 `false` 变 `true`
  （以前走高层 API 拿不到，只能报 0）

### tar 里的符号链接/硬链接（X3，原来全部丢失）
- 现在**真的还原**（Linux 软件包的标准结构：`libfoo.so -> libfoo.so.1`）
- 安全规则：只建「相对路径 + 字面消点后仍在解压目录内」的链接；
  绝对路径、`..` 逃逸、Windows 盘符式一律拒绝并**说明原因**
- 实测：`lib/libfoo.so.1 -> libfoo.so.1.2`、`safe.so -> lib/libfoo.so.1` 正确建出；
  `/etc/passwd`、`../../../../etc/passwd` 被拒（落在 `skipped` 里带原因）

### 文案（X4 / X5 / D21 第一刀）
- **「跳过」不再被当成「数据损坏」**：`stats.failed` 统一记 `Skipped`
  （以前包里有一条链接/同名文件，整条就被标 `DATA_ERROR`）
- 新增 `ItemResult::人话()`：成功 / 跳过 / 校验不过（内容坏了）/ 数据损坏 / 加密，没解开 / 没解出来
- CLI 三处显示（详细日志、实时提示、批量失败行）全部换成人话，不再直露 `DATA_ERROR`

### 失败原因结构化（D8）
- `JobStats.failed` 从 `(名字, 码)` 升级成 `(名字, 码, 原因)` —— 界面/脚本不必再猜
- 协议与 `--json` 多出 `human` / `reason` 两个字段
- CLI 现在打：「⚠ 文件名（跳过）—— 链接指向 /etc/passwd，会跑到解压目录外，已拒绝（防逃逸）」

### 打包进度（L2 / 清单第 6 条，原来打大包界面是死的）
- 新事件 `PACK_PROGRESS`（`progress` + `current`），`hello.event_kinds` 同步
- 引擎：`build_plain_ctx` 逐 4 MiB 上报；`write_zip` / `add_dir` 都接回调（大目录也逐文件报）
- 协议：`pack` 命令先按输入预扫出总字节/总文件，再逐块上报，结束补一条终态
- CLI：进度条同时接 `EXTRACT_PROGRESS` 与 `PACK_PROGRESS`
- 实测：200 MB 文件 `mode:"plain"` 打包 → 收到 `PACK_PROGRESS`（done_bytes 走到 200000000），产物 200,000,112 B 正常
- 待接：GUI 打包页的 Dart 侧监听（下一轮，和界面收口一起做）

### 界面文案表（N1，D21 收尾）
- 新建 `gui/lib/humanize.dart`：错误码 17 条 / 诊断码 10 条 / 条目结果 6 条 / 批量结论，全中文
- **查不到的码不再显示原始码**（D21「缺一条就不显示」落地）：统一给一句通用人话
- 修掉一个真 bug：界面查的是 `PASSWORD_REQUIRED` / `PASSWORD_WRONG`，引擎发的是
  `NEED_PASSWORD` / `WRONG_PASSWORD` —— 对不上，以前密码错会显示「错误码：NEED_PASSWORD」
- `home_page` / `pack_page` 全部改查这张表，两页不再各写一份

### 打包页进度条（N2）
- 打包页订阅 `PACK_PROGRESS`：进度条 + 百分比 + 已写/总量 + 当前文件
- 开始/结束都清读数，不会挂着上一次的百分比
- `flutter test` 4/4、`flutter build linux --release` 通过

### `tests/all.sh`（N3，清单「敢改的底子」）
- 一条命令跑完：编译 → clippy(`-D warnings`) → 单元测试 → 6 个协议探针 → 冒烟 → 界面测试
- `--slow` 追加 300 MiB 大 RAR 与 RAR 分卷回归；`--no-gui` 只验引擎
- 失败项会打「最后 15 行 + 完整日志路径」，一次看全所有问题
- 实测：**13 通过 / 0 失败 / 3 跳过**，耗时 115 秒
- 两个坑记在脚本里：bash 的 `local` 不认中文变量名；`cargo fmt --check` 只作提示
  （项目是手写紧凑风格，rustfmt 会重排上千行）

### 批量单独取消某一行（N4 / D5，0.9.25-c）
- `CancelToken` 新增：`skip_jobs`（按任务号）+ `cancel_job()` / `take_job_skip()` /
  `mark_running()` / `running_job()` / `clear_running()`
- `batch.rs` **顺序和并行两条路**都在每项开始前查「这一项被单独取消了没」
- 协议 `cancel` 支持 `job` 字段：无 `job` 仍是整批停；有 `job` 只跳那一项，
  且**只有正在跑的就是它**才真打断（否则会把别人的任务也停了）
- 界面：队列行加「取消这一项」按钮（排队中 = 不跑；运行中 = 立刻停），
  `JOB_STARTED` 记下任务号，结果显示「已取消」（不再错误地显示「失败」）
- 证据：单元测试 `界面单独取消某一行只跳过那一条`；协议探针 `tests/protocol_cancel_job.py`
  （80 MiB × 3 个包，途中取消第 2 项 → `{1:'ok', 2:'cancelled', 3:'ok'}`，第 1、3 项照跑）
- 顺带核实：D4（Del 键删队列条目）早批次已实现，本轮只确认在册

### 拖入文件夹的提示（N6 / D6）
- 拖/选文件夹进来后，队列顶部直接写明会怎么处理：
  「这是个文件夹，会自己翻开找里面的压缩包和分卷（只读文件头，不动你的文件）；
  不是压缩包的文件会列出来但跳过」
- 清空队列时提示跟着清掉（不会残留）

### 文件管理器右键菜单（N7）
- 新增 `gui/linux/nautilus-script-yshjy`：选中压缩包 → 右键 →「脚本」→「用杨三海解压」
  - 解到包所在文件夹（不把文件撒一地）
  - 不弹终端；结果用 `notify-send` 说一句；详细输出写 `~/.cache/yshjy/nautilus-last.log`
  - 引擎查找顺序：`YSHJY_BIN` → PATH → `/usr/bin` → `/usr/local/bin` → `~/.local/bin`
- 新增 `scripts/install-nautilus.sh`：用户级安装/卸载（**不用 sudo**），已装到本机
- `scripts/build-deb.sh` 打 deb 时一并装到 `/usr/share/nautilus/scripts/`（系统级）
- 已核实 Nautilus 48.3 仍有 scripts 子菜单（二进制含 `scripts-submenu-section`）
- 实测：模拟 `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS` 跑一遍 → 解出 `a.txt`、日志正常
- `tests/all.sh` 增加「右键脚本语法检查」一项

### 待办清单收口（L3 轮结束，2026-10-24）
- 以前项目里有 **4 套清单**同时存在（`LINUX-FULL-PLAN.md` §2 老账 18 项 / §3 的 L1–L7 分批 /
  §7 下一轮 N1–N7 / §8 待观察），章节号还排错（5 → 7 → 8 → 6）。现在全部合并成
  **`docs/TODO.md` 一个文件**，`HANDOFF.md` 和 `LINUX-FULL-PLAN.md` 只留指针与历史证据
- 清单分两段：**阶段 0 体检 T1–T9**（依赖安全 / 测试覆盖 / 性能压测 / 模糊测试 / 装包发布 /
  新手走一遍 / 多语言完整度 / 异常场景 / 数据安全复核，只出证据不写功能）
  + **阶段 1 要做 11 项**（协议契约测试、caps 能力位、伪装包进度、RAR 时间戳与末卷判定、
  zip 符号链接、五态审计、首屏收口、默认打开方式、托盘、断点续传）
- 明确为什么加体检：阶段 1 那 11 项是「已经踩到的坑」，不是「项目所有的坑」；
  体检没做完之前，说「修得差不多了」都只是猜
- 下一轮起点：**T1 依赖安全（`cargo audit` / `cargo deny`）**

### 回归证据
- `cargo test --release`：**303 passed / 0 failed**（新增「认出 RAR 四卷分卷」；
  改写「超大 RAR 不再被大小检查拦住」；新增 ignored「300 MiB 真包解压」）
- `tests/smoke.sh`：**25/25**
- 6 个协议探针（select / level / serve_eof / pack_cover / sfx / sfx_linux）：全过
- `cargo clippy`：无警告

## [0.9.24-c] · 2026-10-24（第 23 批「能力补齐」）

用户原话：「都要」。7 项里 6 项落地，第 7 项（7z 写包）给出评估结论。

### 文件级挑选（P23-1）
- 协议 `extract` / `batch` 支持 `select`：字符串表达式（`all` / `1,3,5` / `*.exe` / `目录/**` /
  `required`）或正整数数组（序号，与 `PREVIEW_READY` 的 `index` 一致）；不传 = 全解
- 非法表达式回 `BAD_PARAM`（以前回 `UNSUPPORTED`，也不会静默全解）
- 界面：解压页列表逐条勾选 + 已选 N/M + 全选/全不选；一个都没勾时主按钮置灰
- 实测：取消勾 `readme.md` → 已选 3/4 → 解压 → 产物只有另外三个文件

### 单任务输出目录（P23-2）
- 解压页「高级选项」里可指定本次输出目录（默认仍是「包旁边」）；输码重试沿用同一目录

### 文件名编码手动切换（P23-3）
- 高级选项里下拉：自动 / GBK / Big5 / Shift-JIS / UTF-8，接引擎 `--encoding`

### 压缩速度/体积档（P23-4）
- 协议 `pack` 支持 `level: 0-9`（0 = 仅存储、9 = 最小体积，默认 6，结果回显实际值）
- CLI 新增 `--level`，越界报错；界面打包页加「压缩级别」下拉
- 顺带修：`level=0` 以前会以 `Unsupported compression level` 整个失败（Stored 不该带压缩级别）

### 自解压增强（P23-5）
- **支持解压码**：存根不再拒绝加密包，改为提示输入（脚本/批量用 `YSHJY_SFX_PASSWORD`）；
  密码错在建目录之前就失败，不留空文件夹
- **有进度显示**：逐条打百分比（原来双击后黑屏无感）
- **Linux `.sh` 自解压**：`--sfx-linux` / 协议 `sfx:"linux"`，产物可执行、双击即解
- 界面自解压开关下加「目标系统：Windows(.exe) / Linux(.sh)」
- macOS 自解压需要 mac 构建机产出 Mach-O 存根，排在最后（不是不做）

### 协议健壮性（P23-7）
- `--serve` 一次性喂命令不再丢结果：stdin 断开后先等当前任务收尾再退出（D16 结案）
  （第二条命令仍按协议回 `BUSY` —— 一次只跑一个任务是设计，脚本应逐条等待）

### 7z 写包（P23-6）：本批不做，结论如下
- `sevenz-rust2` 0.24 确实有写入能力（`SevenZWriter` / `push_archive_entry` / `set_encrypt_header`）
- 但四条交付路径（伪装外壳 / 自解压 / 分卷 / 说明书+校验和）全部围绕 ZIP 实现，
  加 7z 等于把这四条重做一遍；而用户实际场景（网盘躲审核）用的就是 ZIP 伪装
- 结论：排到第 24 批「成熟度基线」再评估；文档写清，不做假选项

### 安装层修正（顺手，用户已确认）
- `~/.local/bin/yshjy` 曾指向旧手工安装（0.9.21-c）→ 改指 `/usr/bin/yshjy`
- 桌面/菜单项曾指向旧 `~/.local/opt/yshjy` → 改指 `/usr/lib/yshjy/gui/yshjy_gui`
- `.gitignore` 收掉第 24 批参考项目（283 MB，磁盘未删）

### 验收
- `cargo test` 316 通过 / `cargo clippy --all-targets -- -D warnings` 零警告
- 新探针入库：`tests/protocol_select.py`、`protocol_level.py`、`protocol_serve_eof.py`、
  `protocol_sfx_linux.py`；自解压旧探针补密码用例
- 回归 6 个探针全 PASS（含 wine 跑真 Windows PE 存根）
- 界面实机验证：勾选解压闭环、完成页、压缩级别下拉、自解压类型下拉

## [0.9.23-c] · 2026-10-18（23.5 批「成熟感最小集」，工作室接手）

只补壳，引擎未改（`src/` 一行没动）。

- **deb 现在带图形界面**：安装包含 `yshjy_gui` + `libapp.so`、应用菜单项
  （`杨三海解压`，7 种压缩格式关联）、256×256 图标、`postinst/postrm` 刷新菜单缓存。
  装完直接在应用菜单里点开，不用再手动跑 `install-gui.sh`。包体积 1.9MB → 9.5MB。
- **桌面通知**：解压完成 / 部分完成 / 解压失败 / 打包完成时发一条系统通知
  （`notify-send`，8 秒消失）。没装 `notify-send` 时静默跳过，不影响主流程。
  设置页新增「处理完成后发桌面通知」开关，保存即生效。
- **版本一致性校验**：`build-deb.sh` / `release.sh` 打包前比对
  `Cargo.toml` 与 `gui/pubspec.yaml`，不一致直接拒绝打包（防止再发旧版本）。
- **出包自查**：deb 打完后自动检查 CLI / GUI / libapp.so / desktop / 图标 / 版本号，
  缺一项就失败退出。
- 依赖补 `libgtk-3-0 | libgtk-3-0t64`（兼容 Ubuntu 24.04 之后的新包名），
  `Recommends: libnotify-bin`。
- 新增 `gui/test/notify_test.dart`：验证「通知命令不存在时不抛异常」与「开关关闭时不调用」。

## [0.9.22-c] · 2026-10-18（第 22 批：伪装文件自选 + 自解压 exe + 同类「限制选择」修复）

- **伪装文件自选**（用户原话「不要内置，用户想选啥就选啥」）：协议 `pack` 新增
  `cover`（外壳文件路径，任意类型）；界面删掉「视频/图片/文本」三个内置素材选项，
  改成「选择伪装文件（任意类型）」+ 常驻说明；输出扩展名强制跟随外壳，
  未选外壳时不允许开始打包。引擎按文件头决定布局（MP4 → free 原子，其他 → 尾部追加）。
- **自解压 exe（Windows）**：新增独立 crate `sfx/`（自己写的 Rust 存根，不借 7-Zip）、
  `src/pack::build_sfx`、CLI `--sfx`、协议 `pack.sfx`、界面开关。产物结构
  `[存根][ZIP][尾标 24B]`，双击后解到同名文件夹并打开目录。防护：路径穿越、
  截断/尾标校验、重名自动加序号、中文路径。暂不支持解压码与分卷（明确报错，不静默降级）。
- **解压页选文件去白名单**：`.pdf/.docx/.mp3/.exe` 等外壳的伪装包不再被文件对话框过滤掉
  （以前只有拖拽能进，点「选择文件」看不见）。
- **分卷大小填错不再静默变「不分卷」**（D20 同类：看不懂就报错）；
  普通压缩的输出后缀强制 `.zip`，不会名不符实。
- **输出文件不能覆盖源文件/外壳**（引擎侧防呆）。
- 测试：`tests/protocol_pack_cover.py`、`tests/sfx_stub_test.py`、
  `tests/protocol_sfx.py` 全部通过。
- 计划：第 23 批（文件级挑选 / 单任务输出目录 / 编码切换 / 压缩级别 /
  自解压增强 / 7z 写包 / 管道多命令）已写入 `docs/GUI-PLAN.md` §8。

## [0.9.21-c] · 2026-10-17（第 21 批完成：平台地基 + 下钻 + 双向协议 + 界面收口 + CLI 扫尾）

- **跨平台路径**：新增 `src/paths.rs`（Windows `LOCALAPPDATA` / macOS
  `Application Support` / Unix `XDG_STATE_HOME`），崩溃日志与诊断包不再各写一份
  Unix 假设；`YSHJY_STATE_DIR` 可覆盖（测试/便携版）
- **命令语义抽成 `src/api.rs`**：`--serve` 与 C ABI 共用同一份 list/extract/pack/
  batch/scan/report 实现，两个传输不会各走各的
- **稳定 C ABI v1**：新增独立 crate `ffi/`（`cdylib` + `staticlib`）、
  `ffi/include/yshjy.h`、`docs/FFI.md`。JSON 进 JSON 出，事件走回调，
  panic 被 `catch_unwind` 拦住不穿 FFI。实测（ctypes 直调 `.so`）通过
- **封面预探**：`list` 结果新增 `cover{size,kind,ext}`（只读 metadata，不落盘）；
  `extract` 新增 `extract:false`（只提封面、不解压、不建输出目录）
- **界面传输层抽象**：`gui/lib/transport/engine_transport.dart`（`EngineTransport`
  + `ProcessTransport`），`EngineClient` 不再死绑 `Process.start`，iOS 可换 FFI 实现
- **界面路径跨平台**：`gui/lib/platform_paths.dart` 统一状态/配置目录；5 处写死
  `$HOME/.local/state` 与 4 处 `xdg-open` 全部替换（Windows/macOS 走各自 API）
- **嵌套包自动下钻（组 C）**：新增 `src/drill.rs`，`extract` 加 `drill:true`。
  只下一层、最多 8 个、跳过输入包自身（dest==源目录不会自我递归）、失败只记
  `nested[].outcome=failed`；协议实测 外层 mp4 → 内层 zip → 深层文件 全通
- **双向协议（组 B）**：新增 `src/ask.rs` + `ASK_USER` 事件 + `answer` / `skip` 命令。
  限额命中时引擎停下来问（60 秒不答=跳过，一次任务只问一次，迟到答案不污染下一次）；
  `extract`/`batch` 只有声明 `ask:true` 才启用询问（老前端行为不变）；
  `skip` 只跳当前一项、批量继续。取消/跳过在 ZIP/7z/单流循环里改为 `Cancelled` 语义
  （不再误报 `DATA_LOST`）。
- **界面收口（组 A）**：解压页「伪装封面 / 压缩包内容」两行独立勾选（只勾封面→只提取、
  只勾内容→不提封面、都不勾→按钮置灰）；`ASK_USER` 弹窗三选一（继续 / 跳过 / 取消整批，
  60 秒不答按跳过）；批量进度页新增「跳过这项」；打包页模式二选一，**默认普通压缩（.zip）**、
  伪装是特殊功能。引擎 `pack` 新增 `mode:"plain"`（`pack::build_plain`：标准 ZIP、零封面）
- **修 D17（假失败）**：小包解压被压缩比防护误判成压缩炸弹（11 B 文本 + 5000 个相同字节，
  整包压缩后约 40 B → 5000/40 = 125 > 100 → `DATA_ERROR`，而 `unzip -t` 完好）。
  比值改为只在本次写出 ≥ 64 MiB 后判断，真炸弹照样拦
- **CLI 扫尾（组 E）**：`--config` / `--layout L1|L2` / `--cover` / `--password-file` /
  `--no-auto-password` / `-v` `-vv` / `--log` / `--no-color` / `--keep` 全部接线并实测
  （16 项断言）；7 种语言包的 `--help` 同步。`--layout L5/L6` 明确拒绝并解释（实测会做残），
  `L3/L4` 报「未实现」；`--keep` 保留并打印中间 ZIP 路径
- **修 D19（找不到解压结果）**：CLI 默认输出从「进程当前目录」改成**包所在目录**
  （跟界面一致），完成行显示 `canonicalize` 后的绝对路径（以前只打 `📁 .`）。
  用户实测：站在 `~` 下解 8 GB 的包，产物铺在家目录，看完输出也不知道去哪找
- **修 D20（拼错参数静默变成解压）**：`-extract-cover`（少一个横线）以前被当成「不认识的
  参数」默默丢掉，然后默认解压照跑 —— 实测把 5.2 GB 的包又完整解了一遍。现在不认识的
  选项一律报错并给拼写提示（`你是不是想写 --extract-cover？`），退出码 2；`--` 之后
  的内容一律按文件名处理
- **交互式输解压码**：终端里遇到带锁的包会先问（`🔑 包带锁，需要解压码`，输入不回显、
  直接回车=放弃），不再「一句话失败」；`-q` / `--json` / 管道输入下保持原来的快速失败，
  脚本不会挂死

## [0.0.19] · 2026-10-17（版本号规则变更 + GUI 更新）

**版本号规则（用户 2026-10-17 定）**：批次号与版本号对齐 —— 第 19 批 = `0.0.19`，
第 20 批 = `0.0.20`，依次类推。此前用的是 `0.9.0` 这类语义化编号。
注意：`0.0.19` 在 semver 里数值上小于 `0.9.0`，这是**内部迭代编号**，不是降级。

### 本版内容（第 19 批成果 + 之后的修复）

- **GUI 已重编译并补引擎进 bundle**：状态栏从「引擎未就绪 (ProcessException)」变成
  `● 就绪 · 引擎 0.0.19`。真因是 bundle 里根本没有引擎文件（三级查找全落空），不是崩溃
- **修 GUI 双引擎竞态**：`EngineClient.start()` 的 `await Process.start` 期间并发调用会
  再起一个引擎（实测同时两个 `--serve`）。改为缓存启动 Future，失败可重试，退出时重置
- **文档**：新增 `docs/AUDIT-2026-10-17.md`（设计与实现对照审查）、
  `DESIGN.md` 增补第十七章（密码编码 / 输出后处理 / 名字解析 / 加密强度 / 封面分离 /
  大包体验 / 安全限额 / 打包开关现状）、`GUI-PLAN.md` 增补第 20 批任务清单
- **引擎开始产库（全平台前置）**：`Cargo.toml` 增加
  `[lib] crate-type = ["rlib", "cdylib", "staticlib"]`，实测产出
  `libyshjy.so`（659 KB）、`libyshjy.a`（41 MB）、`libyshjy.rlib`（8.8 MB）；
  bin 与 `--version` 不受影响。iOS 只能走静态库 FFI（系统禁止 fork/exec 子进程）
- **设计新增第十八章「全平台架构」**：用户 2026-10-17 明确「全平台都要做」
  （Linux / Windows / macOS / Android / iOS）。现状 GUI 死绑 `Process.start`，
  iOS 跑不了 → 需抽 `EngineTransport` 接口 + 引擎产库（产库这步已做）

## [0.9.0] · 2026-10-17（定稿，待发布）

**主题：把「自动试密码」的边界说清楚、钉死。**

### 修复 — GUI 双引擎竞态（随界面更新一起做）

- `EngineClient.start()` 用 `if (_started) return` 挡重复启动，但 `await Process.start`
  挂起期间 `_started` 还是 false → 并发调用各起一个引擎。实测界面启动后**两个**
  `yshjy --serve` 同时存在、各持一条管道（违背「引擎单例」设计，双份内存 + 双份状态写者）
- 改为缓存启动 Future：`start() => _starting ??= _doStart()`；启动失败清缓存允许重试；
  `_onClosed` 里一并重置（否则拿到已完成的旧 Future，永远重起不了）
- 实测：修复后 GUI 1 个 + 引擎 1 个，状态栏 `● 就绪 · 引擎 0.9.0`
- 流程坑（记在 GUI-PLAN D11）：`flutter build linux --release` 会**清掉** bundle 里
  手工放的引擎，必须「先 build 再 cp 引擎」，否则启动就是「引擎未就绪」

### 新增 — 第 19 批：缺分卷检测（P3）+ 智能解压规则上屏（P2）

- **`MissingVolume` 以前是个死错误码**：定义了、有语言包键，但**从来没被抛出过**。
  分卷缺中间块时 `as_7z_volumes` 只是"停下"，后续解压报 `Truncated/DataLost`，
  用户看到"文件损坏"完全被误导。现在 `identify()` 统一做缺卷自检：
  末块序号 +1 不存在、但更后面的块存在 → `MISSING_VOLUME`，
  detail 带 `missing`（缺哪一块）和 `found`（已找到几块）
- 实测：5 块 `.7z.001–005` 删掉 `.003` → 协议返回
  `{"code":"MISSING_VOLUME","detail":{"missing":"卷.7z.003","found":"2"}}`，人类可读输出「缺分卷」
- GUI 失败卡：`缺这一块：卷.7z.003 / 已经找到 2 块。把缺的那几块一起下下来…再拖进来`
- 修两处误导：错误标题不再暴露 `MISSING_VOLUME` 这类码（改「分卷不齐 · 缺了一块」）；
  缺卷时**不再显示**「这个文件看起来是：认不出的文件」（分卷中间块没有文件头，探测必然是认不出）
- 队列页说明智能解压规则（引擎侧 `smart_dir` 早已生效，实测无多余嵌套目录）

### 新增 — 第 18 批：批量队列（P1 / S13）+ 多候选（S10）

- **协议新增 `scan`**：给一批路径 → 展开目录、归并分卷、逐个只读文件头认类型，
  返回 `{path, name, size, kind, volumes, looks_like, archive}`。11 个文件实测 → 4 个候选
  （5 块 `.7z.001–005` 归并成 1，`说明.txt` 被跳过，伪装 `.mp4` 认成 zip）
- **协议新增 `batch`**：队列一次交给引擎跑完，每任务独立成败；返回
  `{total, ok, partial, failed, cancelled, bytes, elapsed_ms, jobs[]}`。
  实测 4 任务全成功：分卷解出 `big.bin` 8192 B、伪装 mp4 解出原文、重名自动 `a (1).txt`
- **GUI 队列**：拖多个文件 / 拖文件夹 / 「选文件夹」→ 队列列表（类型 · 大小 · `分卷 N` 徽章 ·
  等待/处理中/完成/失败）+「开始解压全部 (N 个)」；完成后是 **Sunpack 三段式汇总卡**
  （结果标题 / `4 成功 · 用时 · 写出` 量化副标题 / 打开输出目录 + 再来一批），失败项单列
- 批量每个任务都写一条历史
- `install-gui.sh` 修坑：引擎永远用 `target/release` 覆盖（原来只补缺失 → 界面新引擎旧）

### 新增 — 第 17 批：GUI 从「一个按钮」变成完整应用外壳

- **左侧导航 + 五个功能区**：解压 / 打包 / 历史 / 设置 / 关于（窄栏，适配小窗口；
  参考 Ark、Keka 的极简单窗做法，没有抄 7-Zip FM 的双栏 —— 窗口小的时候双栏放不下）
- **底部状态栏**：引擎版本、当前状态、状态目录，一眼看到引擎活没活
- **打包页**：加文件夹/文件 → 选伪装类型（视频/图片/文本）→ 可选解压码 → 输出路径 →
  结果卡片（大小/条目/free 原子布局）。实测经协议端到端：2 KB 目录 → 2009 B 真 MP4 头 → 解回原文
- **历史页**：本机 JSON（`~/.local/state/yshjy/gui-history.json`，上限 200 条，可清空）；
  解压完成自动记一条（时间/文件/结果/大小/输出目录）
- **设置页**：配置、密码本、状态目录一律「看得见路径 + 一键打开」；
  逐项列出 config.toml 能改什么；**明确标注还没做的**（界面多语言、主题切换、界面改配置）
- **关于页**：版本/许可/硬边界/不联网声明 + 一键诊断包
- **引擎单例**：整个应用只跑一个 `yshjy --serve` 进程，各页面共用事件流
- 修 widget 测试：真实进程会在 fake_async 留 pending timer，测试用 `bootstrapEngine: false`

### 调整 — 窗口尺寸改成纯自适应（不为任何屏幕写死数字）

- 用 `gdk_monitor_get_workarea` 取**可用工作区**（自动扣掉顶栏与 Dock，HiDPI 下返回逻辑像素），
  窗口取 **45% 宽 × 66% 高**（约 30% 面积）—— 小到能看见旁边的文件管理器，又不至于放不下清单
- 上下限**也按比例算**；只有"内容放得下的最小尺寸"用绝对数（520×400），且不允许超过工作区 85%
- 实算举例（纯算术，不是拍脑袋）：1366×768 → 614×480（28% 面积）；
  1920×1080 → 864×686（29%）；2560×1440 → 1000×800（22%）；1024×600 → 520×400（34%，小屏兜底）
- 原因：拖拽是核心入口，窗口一铺满就把源窗口挡住了

### 新增 — 第 16 批：拖拽 / 一键报障 / 「接下来点哪个」

- **拖拽**：整个窗口都是放置区（`desktop_drop`）。拖进去就自动识别+预览；
  拖动时标题会变成「松手就行」，文字也改成「直接拖进这个窗口，或者点下面选择」
- **一键报障**：失败页多一个「生成诊断包（报障用）」按钮 → 引擎生成 zip，
  **路径默认脱敏**（`--no-paths` 等价），用户直接发出来就行，不用教他开终端。
  协议新增 `report` 命令（只加不改，`features` 里已声明）
- **解压完提示「接下来点哪个」**：从清单里认出 `setup.exe` / `启动*` / `开始游戏` /
  `*.bat` / `run.sh` / `*.appimage`，完成页直接说「接下来点这个：xxx」——
  解压出一堆文件后，新手最迷茫的就是这一步

### 新增 — 第 15 批：给「不会解压的人」用（GUI 密码框 / 应用菜单 / 右键打开）

- **GUI 加密包不再是死路**：以前遇到 `PASSWORD_REQUIRED` 只会显示失败；
  现在失败页直接给「解压码」输入框 + 「用这个解压码重试」
  （引擎本身还会**自动从旁边的说明文件里抓密码** —— 实测「加密包.zip + 说明.txt」不带密码直接解开）
- **应用菜单里能找到它**：`scripts/install-gui.sh`（不需要 sudo）把 GUI 装到
  `~/.local/opt/yshjy`，并写 `.desktop` + 256×256 图标 → 搜「杨三海解压」就能开
- **右键「用杨三海解压打开」**：`.desktop` 注册了 zip/7z/rar/gzip 等 MIME 类型，
  GUI 也支持命令行文件参数（带文件启动时自动打开它）
- **技术详情不再糊在新手脸上**：`Error { code: … }` 收进「复制错误详情」按钮（报障时一键复制）

### 新增 — 第 14 批：批量与分卷（目录里挑包 / 分卷别解三遍 / 改名分卷）

- **目录里一大堆文件、只有几个是包**：以前会把每个"看着像"的文件都跑一遍，
  50 个文件能刷出 50 行"这不是压缩包"，成功的两个被埋掉。现在失败**按原因合并**
  （实测：54 个文件 57 行 → 12 行），成功的那几个一眼可见
- **分卷只解一次**：目录里 `.001/.002/.003` 以前会被当成 3 个任务、**解三遍**
  （每块都能认出整组）。新增分卷归并（`.001` / `.z01` / `.part1.rar` 三种命名），
  实测 5 块的分卷目录 → 只解一次
- **给任意一块都能解开**：只把 `.002` 交给它，会自动找齐 1~N（实测通过）
- **改名分卷能认出来**：`视频1.mp4` `视频2.mp4`… 其实是同一份 7z 的切片（网盘躲审核的常见手法）。
  判定四条全满足才认：名字=前缀+数字、连续编号≥2块、**第一块有 7z 魔数**、各块大小一致
  （最后一块可小）。实测 5 块改名的 7z → 认出并解开
  宁可漏认也不误伤：`IMG1.jpg IMG2.jpg` 因为头不是 7z 魔数，绝不会被当分卷

### 新增 — 第 13 批：「你给我的是啥文件」（类型探测 + 人话回答）

- **新模块 `src/probe.rs`**：只读文件头 4 KB，认出 30 种类型 ——
  mp4/mov/mkv/webm/avi/flv/ts、png/jpeg/gif/webp/bmp/tiff、mp3/flac/ogg/wav、
  pdf、elf/exe、zip/7z/rar/gzip/xz/zstd/bzip2/lz4，外加「文本 vs 二进制」兜底。
  类型名是稳定标识（`mp4`/`elf`），**人话归外壳查文案**
- **失败不再只说"不是压缩包"**：`identify` 失败时把
  `looks_like` / `looks_kind` / `ext` / `ext_kind` / `ext_mismatch` / `size` 塞进错误参数。
  于是终端能说：「这个文件看起来是：MP4 视频（2 KB）」
- **「名字 vs 内容」对比（伪装包最典型的特征）**：
  `游戏.zip` 里其实是 MP4 → 「名字写的是 .zip，内容其实是 MP4 视频 —— 可能被改过名」；
  `可疑.mp4` 里其实是 ZIP → 「这很可能就是伪装包」；
  内容是可执行程序时给**安全警告**（来路不明的程序别运行）
- **空文件 / 文件夹 / 认不出** 各有专门说法（空文件=下载失败的壳；
  目录=里面没有能找到的压缩包；认不出=可能被加密或文件头坏了）
- **顺手清掉两处噪音**：识别失败不再重复发 `PartialData` 诊断；
  「走了慢路」这类性能诊断不再打扰用户（`--json` 里仍完整）
- 协议兼容扩展：`error.detail` 带出结构化参数（**只加字段，不升协议版本**）

### 新增 — 第 12 批：契约服务 `--serve`（界面的地基）

- **`docs/PROTOCOL.md`（协议 v1）**：这是前后端**唯一**的接头处，写死了四条兼容性承诺 ——
  只加不删 / 前端忽略不认识的字段与事件 / 引擎只给「码 + 参数」不给人看的句子 /
  破坏性改动必须 `protocol + 1`。守住这四条，"后端升级前端不用大改"才不是口号
- **`yshjy --serve`**：一行一个 JSON 的双向管道（NDJSON）。命令 `hello` / `list` / `extract` /
  `pack` / `cancel`；事件带稳定的 `kind`（`PREVIEW_READY` / `EXTRACT_PROGRESS` / `DIAGNOSTIC` …），
  结构复用 `src/event.rs`（唯一真源）
- **单写者模型**：读线程 → 命令队列 → 主线程唯一写 stdout → 任务线程
  （多线程同时写一行 JSON 会插成半行，前端解析会随机炸，这类 bug 从结构上杜绝）
- 一次只跑一个任务：忙时返回 `BUSY`（并发解同一个磁盘只会互相拖慢，界面也只有一个进度面板）；
  `cancel` 立即返回，真正停下由引擎看到取消标志
- **不开网络端口**：桌面 / 手机 / 脚本都走标准流，不引入监听端口与远程攻击面
- **`tests/protocol_smoke.py`**：7 组端到端冒烟（握手 / 列内容含中文名 / 解压并校验事件 /
  打包成真 PNG / 伪装图再识别 / 文件不存在 / 未知命令）。这个脚本本身就是前端的参考实现

### 新增 — 第 10 批：零碎收口（tar 列清单 / 救援事件 / 文案归位）

- **tar 家族现在能列清单了**（`--list` 不再只说"信息不完整"）：tar 是顺序格式、没有
  中央目录，只能把整条流的**头**扫一遍（不读数据体）。序号与解压完全一致（1-based、
  目录条目也占号），中文名宽松解码，mtime / 可执行标记照实给；中途取消或遇到坏条目就
  把 `complete` 标成 `false`，**绝不把半份清单说成全部**。
  剥外层压缩壳的逻辑抽成 `extract_stream::open_tar_stream()`，**解压与列清单共用一份**
  —— 否则两边支持范围会悄悄分叉（解压认得 `.tar.br`、预览却说"不支持"）
- **救援路径的安全提醒终于到得了用户眼前**：`extract_by_locals` 新增带 ctx 的
  `extract_by_locals_ctx`（旧入口保留为 quiet 包装，不破坏调用方）。「解出了可执行文件」
  从内部 `warnings` 改走 `Diagnostic::RiskyEntry` 事件 —— 之前塞在引擎内部列表里，
  外壳根本看不见，等于没提醒；顺带补了条目级事件（Ok / Encrypted / CrcError / DataError）
- **打包与救援的输出不再写死中文**：删掉 `PackReport::summary()`（引擎拼中文句子违反分层），
  文案搬回外壳查语言包；`--list` 兜底与 `--rescue` 两处重复的清单打印合并成 `打印救援清单()`，
  体量统一走 `人类可读()`（以前是 "…字节" + 一律 `%.2f GB`，3 字节也显示 0.00 GB）。
  7 个语言包同步补了 25 条键（含打包/救援的整块输出，英文/日文实测正常）

### 新增 — 安全护栏（回答"自动输密码会不会给别人可乘之机"）

- **试密码候选数加了硬上限（64）**：原来 `max_candidates` 只有下限没有上限，
  理论上一句配置就能把它变成爆破器。现在夹死上限 —— **配置改不出"爆破模式"**（有测试守着）
- **删掉 `try_common`（"尝试内置常见解压码"）配置项**：它从来没接线过（配置项在、没人读），
  而真要实现它就得内置一张常见密码表 —— 那等于自带一个小字典，越过了本项目的安全边界。
  删掉比留一个骗人的开关好：用户以为有、实际没有，比明说"我们不干这个"更糟。
  老配置里写着它也不报错（宽容解析照旧）
- **"只试不存"写成代码里的硬边界**：从说明文件抓到的密码用完就丢，**绝不回填密码本**；
  并加了一条"读密码本不许改动文件"的测试当保险丝（谁以后加"自动收集密码"，这条会红）
- **试密码个数改成真由配置决定**：原来 ZIP 分支硬编码只试前 3 个，`max_candidates` 写 12 也没用
- **README 新增「本工具不做的事」**：不做字典/爆破/内置密码表、不联网、不自动收集密码 ——
  把边界写成公开承诺。`DESIGN.md` / `docs/UI-SPEC.md` / `docs/BACKEND-SPEC.md` 里残留的
  "常见解压码库"设计同步标注为**已否决**，不留"文档说有、代码没有"

### 修正 — 密码抓取的误报（真 bug）

- **`pass` 标签不做词边界检查**：`test result: ok. 251 passed; 1 failed;` 这行会被抓出
  候选解压码 `ed`（`pass` 之后的残留）—— 旁边放着一份日志的用户会白试一堆垃圾密码，
  界面还会说"从说明文件里抓到了密码"。现在 ASCII 标签必须卡在词边界上
  （`passed` / `bypass` / `passwordless` 都不再误命中），正常写法（`password: x`）照旧能抓

### 新增 — 统一解压器接口（「加格式只改一处」已落地）

- 新模块 `src/unpack.rs`：`trait InArchive`（一个格式一个实现）+ 静态**有序注册表** `HANDLERS`
  + `Caps` 能力表（界面可直接读来画"支持加密 / 分卷"）+ 统一参数 `UnpackOptions`
  （原来四份各写一遍的 Options 合并成一份）+ `Source`（**一条流**：偏移与多卷由编排层消化）
- 参考依据全部来自 `refs/tech` 台账：7-Zip 的 `IInArchive`/`IOutArchive` 读写分离、
  能力靠探测而"主接口永不膨胀"；XADMaster 的"handle 是唯一 I/O 抽象""注册表顺序即优先级"；
  PeaZip 的"每后端一个构造器 → 每格式一个 impl"；zip2 的反面教材（接口不暴露就没法复用）
- 加格式现在只要三步：写 `impl InArchive` → `HANDLERS` 加一行 → 补一条能力表测试
- **已接进编排层**：`job.rs` 的 `extract_target` 从「五段几乎一样的代码」变成
  「准备源 → 查注册表 → 交给解压器」。7z / RAR 也改走**一条流**
  （新增 `extract_7z_stream_ctx` / `extract_rar_stream_ctx`，RAR 那边先 seek 取长度
  再读进内存，仍受 256 MiB 上限保护），`Source` 的 `path` 字段删掉（接线时发现没人读 ——
  RAR 也能从流读了，接口保持最小）
- **真实包复验**（改了核心解压路径，必须比哈希）：5.9 GB 包 → `7dda9883…3b59`；
  8.7 GB 包 → 8,589,934,592 字节 / `e6269244…62dd` —— 两个都与基准**逐字节一致**
- **预览（`list()`）也已归位**：`trait InArchive` 新增 `list()`（默认实现**明确报
  「还没实现」**，绝不返回空清单 —— 空清单会被当成"包里什么都没有"）；`preview.rs`
  的三个预览函数改成按「一条流」工作，`preview_target` 同样改成「准备源 → 查注册表」；
  5 个格式全部实现（tar 是顺序格式，如实答「信息不完整」，不假装列出来了）。
  **至此加格式只需改 `src/unpack.rs` 一处** —— 两个编排函数（`job.rs` / `preview.rs`）
  都不用碰
- 顺带逮到的真问题：`Source.len` 为 `None` 的契约是「用到流尾」，但**不能当成 0** ——
  长度 0 的窗口一个字节都读不出来（症状："明明是好包却被判成不是压缩包"）

### 性能 — 修掉一个百倍性能坑（`zip` 库别自己去检测偏移）

- **症状**：真实 8.7 GB 伪装包 `--list` 要 **109 秒**；解压也比应有的慢
- **诊断**：`--json` 事件流显示 **Locate 阶段只占 40 毫秒** —— 慢的不是识别，
  而是 `zip::ZipArchive::new` 这一步（我最初误判成"识别阶段在扫全文件"，是数据纠正了我）
- **根因**：`zip` 库默认 `ArchiveOffset::Detect` —— 它会自己"搜索中央目录的真实位置"
  来反推归档偏移。可我们的偏移**早就自己算好了**（`io::Window` 把 base 归零 → 就是 0），
  这一遍纯属浪费，代价却是把整个窗口扫一遍
- **修法**：新增 `layout::zip::打开已知偏移()`（`Config { archive_offset: Known(0) }`），
  三个打开点全部改用它：preview（列清单）/ extract（解压）/ verify_password_at（试密码）
- **效果**（同一个真实包，产物 SHA 逐字节一致）：
  · `--list` 8.7 GB：**109 秒 → 0.09 秒**（约 1200 倍）
  · `--list` 5.9 GB：22 秒 → 0.39 秒
  · 解压 5.9 GB：**129 秒 → 55.2 秒**（2.3 倍）
- **纪律**：打开 ZIP 一律走 `打开已知偏移()`，别直接 `ZipArchive::new` ——
  「我们定位、库解码」这条分工漏一个调用点就是百倍性能差

### 修正 — 既有代码的两处松绑

- `extract_tar_ctx` 的 `R: Read + 'static` 放宽成 `R: Read + 'a`：内部只是把源装进
  `Box<dyn Read>`，`'static` 让**借来的流**（`&mut File` / `&mut Window`）传不进来，
  而统一接口恰恰只提供借来的流。对原有调用点完全兼容（约束更松，调用更自由）
- `ExtractReport` 新增 `repaired` 字段：修过索引从"塞在日志里"变成**结构化结果**，
  批量汇总据此把任务标成「索引有错但救回来了」；`is_clean()` 也不再把它算作干净

### 新增 — 诊断包 `--report`（报障用）

- `yshjy --report [包]` 生成一个**能直接贴出去**的 zip，内含：
  `README.txt`（写清"包含什么/不包含什么"）+ `meta.json`（版本/系统/硬件摘要/
  磁盘余量/脱敏命令行）+ `findings.json`（识别出的格式、基准偏移、条目数、加密数）
  + `config.snapshot.toml`（当时生效的配置）
- `--no-paths`：路径脱敏成 `<path-1>` 这类占位符（要贴到公开论坛就用它）
- **隐私红线（这次守住了什么）**：
  · 命令行里的 `-p xxx` / `--password=xxx` 值一律抹成 `<已抹掉>` ——
    设计文档 §4.2 原本要求记「完整命令行」，那会把**明文密码**写进包里；
    这条已修正（理由写在 `src/report.rs` 模块头）
  · 代理环境变量**只记有没有设、不记值**（值可能长成 `http://user:pass@host`）
  · 文件内容一个字节都不读；密码本只记行数不记内容；**文件名清单不含**（只给数量）
  · `README.txt` 逐条写清包含/不包含 —— 用户要敢把它贴出去
- 7 个语言包都补了文案与帮助行（`yshjy --help` 里能看到这条命令）
- **实测（生成一份带假密码的诊断包、逐字段检查）还逮到三个真问题，已修**：
  · `--report` 把 `-p` 的**值**当成了目标文件（`findings.target` 变成了密码）——
    新增 `找位置参数()`：选项本身与「带值选项的值」都要跳过
  · 磁盘余量恒为 `null` —— 借用的 `check_space(dir, 0)` 对 `need == 0` 直接返回
    `Unknown`（它是给"空间预检"用的）；改成直接问文件系统
  · zip 条目时间戳是 1980（`SimpleFileOptions` 没设时间，与 v0.8 打包侧同款坑）
  · 顺带：配置快照里 `language = None`（Rust 的 Option 直接 Debug 输出，
    不是合法 TOML 值）→ 改成 `"(跟随系统)"`
- **隐私实测结果**：`-p` 的值 → `<已抹掉>`、假密码在包内 **0 次出现**；
  `--no-paths` 下 argv / target / 配置路径 → `<path-1>` … `<path-3>`、
  真实路径 **0 次出现**

### 新增 — fuzz 模糊测试（三个目标）

- 新增 `fuzz/`（cargo-fuzz + nightly），三个"任意字节输入"的入口：
  · `list_zip` —— 统一接口的列清单（纯读不写盘，一次覆盖五个 handler 的头部解析）
  · `locate_zip` —— ZIP 定位（全是偏移算术：相对/绝对换算、小端读整数、`checked_sub`，
    越界与下溢的高危区，也正是我们"别人做不到"的核心能力）
  · `detect_stream` —— 单流格式识别 + 按文件名的兜底判断（外部数据进门第一道关）
- 判据只有一条：**不 panic / 不越界 / 不无限循环**（返回 `Err` 是正常结果，
  畸形输入本来就该被拒）
- **首次体检（每目标 60 秒）**：合计约 **2787 万次**畸形输入，**零 crash**
- **如实说明**：60 秒只能算"初步体检通过"，**不等于**证明没有内存安全问题
  （`locate_zip` 覆盖率只有 138 —— 随机字节很难凑出合法的 ZIP64 记录链）。
  怎么跑久一点、corpus / artifacts 放哪，都写在 `fuzz/README.md`
- 顺带修：`.gitignore` 以前把整个 `fuzz/` 忽略了 —— 那样连目标**源码**都进不了库；
  现在只忽略构建与运行产物

### 测试

- **259 单元测试全过**（v0.8 的 245 + 本轮新增 14）；`clippy --all-targets -- -D warnings` 零警告
- 冒烟脚本 **25 个场景**全过
- fuzz 首次体检：**2787 万次**畸形输入、**零 crash**（三个目标各 60 秒）

---

## [0.8.0] - 2026-10-17 · 多语言 + 安全护栏（"让大部分人能用"第 1 阶段）

**主题：能看懂、别白干活、文件别乱。**

### 新增 — 多语言 ⭐

- **7 种语言**：简体中文 / 繁体中文 / 英语 / 日语 / 韩语 / 俄语 / 西班牙语
- `yshjy --lang en` 手动切；不给就**自动跟系统语言**（`YSHTY_LANG` → `LC_ALL` → `LANG`）
- **加一种语言不用改一行代码**：往 `locales/` 丢一个 `.toml` 就行（`build.rs` 自动扫进二进制）
- **支持局部覆盖**：`~/.config/yshjy/locales/xx-XX.toml` 里只写想改的几句，
  其余自动用内置的 —— 想换个说法不用抄整个文件
- 插值支持 `{name}` 与 `{name:>4}` 对齐；`{{ }}` 是字面花括号
- 查找链：外部包 → 内置精确匹配 → 默认表落位（`zh-Hant`→繁体、`en-GB`→英文）→ 兜底中文

### 新增 — 配置 + 密码本

- 配置在 `~/.config/yshjy/config.toml`（Linux XDG；macOS / Windows 按平台惯例），
  可用 `YSHJY_CONFIG_DIR` 整体换掉
- **读的时候宽容**：文件不在→默认值；某个字段写错→**只有那个字段**退回默认；
  未知字段 / 自造的段一律忽略 —— 一个笔误让整份配置作废是最恼人的失败方式
- `passwords.txt` 每行一个解压码（`#` 注释）。**兼容隐写者的 `PW.txt`**（Tab 取第一列），
  但**不做 ASCII 过滤** —— 那正是隐写者的 bug（中文密码被吞掉）
- 密码本自动接进「试密码」：候选顺序 **用户记的 > 旁边的说明.txt**，
  仍然只读加密条目开头几个字节验证，不整包试
- `yshjy --show-config` 看配置；`yshjy --import-passwords <文件>` 导入

### 新增 — 磁盘空间预检

- 解压前看目标盘够不够，**不够就直接拒绝开始**（而不是解到一半炸掉、前面的时间全白费）
- 三个刻意的设计：① 只有预估得出总大小时才拦（拿不准就放行 —— 假拒绝比漏拒更恼人，
  而且用户没法绕过）② 留 5% 余量 ③ `--force` 可跳过

### 新增 — 保留原始时间戳 / 权限

- 新增 `restore` 模块：格式无关的「属性还原」中间层（各解压器只负责翻译元数据）
- ZIP + TAR 已接；**剥掉 setuid/setgid/sticky** —— 从网上下载的包不该凭归档里
  写的权限位就给文件挂 setuid（提权隐患）
- 尽力而为：设不上只提示一句，**绝不把一次成功解压变成失败**

### 修正 — 七个真 bug（都是新功能顺带暴露的老问题）

1. **`yshjy ys` 打包从不写时间戳** → 解出来全是 `1980-01-01 08:00`（ZIP 格式起点）。
   根因：所有条目共用一份选项，而一份选项只能装一个时间。
2. **`--lang en 包.mp4` 把 `en` 当成要解压的文件** —— "带值选项"清单漏登记了 `--lang`。
   已抽成常量，以后加选项不会再漏。
3. **`zh` / `zh-Hans` 随机落到简体或繁体** —— 遍历 `HashMap` 的迭代顺序是随机的，
   同一条命令今天明天不一样。已改成常量默认表 + 20 次重复断言防复发。
4. **`zh-Hant` / `zh-HK` 被当成简体** —— 已按脚本标签正确导向繁体。
5. **`yshjy --list 包.mp4 | head -3` 会 panic** —— Rust 默认忽略 SIGPIPE，管道被关闭
   变成写失败、`println!` 直接崩，用户看到一段吓人的回溯（其实一切正常）。
   已恢复系统默认行为（Unix SIG_DFL），现在安静退出 141，像所有 Unix 工具一样。
6. **「解出了可执行文件」的提醒传不到用户眼前** —— 警告塞在引擎内部列表里，外壳看不见，
   所以配置开关"看起来无效"。已改成走**诊断事件**（外壳本来就会渲染它）。
7. **配置项"只读不用"** —— `warn_executables` 传进 Limits 了但引擎里没人读它。
   已加 `safety::should_warn_executable` 真接线，实测开/关都生效。

### 内部

- 引擎里**面向用户的文案**继续归位到语言包（14 个错误码 + 结果行模板）
- `do_extract` 参数涨到 9 个被 clippy 拦下 → 重构为参数结构体
  （一堆 `bool` 按顺序传错是**不会报错**的，这是真隐患）

### 验证

- 测试 **245 个**全过；冒烟 25/25；`clippy -D warnings` 零警告
- 真机验证：源文件 `2020-10-17 12:00` → 打包 → 解压 → 仍是 `2020-10-17 12:00` ✓
- 七种语言的帮助 / 结果行 / 错误文案实测切换正常

### 已知边界（有意为之，不是漏做）

- **7z 的时间戳还原不了**：`sevenz-rust2` 把 `NtTime` 的原始值定成 `pub(crate)`，
  外部取不出来。不用"格式化再解析"那种脏办法，等库暴露访问器。
- **RAR 的时间戳还原不了**：走的是高层解压 API，拿不到 `FileHeader`。
  这类包里 RAR 占比低，取舍掉。
- **救援模式的可执行提醒到不了外壳**：那条路径的函数签名没有 `ctx`（事件通道）。
  救援是"包已经坏了"的兜底路径，优先级低；代码里写了注释说明，没假装它生效。

## [0.7.0] - 2026-10-17 · 常用版补全 + 双版本骨架

**主题：把「常用」这个词做实 —— 补上 Brotli、LZ4、ZIP 分卷，并分出常用版 / 全能版两个档位。**

### 新增 — 三个格式（常用版就此名副其实）

| 格式 | 怎么认出来的 | 说明 |
|---|---|---|
| **Brotli**（`.br`） | **只能按扩展名** | 这格式**没有文件头魔数**（设计如此），字节层面根本认不出来 |
| **LZ4**（`.lz4`） | 魔数 `04 22 4D 18` | 帧格式；`.tar.lz4` 双层也支持 |
| **ZIP 分卷**（`.z01` + `.zip`） | 扩展名 + 收尾块 | 拼成逻辑流后按普通 ZIP 解；从 `.z01` 或 `.zip` 进都认 |

顺带把 `.tar.br` / `.tar.lz4` 加进 tar 家族识别。

### 新增 — 双版本（常用版 / 全能版）

- `cargo build --release` → **常用版**（默认；日常下载会遇到的格式全都在）
- `cargo build --release --features full` → **全能版**（冷门格式挂这里）
- `yshjy --version` 显示档位；`bash scripts/release.sh` 一次出两个包
- **诚实说明**：全能版当前覆盖与常用版一致 —— ISO/UDF、CAB、ARJ 这些还没做，
  骨架先搭好，格式一个个补进来
- 为什么分档：常用版**依赖少 → 被投毒的风险面小**（安全工具的实打实收益）；
  冷门格式解析器代码复杂，不该拖累主路径

### 修正

- `--list` 对单流格式不再笼统显示 `stream`，改为具体格式名（brotli / lz4 / gzip…）
- **ZIP 分卷必须在 7z 分卷之前判**：`.z01` 会被「裸分卷」逻辑抢走 —— 已加测试盯着

### 新增测试（+3）

- `lz4_单流能靠魔数认出并解开` —— 造样本 → 识别 → 解压逐字节对照
- `brotli_没有魔数只能按扩展名认并解开` —— **断言「靠魔数认不出来」**，把格式特性钉死
- `zip_分卷能认出并拼接解开` —— 带**对照组**（不切分时先要能解），
  避免把「造数据的问题」误判成「分卷逻辑坏了」
- 新增 `examples/造样本.rs`：造真实 `.br` / `.lz4` 文件（系统上不一定装这俩命令）

### 验证

- 单元测试 **217 个**全过；常用版 / 全能版**两档** clippy `-D warnings` 零警告
- 端到端实测：`.br` 与 `.lz4` 走完整 CLI 解出来**逐字节一致**；
  ZIP 分卷（切在数据中间、真跨卷）列表与解压均正常

### 踩坑记（值得记住）

- **测试数据不能太「好压」**：`'x' * 5000` 这种压缩比正好 100 倍 → 撞上压缩炸弹防护被拒。
  那是防护在**正常工作**，正确做法是换数据，**而不是为迁就测试去放宽安全限值**。

## [0.6.0] - 2026-10-17 · 欠债清理（预览接线 / 智能解压 / Ctrl-C）

**主题：把 v0.5 留下的三个口子补上 —— 预览真能用、解压目录变聪明、Ctrl-C 不再留半成品。**

### 新增 — 多格式预览 ⭐

- 新增 `preview` 模块：**不解压**就把包里有什么列出来，ZIP / 7z / RAR / 单流全部支持
  - **ZIP**：算好基准偏移喂给库读中央目录（8.7 GB 的包也是微秒级）
  - **7z**：`ArchiveReader` 解析头部就拿到条目（不碰数据）
  - **RAR**：`members()` 只解析头；该库要求数据在内存里，所以超大 RAR **如实降级**
    （标 `complete = false`，不硬撑）
  - **单流**：只有一个产物，名字由文件名推
- **`PreviewReady` 事件接线**（v0.5 里只定义了类型、没接）：这是界面 S3
  「列出内容 → 勾选 → 再解压」的数据源
- CLI `--list` 改走新路径（v0.4 只有 ZIP 能列），并支持 `--list --json`

### 新增 — 智能解压三规则 ⭐

（UI-SPEC §9.2，源自 Bandizip 的智能解压）

1. 包里只有**一个顶层目录** → 直接解到目标目录，不再多套一层同名文件夹
2. 只有**一个文件** → 同样直接放目标目录
3. **一堆散文件** → 自动建同名子目录（免得几十个文件散在下载目录里）

CLI 可用 `--no-smart-dir` 关掉（退回 v0.5 行为）。

### 新增 — Ctrl-C 接管

- 信号转成引擎的**取消令牌**：已解出的文件保留、写了一半的删掉、退出码 `130`；
  不再是"被信号打死、留下一堆半成品"
- 连按两次强制退出（第一次温柔收尾，第二次立刻走）

### 修正 — 条目序号统一为 1-based

- 界面上显示的号 = `--select` 用的号 = 事件里的号（避免"看到 1、勾到 2"这类错位）
- 涉及 ZIP / 7z / RAR / TAR / 救援五条路径，每处都写了注释说明

### 验证

- 单元测试 **214 个**全过（新增 7 个：预览 6 + 智能规则 1）
- clippy `-D warnings` 零警告
- 冒烟场景 **25/25 全过**（`--list` 路径本次有改动，已重跑确认）
- 端到端实测：`--list` 带序号清单 / `--list --json` 事件流（含 `PreviewReady`）/
  智能目录 / `--select "3"` 精确只解第 3 个条目 ✓

### 已知边界

- RAR 预览**没有条目大小**（`rars` 公开的成员元数据里没有这个字段），如实标 `complete = false`
- TAR 家族暂不预览（顺序格式要扫完整流才能列全）

## [0.5.0] - 2026-10-17 · 引擎地基（事件 / 进度 / 挑选 / 批量）

**主题：把「能解」变成「看得见、挑得动、能批量」—— 这是图形界面的地基。**

### 新增 — 事件流 ⭐

- **引擎事件模型**：任务/阶段/扫描/预览/进度/条目结果/诊断七类事件，
  全部 `serde` 可序列化（将来 GUI 与手机端通过 FFI 直接吃）
- **有界队列 + 进度合并**：同任务的连续进度只保留最后一条；队列满时先丢最旧的进度，
  **结果事件永不丢**（解 351 个文件也不会把外壳淹掉）
- CLI `--json` 把事件流按行吐出，脚本/前端可直接消费

### 新增 — 进度与取消 ⭐

- **无锁进度快照**（全 `AtomicU64`）：8 GB 解压期间界面随时能读到一致数字
- **多任务聚合**：批量时进度只增不退，任务切换不跳变
- **嵌套进度折算**（RAII）：ZIP 里套 7z 时内层进度正确折算到外层，结束自动补齐
- **剩余时间估算**：指数滑动平均；样本不足时**不给估算**（宁可不显示也不乱跳）
- **取消**：检查下沉到数据块级（64 KiB 粒度），8 GB 单文件也能及时停；
  已解出的文件**保留**，写了一半的删掉；取消是独立语义（退出码 130），不显示成"失败"
- CLI 进度条只在终端上画（管道/日志里不吐控制字符）

### 新增 — 预览挑选 ⭐

- `Selection`：按序号 / 通配表达式 / "只要必需的"三种挑法
  - 通配自实现：`*` 不跨目录、`**` 跨目录；模式不含 `/` 时按文件名匹配任意层级
  - `required` 自动跳过 `.DS_Store` / `__MACOSX/` 这类平台垃圾
- CLI `--select "*.exe"` / `--select "游戏数据/**"` / `--select 1,3,5` / `--select required`

### 新增 — 批量处理 ⭐

- 给一个**目录**就自动排队：递归找可疑文件、去重、按体积倒序（大的先跑）
- 每个任务独立成败，**一个坏了不影响其余**（修掉隐写者"批量成功率恒 100%"的假成功）
- `--jobs N` 并行（默认 1，顺序跑最省磁盘；分片并行结果仍按任务号排序）
- 批量汇总 + 退出码统一：全成功 0 / 有失败且有成功 5 / 全失败取首个错误码 / 取消 130
- 多任务 + `-o` 时每个包解到自己的子目录（免得互相改名打架）

### 新增 — 三层结果模型

- 以前只有"一次操作一个错误码"，现在补齐：
  - **层1 调用级** `ErrorCode`：这次调用成了吗
  - **层2 归档级** `ArchiveFlags`：11 位诊断位图（可同时表达"尾部截断 + CRC 错"）
  - **层3 条目级** `ItemResult`：第 47 个文件对不对
- `Outcome`：成功 / 部分成功 / 取消 / 失败四态，**退出码只在引擎里算一次**
  （修掉 7-Zip 两个壳各算一份、行为不一致的老毛病）

### 重构 — 编排搬进引擎

- 新增 `job`（单任务编排）：识别 → 定位 → 分派 → 结果映射，全部在引擎里
  - 此前这套逻辑散在 `main.rs` 的 `do_extract_*` 分支里 —— **GUI 想解个包得把它抄一遍**
  - 现在 CLI 与 GUI 共用同一份：外壳只负责"把事件渲染成人话"和"把结论变成退出码"
- 所有解压路径（ZIP / 7z / RAR / 单流 / TAR）统一支持事件、进度、取消、挑选
- CLI 从 1078 行瘦到 744 行
- 退出码常量全部改为从引擎取，外壳不许自己定数

### 修复

- 🔴 **中文名会 panic**（从 v0.4 继承的真 bug）：分卷名解析用字节索引切字符串，
  `示例视频` 这类以中文结尾的文件名直接崩 → 改为按字符边界查找
- 取消被误判成"不是压缩包"（7z / RAR / 单流三条路径）
- **试密码不再污染输出目录**：改在临时目录静默试，成功才采用
- 进度总量只统计被挑中的条目（否则进度条永远到不了 100%）
- 进度节流首条不生效（小任务一条进度都没有，界面像卡住）
- 目录条目也计入进度（否则"完成 4/5"永远差一个）
- 单流解压走快捷路径时不产生条目事件

### 验证

- 单元测试 **130 → 206 个**，全过
- 冒烟场景 **25/25 通过**（含打包→解压闭环 `diff -r` 一致）
- clippy `-D warnings` 零警告；全程 `#![forbid(unsafe_code)]`
- 新增依赖：`serde` / `serde_json`（跨语言事件流与 `--json` 用）

### 已知边界（诚实说明）

- 「智能解压三规则」（单条目直接解到当前目录 / 重名加 `(2)`）需要先预览才知道条目数，
  留 v0.6 与图形界面一起做
- **预览事件 `PreviewReady` 已在事件模型里定义，但尚未接线**：CLI 的 `--list` 仍走
  v0.4 的 ZIP 路径（`layout::zip::list_entries`），其它格式的预览与 `--list --json` 留 v0.6
  （界面 S3「列出内容 → 勾选」靠它）
- CLI 暂不接管 Ctrl-C（图形界面用引擎的取消令牌，v0.6 接上）
- `ExtractReport` 里的跳过原因是中文文案（引擎产出用户文案的老账），v0.6 清理成错误码

## [0.1.0] - 2026-10-16 · 第一个测试版

**主题：把「伪装成视频/图片的压缩包」解出来，并能反向打包。**

### 新增 — 定位（读取层）

- 尾部窗口定位 + **偏移反推**：从 EOCD 记录的中央目录位置反算出压缩包在文件中的真实起点
- **全文件签名扫描**：分块读取 + 重叠窗口，找到藏在文件任意位置的压缩包
  - 首字节位图 + 两字节索引，实测 8.7 GB 扫描 160 秒（优化前 387 秒）
  - **假签名识别**：短签名附加合理性校验，误报从 1064 处降到 83 处
- **硬件探测 + 资源档位**：识别机械盘 / 固态盘、核心数、可用内存，
  据此推荐扫描深度，并预估全扫耗时（实测误差 < 6%）
- 可中断读取、分段虚接（对付"包被切成两段"）

### 新增 — 判断（识别层）

- ZIP 结构解析：本地文件头 / 中央目录 / ZIP64 / EOCD 一致性校验
- **文件名四级回退解码**：UTF-8 标志位 → 严格 UTF-8 → **GB18030** → 兜底
  （实测：GBK 编码的中文文件名正确显示，优于 Python `zipfile` 的 CP437 行为）
- 加密标记识别（ZipCrypto / AES）

### 新增 — 解压层 ⭐

- **ZIP 解压**（store / deflate），流式处理不占内存
- **解密**：ZipCrypto + AES-256，支持中文密码
- 安全落盘：路径穿越防护、解压炸弹限额（压缩比 + 总量双闸）、
  危险可执行文件提醒、失败自动清理半成品
- 同名策略（跳过 / 覆盖 / 自动改名）、选择性提取（支持目录前缀）
- **结果分级**：不再是非黑即白的成功/失败，如实报告"解出几个、跳过几个、为什么"

### 新增 — 压缩层 ⭐

- **伪装打包**：把文件藏进封面（MP4 `free` 原子 / 尾部追加两种布局）
  - 64–256 字节随机填充（更隐蔽 + 每次产物哈希不同）
  - 支持 `free` 原子的 64 位 large-size 形式（> 4 GiB 也不会写坏）
- 密码加密打包、目录递归（保留顶层目录名）
- **内置封面素材**（编译进二进制，零外部依赖）
- **产物自检**：写完回读验证能定位、条目数一致 —— 杜绝"造出一个打不开的包"

### 新增 — 修补层 ⭐

- **索引修复**：中央目录偏移被改坏时，在**内存里**把错的字节改对后重试
  - 支持三种成因：偏移被改写、32 位回绕、中央目录被平移
  - 磁盘上的源文件一个字节都不动
- **救援模式**：不看中央目录，按每个文件的头顺序重建清单
  - 借鉴 7-Zip 的 LOCALS-CD-MODE，并增强为"可指定起点"（对付伪装包）
  - 数据描述符双重验证（防止把随机数据当成文件）

### 新增 — 命令行

- `yshjy <文件>` 默认解压；`yshjy ys` 打包；`--list` 只看内容；`--rescue` 救援
- **退出码分级**（0 成功 / 1 一般错误 / 2 用法 / 3 不是压缩包 / 4 密码 / 5 部分成功）
- 安静模式 `-q`（只输出路径，适合脚本）、密码永远打码
- 中文人话提示 + 下一步建议

### 实测验证

| 场景 | 结果 |
|---|---|
| 8.7 GB 真实伪装包（内层 8 GiB 分卷，中文解压码） | 解出 8,589,934,592 字节（误差 0），**SHA-256 与手工脚本一致** |
| 29.4 MB 仿真包（AES-256 + 中文密码 + 中文名 + 嵌套目录） | 定位 510 µs / 解压 146 ms，字节一致 |
| 打包 → 解压闭环 | `diff -r` 完全一致 |
| 索引被改坏的包 | 自动修复后正常解出 |
| 冒烟测试 | **25/25 通过** |
| 单元测试 | **98 个全过**，`clippy -D warnings` 零警告 |

### 技术基础

- 拆解 6 个开源参考项目（共 75 万行源码）→ **681 条技术点台账**
  （每条带参考源码文件行号 + 是否采用 + 实测结论），见 `refs/tech/`
- 采用项全部在代码注释里标注出处，保持可追溯
- 明确记录**不采用**的 101 条（附原因），防止以后重复踩坑

### 已知限制（诚实说明）

- 解压仅支持 ZIP 家族；7z / RAR / TAR / GZ / XZ / ZSTD / BZIP2 目前**能识别但还不能解**
- 分卷合并、批量处理、图形界面尚未实现
- Windows / Android 未验证（代码已避免平台特有 API，路径全用 `PathBuf`）
- 加密打包使用 ZipCrypto（AES 写侧 API 在依赖库中尚未稳定）

---

## [0.4.0] - 2026-10-17 · 格式补全（单流 + TAR）

### 新增

- **单流压缩格式解压**（`extract_stream`）—— 补齐格式矩阵最后一块：
  - **GZIP**（`flate2`）
  - **XZ / LZMA**（`lzma-rs`，纯 Rust）
  - **ZSTD**（`ruzstd`，纯 Rust）
  - **BZIP2**（`bzip2-rs`，纯 Rust）
  - 全部 MIT / Apache-2.0，**零外部程序依赖**
- **TAR 解压**（含 `.tar.gz` / `.tar.xz` / `.tar.zst` / `.tar.bz2` / `.tgz` / `.txz`）
  - 自动剥两层：先解压缩流，再展开成目录树
  - **按文件名识别 tar 家族** —— `.tar.gz` 的头部只能看到 gzip 签名，
    光看头部判断不出里面是 tar（这是所有归档工具的通行做法）
  - 符号链接/硬链接条目**明确跳过**并说明原因（防链接逃逸）

### 修复

- **单流格式解压出 0 字节** —— 又是压缩比防护误报（分母传了 1，
  任何解压都算"压缩比 1750 倍"）。改用**实际压缩数据大小**做分母。
  （这是第二次踩同一个坑：7z 那次也是。已统一到"从文件元数据取真实大小"。）

### 实测（9 种格式全通）

| 格式 | 结果 |
|---|---|
| `.gz` / `.xz` / `.zst` / `.bz2`（单流） | 内容与原文逐字节一致 |
| `.tar` / `.tar.gz` / `.tar.xz` / `.tar.zst` / `.tar.bz2` | 目录树与内容一致 |

---

## [0.3.0] - 2026-10-17 · 救援式提取 + 自动找解压码

### 新增

- **救援式提取**（`rescue::extract_by_locals`）—— 中央目录**彻底坏掉**也能把数据救出来
  - 别人（7z / unzip / unar）在中央目录坏掉时基本没辙，我们按每个成员的文件头**逐个抢救**
  - 支持 store / deflate 两种方法；用 local header 自带的 CRC 校验，对不上就如实报告
  - 实测：一个 3.4 GB、中央目录被改坏的包装包 → 正确重建并抽出 `示例包.7z.001`（8.59 GB）
- **自动找解压码**（`password_hint`）—— 从旁边的「说明.txt」里把密码抓出来
  - 认 `解压码：xxx` / `密码: xxx` / `password = xxx` 等写法（全角半角、空格 Tab 都行）
  - 显示**原始行**让用户核验用了哪一条；找不到就照常让用户输入
  - 只读文本、不改文件、不做字典爆破

### 修复

- `password_hint` 用**字节数**判长度，导致中文标点（如 `。`）被当成合法密码 → 改用字符数
- `password_hint` 在**相对路径**下失效（`Path::new("a.mp4").parent()` 返回空路径，
  `read_dir("")` 直接失败）→ 兜成当前目录，并补了专门的回归测试

### 实测

| 场景 | 结果 |
|---|---|
| 相对路径 + 加密包 + 说明.txt | 自动抓到「demo-password」并解出 6 个文件 |
| 中央目录坏掉的包（救援式提取） | 按文件头逐个抽取，CRC 校验后落盘 |
| RAR 13 个样本变体 | 13/13，与 unrar 逐字节一致 |

---

## [0.2.0] - 2026-10-17 · 7z / RAR 解压

### 新增

- **7z 解压**（`sevenz-rust2`）：支持 LZMA/LZMA2/PPMd/BZip2，含 AES-256 加密与中文密码
- **分卷虚拟拼接**：`x.7z.001` / `x.002` … 按裸字节语义首尾相接成一个逻辑流
  - CLI 自动发现分卷（给第一卷即可，自动找齐后续卷）
  - 实测 5 卷 20 MB → 解出 6 个文件，与原始目录 `diff -r` 完全一致
- **救援模式增强**：解析 ZIP64 extra（0x0001），正确读出超过 4 GiB 的真实大小
  - 实测：一个 3.4 GB、中央目录被改坏的包装包 → 正确重建出 `示例包.7z.001`（8.59 GB）
- **`--list` 支持坏索引兜底**：中央目录坏了也能列出内容清单
- **RAR 解压**（`rars`，纯 Rust / Apache-2.0）：RAR1.3 → RAR7 全系列
  - 加密包主动做**密码校验**（底层库解密失败时**静默不写数据**，会产出空文件 —— 必须自己验）
  - 全量内存读，设 256 MiB 上限，超限明确拒绝而不是吃光内存
  - 实测：库自带 13 个样本变体（RAR5/RAR7/分卷/恢复记录/注释）**13/13 全通**
  - 与系统 `unrar` 交叉验证：解出的文件**逐字节一致**

### 实测

| 场景 | 结果 |
|---|---|
| 7z 分卷（5 卷 20 MB，含中文名） | 自动发现分卷 → 解出 6 文件，`diff -r` 一致 |
| 7z 加密包（AES-256） | 给对密码正常解出 |
| 3.4 GB 中央目录被改坏的包装包 | 救援模式重建清单，读出 `示例包.7z.001`（8.59 GB，ZIP64 正确解析） |
| RAR 样本 13 个变体 | 13/13 通过，与 unrar 结果一致 |

### 修复

- 7z 路径的解压炸弹防护误报（7z 没有 ZIP 那样的中央目录先验，
  原先用固定基准 1 会导致正常文件被判为压缩比异常）→ 改用**压缩包实际体积**做分母

### 计划

- v0.2 — 7z / RAR 解压、分卷合并、批量、预览挑选、自动抓解压码
- v0.3 — 桌面图形界面
- v0.4 — Windows
- v0.5 — 手机（Android）
