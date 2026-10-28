# 低优先四项 + Keka 简拆（补拆 §10.2 第 5–8 项）

> 拆解日期：2026-10-28。这四个本来只被「提过名字」，这里按「以后真要用时不用重新调研」的标准拆。
> **本文件里有一条比前三份文档都更值钱**：§3 的 inquire「取消 vs 中断」语义，直接给出了
> `docs/TODO.md` 表尾第 2 条（`--pick` 被 Ctrl-C 后光标不恢复）的**根因和第二条解法**。

---

## 1. Keka：**拆不了 —— 上游根本不给源码**（诚实记录）

`refs/外部-归档软件/Keka/`（git log 最新 `64dc0818` 2026-10-07）里只有：

```
Bin/            ← 4 个预编译 tgz，不是源码
Img/  Wiki/  tests/  Translations-{iOS,macOS,Web}/  .github/  FUNDING.yml
```

- `find -name '*.swift'` → **0 个**；`*.m` / `*.mm` / `*.h` → **0 个**
- `Bin/` 里是 `lbzip2.tgz`(648K) / `lzip.tgz`(104K) / `plzip.tgz`(170K) / `unar.tgz`(2.2M)

**结论**：Keka 是 macOS 上的**闭源 app**，这个仓库只是它的发布/翻译/反馈仓。所以
`docs/REFERENCE-INDEX.md:223` 说「Keka 只有一句」**没法通过读源码补救** —— 它本来就没有源码。

**能看出来的两件事（不算白来）**：

1. **Keka 自己也不写解码器**：`Bin/` 里塞的全是外部 CLI（`unar` = XADMaster 的命令行版、
   `lbzip2`/`plzip` 是并行 bzip2 实现）。和 FileRoller 的「一 CLI 一个子类」是同一思路。
   → 间接印证我们「自研内建引擎」是少数派但更干净的选择（也解释了为什么我们能在
   **假签名 / 内嵌封面 / 纯伪装包**上做别人做不到的事）。
2. **34 种语言**（`Translations-macOS/` 34 个 `.lproj`）—— 我们引擎 7 语言、界面还没接多语言
   （`docs/TODO.md` 阶段0 T7 已记为「架构决定，非缺陷」）。**不追**。

> 台账处理：把 §10.2 第 5 项从「🟢 低 —— 格式扩展时才用」改成
> **「❌ 无法拆（上游闭源），已用发布产物替代」**，免得下次又有人去 clone 一遍。

---

## 2. NanaZip：Core / Codec / Base 三层分离（`K7Base` 是关键）

`refs/外部-归档软件/NanaZip/`（Windows，C++，62 MB）目录结构：

| 目录 | 是什么 |
|---|---|
| `K7Base/` | **7-Zip 的底层 + Detours 钩子层**（`K7BaseDetours.cpp`、`K7BasePolicies.cpp`、`K7BaseMitigations.cpp`、`K7BaseModern.cpp`、`K7BaseHash.cpp`） |
| `NanaZip.Codecs/` | 每个格式一个 `NanaZip.Codecs.Archive.*.cpp`：**DotNetSingleFile / ElectronAsar / Littlefs / Romfs / Ufs / WebAssembly / Zealfs** … 还内嵌了 BLAKE3 / FreeBSD / GmSSL / LZ4 / LittleFS 等子库 |
| `NanaZip.Core/` | 自解压模块的**三种变体**：`Sfx.Console` / `Sfx.Setup` / `Sfx.Shared`（各有 `.vcxproj` + `.manifest`） |
| `NanaZip.ExtensionPackage/` | 打包成 Windows 扩展包（Store 分发） |

**值得记的三点**：

1. **`K7Base` 是「不改 7-Zip 源码，用 Detours 挂钩子改行为」的妥协方案**。
   反面教材：升级上游版本时要重做钩子。我们**不抄**（我们用 Rust 生态的库而不是 fork C 工程）。
2. **Codec 目录里出现了「开发者格式」**：ElectronAsar、.NET SingleFile、WebAssembly、UFS、Romfs、
   ZealFS —— 说明「归档格式」的外延已经扩到**应用打包产物**。
   我们的主场景是「伪装成视频的压缩包」，**短期不碰这些**，但记着：如果将来有人问
   「能不能打开 .asar / .wasm」，答案是「NanaZip 有现成枚举，照 Codec 清单加」。
3. **`Sfx.Console` / `Sfx.Setup` / `Sfx.Shared` 三档自解压** —— 我们只有一种（`assets/sfx/yshjy-sfx.exe`
   + `--sfx` / `--sfx-linux`）。**不追**，但知道「Windows 的自解压通常至少要 console 版和 GUI 版两种」。

---

## 3. ⭐⭐ inquire：`Esc`（取消）与 `Ctrl+C`（中断）是**两种不同的事情**

来源：`refs/upstream/inquire/`（`inquire/src/{error.rs,prompts/prompt.rs,terminal/console.rs}`）

### 3.1 语义定义（`inquire/src/error.rs:24-25` 取消 / `:27-33` 中断）

```rust
/// The user canceled the operation by pressing ESC.
OperationCanceled,

/// The operation was interrupted by the user after they
/// pressed Ctrl+C.
///
/// This error will be returned only when using `crossterm`
/// or `termion` as the terminal back-end. If using `console`,
/// pressing Ctrl+C will trigger SIGINT.
OperationInterrupted,
```

- **`Esc` = 取消**：用户主动放弃这一问，可以「跳过」（`prompt_skippable` → `Result<Option<T>>`），
  **收尾代码正常执行**（重画一帧、`frame_finish(true)`、把光标放回来）。
- **`Ctrl+C` = 中断**：用户要求「立刻停」，语义接近终止程序。

### 3.2 关键的那句注释（同上）

> **`If using console, pressing Ctrl+C will trigger SIGINT.`**

这就是我们 `docs/TODO.md` 表尾第 2 条的**根因**，之前记的是「中断走信号路径，`dialoguer` 的复原代码
没有执行机会」——现在有了上游的权威背书：

| 后端 | `Ctrl+C` 的处理 | 结果 |
|---|---|---|
| `console`（**dialoguer 用的**） | 终端驱动直接发 **SIGINT** | 进程被杀，**raw mode 与隐藏光标都没恢复** |
| `crossterm` / `termion` | 当**普通按键**读进来（`Key::CtrlC` → `KeyModifiers::CONTROL`） | 走 `Action::Interrupt` 分支，**收尾代码正常跑** |

`inquire` 的分派证据（`prompts/prompt.rs:141` 取消 / `:153` 中断）：

```rust
Action::Interrupt => return Err(InquireError::OperationInterrupted),
```

它**没有**在信号处理里做事 —— 因为压根没进信号路径。

### 3.3 对我们的两种解法（比现在文档里写的多一条）

`docs/TODO.md:84-98` 现在写的解法是「引 `signal-hook`，在 handler 里写 `\x1b[?25h` + `disable_raw_mode()` + `exit(130)`」。
**拆完 inquire 之后，多出一条更干净的路**：

**解法 B（新，推荐优先评估）：`--pick` 自己用 crossterm 读键，不用 dialoguer**

我们 `Cargo.toml` 里**已经有 `crossterm = "0.29"`**（`--list` 表格宽度用它）。
自己读键的话：
- `Ctrl+C` 是 `KeyCode::Char('c') + KeyModifiers::CONTROL` —— **普通按键，不触发 SIGINT**；
- 退出前在同一函数里 `execute!(stdout, Show)` + `disable_raw_mode()` —— 代码顺序可控；
- 不需要 `signal-hook` 依赖（`deny.toml` 白名单不用动）；
- 代价：要把 `dialoguer::MultiSelect` 的 60 行键位逻辑自己写回（分页、过滤、非 TTY 降级）。
  **这正是 `docs/REFERENCE-INDEX.md:136-140` 记下的「别自己写 raw mode 循环」那条教训的反面** ——
  两条冲突，权衡点在于：**我们只要「光标恢复」这一个行为，值不值得重写整个多选 UI？**

**建议**：先试解法 A（`signal-hook`，改动最小，只影响 `--pick` 进屏期间）；
如果 `signal-hook` 的 handler 里调 `disable_raw_mode` 出现不可重入问题（信号上下文不能干重活，
`docs/TODO.md:93` 自己已经写了这个警告），再退到 B。**别两条一起上。**

### 3.4 另一条可抄的：`prompt_skippable`

`prompt_skippable` 把「用户按 Esc 跳过」变成 `Ok(None)`，与「出错」区分开。
我们的 `--pick` 现在是「Esc/q 取消」→ 直接退出。如果将来做成「可选勾选、跳过就用默认全部」，
这个 API 形状是对的（`Option<T>` 而不是 `Err`）。

---

## 4. fastforge：打包工具的**五段流水线**分层

`refs/upstream/fastforge/crates/`（Flutter 桌面一键出 deb/rpm/AppImage/exe/dmg）：

```
app_analyzer   → 分析 Flutter 产物（哪个平台、产物在哪、版本号）
app_builder    → 编译/准备可分发物
app_packager   → 打成各平台安装包（deb/rpm/AppImage/exe/dmg）
app_publisher  → 上传到发布渠道
dmg_maker      → macOS 专用
stores         → 各应用商店渠道
core / remote  → 公共设施
```

**为什么值得记**：我们 `scripts/release.sh` + `build-deb.sh` 是一堆 if 分支，
要做多平台时会迅速失控。fastforge 的分层给出的答案：
**分析 → 构建 → 打包 → 发布，四段分开，各段自己的 crate/crate 目录**，
`stores` 和 `dmg_maker` 这种平台特例**单独成模块而不是塞进 if 里**。

⏸ 暂不动（多平台是最后一块），但 `docs/TODO.md` 多平台表里「谁出包」那一列，
将来应该按这四段去填，而不是按平台填。

---

## 5. cargo-dist：**配置即发布流水线**

`refs/upstream/cargo-dist/dist-workspace.toml:1-25`（摘录）：

```toml
[dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "powershell", "npm", "homebrew"]
tap = "axodotdev/homebrew-tap"
npm-scope = "@axodotdev"
publish-jobs = ["homebrew", "npm", "./publish-crates"]
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "aarch64-unknown-linux-musl",
           "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl",
           "x86_64-pc-windows-msvc"]
pr-run-mode = "plan"
hosting = "github"
install-updater = false
```

要点：**一个 toml 决定「出哪些目标三元组 × 生成哪些安装器 × 在 PR 上跑不跑」**，
剩下全是生成出来的 GitHub Actions YAML。

我们**用不上**（我们的产物是「一个 deb + 两个 tar.gz + Flutter GUI」，不是「多目标三元组的 Rust CLI」），
但**有一条必须记**：`x86_64-unknown-linux-musl` 这个 target —— 静态链接，**不依赖系统 glibc**。
这就是我们 AppImage 那次翻车（本机 glibc 2.41 基线）的**根治方案之一**。
`docs/RELEASE-CHECK.md` §4 里那套「bookworm 容器 + 只捆 GTK3」的配方，
将来可以补一句：**如果用 musl 静态编译，纯 CLI 版可以做到零 glibc 依赖**（GUI 版不行，GTK 还是要动态库）。

---

## 6. flutter_rust_bridge：确认「不走它」是对的

`refs/upstream/flutter_rust_bridge/` 结构：`frb_codegen`（代码生成器）/ `frb_dart`（Dart 运行时）/
`frb_rust`（Rust 运行时）/ `frb_macros`（过程宏）/ `frb_utils`。

**它的工作方式**：从 **Rust 侧**扫描 `pub fn`，生成一套 Rust FFI 胶水 + Dart 绑定。
钩子点在 `frb_macros`（`#[frb]` 属性）+ `frb_codegen` 扫描。

**为什么不选它**（`docs/REFERENCE-INDEX.md:174,190` 已定，这里补上理由）：
1. 它要求**改我们的导出层**（加 `#[frb]`、按它的规则组织模块），而我们 `ffi/` 的 C ABI 已经是
   JSON 进 / 事件出，**一行不用改**就能被 ffigen 吃。
2. 它引入两套运行时（`frb_dart` + `frb_rust`），包体积和版本耦合都上去了。
3. 我们的 ABI 有 `yshjy_abi_version()`，本来就要手控版本 —— frb 的自动生成会把这块藏起来。

**保留为备选**：如果将来 Dart 侧要频繁加新方法（比如预览树要暴露十几个新命令），
frb 的「Rust 写好自动生成绑定」会省事。到那时再看 `frb_example/` 里的现成例子。

---

## 7. 台账处理小结（给 `docs/REFERENCE-INDEX.md` §10 用）

| 项 | 原判定 | 拆完后的判定 |
|---|---|---|
| clap_complete / clap_mangen | 🔴 一行没读 | ✅ 已拆 → `refs/tech/clap-complete-mangen.md`，产出 **1 个真 bug + 3 条改进** |
| hiddify-core C ABI | 🔴 只抄 6 行 ffigen 配置 | ✅ 已拆（**需补 clone**）→ `refs/tech/hiddify-core-c-abi.md`，产出 3 条可抄 |
| libarchive | 🟡 只当格式来源 | ✅ 已拆 → `refs/tech/libarchive.md`，产出「声明大小封顶」等 2 条 |
| Ark / FileRoller | 🟡/零提及 | ✅ 已拆 → `refs/tech/ark-fileroller.md`，产出「菜单按可写性裁剪」 |
| Keka | 🟢 只有一句 | ❌ **无法拆（上游闭源，仓里只有 Bin/ + 翻译）** |
| NanaZip | 🟢 只提了 Core-Codec | ✅ 简拆（本文 §2） |
| inquire | 🟢 只抄了文档格式 | ✅ 简拆（本文 §3）——**产出最有价值的一条**：Ctrl-C 根因与解法 B |
| fastforge / cargo-dist | 🟢 只看了配置文件头 | ✅ 简拆（本文 §4/§5）—— cargo-dist 补一条 musl 静态链接 |
| flutter_rust_bridge | 🟢 零提及 | ✅ 简拆（本文 §6）—— 确认不选，理由记档 |
