# yshjy 整改与全平台方案（2026-10-28 定稿）

> **一句话**：先用四期把产品本身修干净，最后才上平台。
> 平台是放大器不是修复器 —— 现在修一处，五个平台各省一次；等出包了再回头改，每个平台都要重编译重验收。
>
> 本文是**方案**：找到什么、抄什么、什么顺序、怎么验收。
> 能打勾的条目只记在 **`docs/TODO.md`**（唯一真源，本文不重复列表）；
> 做法与源码行号在 `refs/tech/*.md`（5 份参考拆解）；代码结构在 `docs/INTERNALS.md`。

---

## 0. 一页看懂

| 项 | 结论 |
|---|---|
| 项目体量 | 引擎 **27,722 行 / 49 文件**；界面 **5,074 行 / 15 文件**；**0.9.28 已装机** |
| 对外面 | 10 协议命令 · 30 事件 · 16 错误码 · 37 CLI 选项 · 14 种 MIME 认领 |
| 测试 | `tests/all.sh` **29 通过 / 0 失败 / 4 跳过**（179 秒）+ 13 个 shell 探针 + python 探针若干 |
| 核心依赖 | `error` 被 **22** 个模块依赖（最底层）· `ctx` 10 · `event` 7 |
| **体检总评** | 代码比台账写的**健康得多**：台账 32 条「未做」里 **14 条其实早做完了** |
| 真 bug | **1 个**（bash 补全空格文件名） |
| 真缺口 | **9 个**（都不致命，但用户能感觉到） |
| 硬阻塞 | **1 个**（Windows 编不过：5 处 unix-only 没 cfg 隔离，实测 `E0433`） |
| 参考项目可抄 | **11 条值得 + 5 条记着 + 5 条明确不抄** |
| 分期 | 一期对齐止血（半天）→ 二期补缺口（1–2 天）→ 三期结构优化（2–3 天）→ 四期全平台 |

---

## 1. 体检：找到了什么（每条都有代码证据）

> 方法：**5 份参考项目拆解**（1123 行）× **代码全景**（49 模块依赖矩阵）× **台账 64 条逐条 grep 核实**
> × **环境实测**（交叉编译真跑一遍）。不靠印象。

### 1.1 A 类：真 bug（1 个）

| # | 问题 | 实测证据 |
|---|---|---|
| A1 | bash 补全在**没装 bash-completion** 的机器上，把带空格的文件名劈成两个候选 | 目录只有 `有 空格.zip` 时：我们 fallback 吐 `有` + `空格.zip`（**2 条 ❌**）；装 bash-completion 走 `_filedir` 是 1 条（✓）；clap 的 `IFS=$'\n'` 写法也是 1 条（✓） |

三处同病：`scripts/gen-cli-assets.py:274`（目录）、`:295`（文件）、`:312`（位置参数 fallback）。

### 1.2 B 类：台账失真（14 条）—— **最该先修的其实是这个**

> ✅ **2026-10-29 已重写完毕**：14 条全覆盖（另抓到 2 条同类失真），当前状态看
> `docs/REFERENCE-INDEX.md` 本身。本节留作**历史证据**，不用再按它改一遍。

`docs/REFERENCE-INDEX.md` 的状态列落后两三个版本：

| 台账项 | 实测证据（文件:行） |
|---|---|
| 密码多编码重试 | `src/extract.rs:178` 候选按「现代→中文Windows→7z」排序 |
| Unicode Path 0x7075 | `src/layout/zip.rs:375` |
| EOCD 分块倒扫 | `src/layout/zip.rs:128`（1 MiB 尾部窗口倒找；还留了「不要盲抄 SIMD」的实测） |
| 双维度识别进度 | `src/event.rs:521-523`（`total_bytes` + `total_files`） |
| 解压前内存预检 | `src/hardware.rs:92` `available_mem_mib` |
| 试密码可视化 | GUI 消费 `PASSWORD_TRY` |
| 完成卡分享文案 | `home_page.dart:1814`「复制解压方法」 |
| 新手引导 | `home_page.dart:110` |
| 历史页可点开 | `history_page.dart:128` `onTap` |
| 分卷生成 | `src/pack_post.rs:19-44`（`split` + `volumes`）※`pack/mod.rs:666`「暂不支持分卷」说的是**自解压**分支 |
| 多任务并发进度 | `main.rs:1238` indicatif `MultiProgress`（0.9.27） |
| 多选交互 `--pick` | `cli_spec.rs:71` + `src/pick.rs`（0.9.27） |
| 列表/表格输出 | `main.rs:872` comfy-table（0.9.27） |
| 补全与 man 自动生成 | `gen-cli-assets.py` + `cli_spec.rs`（0.9.27/0.9.28） |

**后果**：照着它排活 = 重复劳动。这一轮我差点被带偏（把已做的当没做）。

### 1.3 C 类：真缺口（9 个，确认代码里没有）

| # | 缺口 | 用户能感觉到什么 |
|---|---|---|
| C1 | 条目 **packSize 校验** | 包拼接错位时可能"安静解出垃圾"而不是报错 |
| C2 | 窗口**尺寸/位置记忆** | 每次开都要手动拉窗口 |
| C3 | 密码卡「**这次不保存**」 | 公共电脑上没法避免记住密码（`_recentPw` 已有） |
| C4 | 界面「**解完把原包移回收站**」开关 | 引擎支持 `on_done=trash`，界面没开关 |
| C5 | CLI **`--trash-source` / `--delete-source`** | 终端用户没有删源能力 |
| C6 | **预览树 + 部分解压**（界面） | 引擎有 `select`，界面还是平铺 |
| C7 | 进度条缺「**已用时**」 | 只有速度/剩余，判断不了"要不要等" |
| C8 | **干扰数据 / 假签名**（打包） | 伪装包没有伪特征码，反检测弱一档 |
| C9 | **移动端 seek 三档** | Android/iOS 大包要全量复制才能动 |

### 1.4 D 类：工程债与硬阻塞（4 个）

| # | 问题 | 状态 |
|---|---|---|
| D1 | **Windows 编不过**：5 处 unix-only 调用没 cfg 隔离（实测 `E0433`） | 🔴 硬阻塞 |
| D2 | 3 处反向依赖：`preview→job`、`drill→batch`、`safety→ask` | 合理妥协，已记档 |
| D3 | `main.rs` 2670 行（最大单文件） | 再加两个终端功能就该拆 |
| D4 | `ffi/include/yshjy.h` 手写第二份真源（43 行 vs 9 导出） | 改实现忘改头文件不报错 |

**D1 的 5 处**（一次扫完，别修一个编一次）：

| 位置 | 用的东西 | 修法方向 |
|---|---|---|
| `src/extract.rs:499` | `std::os::unix::fs::symlink` | `#[cfg(windows)]` 用 `symlink_file`，失败退回复制并标 `SLOW_PATH` |
| `src/extract_stream.rs:546` | 同上 | 同上 |
| `src/crash.rs:68-71` | `libc::localtime_r` / `libc::tm` | Windows 用 `GetLocalTime` 或纯 std 手算 |
| `src/main.rs:263` | `libc::signal(SIGPIPE)` | `#[cfg(unix)]` 整段包起来 |
| `src/main.rs:1215-1223` | `libc::termios` 关回显 | `#[cfg(windows)]` 走 crossterm 或明确降级 |

> 另有 4 处 `use std::os::unix::fs::PermissionsExt`（测试 / 权限还原里），编 Windows 时要逐个确认。

### 1.5 E 类：半截（3 个）

- 7z **头加密**：有 `encrypted` / `encrypted_count`，但「头加密时无密码连清单都列不出」没有专门提示
- `build-deb.sh` 有 `postinst` + `postrm`，**缺 `prerm`**
- GitHub 只有单平台 `ci.yml`，**没有多平台矩阵与发布汇总**

---

## 2. 对照参考项目：能抄的 16 条

> 判定标准：**有源码位置 + 我们确实没有 + 收益可描述**。已做的一律不列。

### 2.1 🔴 趁手就做（5 条）

| # | 抄谁 | 抄什么 | 落点 | 收益 |
|---|---|---|---|---|
| R1 | clap `aot/shells/bash.rs:190-201` | `IFS=$'\n'` 包 `compgen` | `gen-cli-assets.py` 三处 | 修 A1 |
| R2 | FileRoller `nautilus-fileroller.c:255-290` | 按**目标目录可写性**裁剪菜单（能写→「解压到此处」，不能写→只给「解压到…」） | `gui/linux/nautilus-script-yshjy`（现在零判断） | 只读目录不再给必然失败的入口 |
| R3 | libarchive `archive_read_private.h:184-192` | `entry_bytes_declared`：按头部声明大小给输出封顶 | `layout/zip.rs` + 写盘路径 | 修 C1 |
| R4 | indicatif `style.rs:270-350` | `{elapsed}` 字段 | `main.rs` 的 `进度消息()` | 修 C7 |
| R5 | hiddify（头文件是构建产物） | 头文件 ↔ 实现双向对账探针 | 新增 `tests/ffi-header-sync.sh` | 防 D4（同类病已栽两次） |

### 2.2 🟡 值得做，先想清（6 条）

| # | 抄谁 | 抄什么 | 落点 |
|---|---|---|---|
| R6 | clap `bash.rs:74-78` | `complete -o nosort -o bashdefault -o default`（bash≥4.4 才有 nosort） | 生成器注册行 |
| R7 | Ark 插件元数据 | **能力按格式声明**（现在全局一份 `caps`，`api.rs:85`） | 协议升版，GUI + FFI 两侧 |
| R8 | Ark `SupportsTesting` | `--test`：只测不写 | 新命令 + 契约探针 |
| R9 | clap `zsh.rs:400-420` / `fish.rs:308-320` | zsh `_files -/`、fish `(__fish_complete_directories)` | 生成器两行 |
| R10 | inquire `error.rs:27-33` | 取消（Esc 可跳过）vs 中断（Ctrl+C）语义；dialoguer 走 console → SIGINT 故光标不恢复 | `pick.rs`，解法 B（crossterm 读键） |
| R11 | hiddify `pubspec.yaml:197-203` | ffigen 六行配置 + `FfiTransport` 实现 | Android 地基 |

### 2.3 🟢 记着就行（5 条）

| # | 抄谁 | 内容 | 什么时候用 |
|---|---|---|---|
| R12 | NanaZip | 三档自解压 `Sfx.Console/Setup/Shared` | Windows 自解压体验升级 |
| R13 | cargo-dist | `x86_64-unknown-linux-musl` 静态链接 | 想出零 glibc 依赖的纯 CLI 包（GUI 不行） |
| R14 | Ark `KPluginMetaData` | Nautilus **一级**菜单（我们在「右键→脚本」二级） | 有人抱怨藏太深（要写 C/Python 扩展） |
| R15 | fastforge `crates/` | 四段流水线：分析→构建→打包→发布 | 多平台铺开时重构 `scripts/` |
| R16 | libarchive `choose_format` | bid 传 `best_bid` 提前剪枝 | 扫描顺序成为瓶颈时 |

### 2.4 明确不抄（5 条，免得反复讨论）

| 不抄 | 理由 |
|---|---|
| clap `engine/` + `env/` 动态补全 | 要程序内跑补全服务 + 用户 shell 加一行，对「装完就能用」是倒退 |
| Ark 插件化 | 单引擎够用；插件化只增加攻击面，与「常用版依赖越少」冲突 |
| FileRoller「一 CLI 一子类」 | GNOME 官方自己在往内建 libarchive 迁（**反向印证我们路线对**） |
| flutter_rust_bridge | 要改导出层 + 两套运行时；ffigen 直吃现成 C ABI |
| Keka 深度拆解 | 上游闭源：仓里只有 `Bin/`（4 个外部 CLI）+ 34 种翻译，零源码 |

---

## 3. 方案：四期，先内后外

> 每期独立可验收、独立可回滚（纯本地 git，一项一提交）。

### 3.1 第一期：对齐与止血（半天）

**目标**：让「账本」和「代码」重新一致，顺手修掉真 bug 与硬阻塞。

| 条目 | 来源 | 验收 |
|---|---|---|
| 1.1 重写 `REFERENCE-INDEX.md` 状态列（14 条失真） | B | 抽查 10 条与代码一致 |
| 1.2 bash 补全 IFS 保护（R1，三处） | A1 | 新探针 `tests/completions.sh`：带空格文件名只出 1 条 |
| 1.3 5 处 unix-only 做 cfg 隔离（D1） | D1 | `cargo build --target x86_64-pc-windows-gnu` 通过（不要求出包） |
| 1.4 新增 `tests/ffi-header-sync.sh`（R5） | D4 | 探针绿；故意删一个声明要红 |
| 1.5 进度条补「已用时」（R4） | C7 | 单测覆盖字段取舍；窄终端不超宽 |

**为什么 1.1 排最前**：不修它，后面所有人（包括我）都在拿错地图打仗。

### 3.2 第二期：补用户能感觉到的缺口（1–2 天）

| 条目 | 来源 | 验收 |
|---|---|---|
| 2.1 条目 packSize 校验（R3） | C1 | 构造拼接错位样本 → 报「数据不一致」而不是解出垃圾 |
| 2.2 窗口尺寸/位置记忆 | C2 | 重开窗口尺寸不变；引 `window_manager` 前先看许可证 |
| 2.3 密码卡「这次不保存」 | C3 | `flutter test` 断言勾选后不写入密码本 |
| 2.4 界面「解完移回收站」开关 | C4 | 开启后原包进 `~/.local/share/Trash` |
| 2.5 CLI `--trash-source` / `--delete-source` | C5 | `--delete-source` 二次确认 + 说明不可恢复，默认关 |
| 2.6 nautilus 脚本按可写性提示（R2） | C 体验 | 只读目录运行时给「请换个目录」而不是报错 |
| 2.7 `complete -o` 三项 + zsh/fish 目录补全（R6/R9） | A 补全 | `cli_spec_match.sh` 仍绿；zsh/fish 语法检查通过 |

### 3.3 第三期：结构优化（2–3 天，要设计）

| 条目 | 来源 | 前置 |
|---|---|---|
| 3.1 预览树 + 部分解压（界面） | C6 | 与 `select` 协议对接 |
| 3.2 干扰数据 / 假签名（打包） | C8 | **要定规则**：打多少、怎么打、会不会影响播放 |
| 3.3 `caps` 按格式声明（R7） | 结构 | 协议升版（`PROTOCOL.md`），GUI + FFI 同步 |
| 3.4 `--test` 只测不写（R8） | 功能 | 契约探针 |
| 3.5 `--pick` 的 Ctrl-C 光标（R10） | 体验 | 先 A（signal-hook），不可重入则退 B（crossterm） |
| 3.6 7z 头加密专门提示 | E | 文案 + 事件 |
| 3.7 `prerm` + GitHub 发布流水线 | E | 打包自检项增加 |

### 3.4 第四期：全平台

**前置：一期必须完成**（cfg 隔离是 Windows 的钥匙，ffi 探针是 Android 的钥匙）。详见 §4。

---

## 4. 全平台路线

### 4.1 现状实测（2026-10-28，全部真跑过）

| 项 | 状态 | 证据 |
|---|---|---|
| Linux 终端 + 桌面 | ✅ 完成并装机 | `dpkg -l yshjy` = 0.9.28；`tests/all.sh` 29 通过 |
| Windows 交叉目标 | ✅ 已装 | `x86_64-pc-windows-gnu` / `gnullvm` |
| mingw 链接器 | ✅ | `/usr/bin/x86_64-w64-mingw32-gcc` |
| wine（本机验证） | ✅ 10.0 | `wine-10.0 (Debian)` |
| **Windows 实际编译** | ❌ 失败 | `E0433 cannot find unix in os` @ `extract.rs:499`（共 5 处） |
| Flutter | ✅ 3.44.9 stable | `~/flutter/bin/flutter`（不在 PATH，脚本用绝对路径） |
| 五个平台脚手架 | ✅ 都在 | `gui/{android,ios,linux,macos,windows}` |
| JDK | ⚠️ 25（要 17） | Android Gradle 要求 17；apt 暂无 17 |
| Android SDK / NDK / cargo-ndk | ❌ 都没有 | 无 `ANDROID_HOME` |
| 磁盘 | ⚠️ 可用 67 G | SDK+NDK 约 15–20 G，够但紧（项目自身 16 G） |

### 4.2 Windows（一期完成后约 1 天）

```
① 5 处 cfg 隔离（一期 1.3 已完成）
② cargo build --release --target x86_64-pc-windows-gnu
③ wine 实测：zip / 7z / rar / 伪装包(.mp4) 各一 + 一个故意损坏的包
④ 出 zip：exe + README-Windows.txt（含 SmartScreen 提示）
⑤ GUI 版：只能在 Windows 上编（Flutter Windows 需 MSVC）→ 写 docs/BUILD-Windows.md
   交给用户自己编，**不承诺交叉出图形包**
```

**验收**：wine 4 样本成功 + 坏包给人话错误 + `yshjy.exe --version` 正确。

### 4.3 Android（3–5 天，两步）

**第一步：环境（要用户授权，约 15–20 G）**
```
① JDK 17：Adoptium tarball 解到 ~/ 独立目录（不碰系统 25）
② commandline-tools → sdkmanager：platform-tools、platforms;android-34、ndk;26.x
③ rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
④ cargo install cargo-ndk
```

**第二步：桥接（参考拆解在这里派上用场）**
```
⑤ cargo ndk -t arm64-v8a -o gui/android/app/src/main/jniLibs build --release（编 ffi/，已是 cdylib）
⑥ ffigen 六行配置（R11）→ gui/lib/gen/yshjy_bindings.dart（进 .gitignore）
⑦ 实现 FfiTransport（gui/lib/transport/ 已有接口注释）
⑧ 分区存储政策（先定再写）：私有目录直写 / 外部走 SAF / **不许静默失败**
⑨ flutter build apk --release（自签 keystore，不上架）
⑩ 体验不够再加 C9 的 seek 三档
```

**验收**：真机「文件管理器分享 → 解伪装包 → 进度动 → 落 SAF 选的目录」全链路；SAF 拒绝有明确提示。

### 4.4 苹果（1–2 天，卡在 CI）

```
① 本机能编的：ffi/ 静态库（crate-type 已含 staticlib）
② 出包必须在 macOS：GitHub Actions macos runner（公开仓免费额度够）
   照 localsend 拆法：一平台一个 reusable workflow + release 汇总；**钉老基线**
③ 口径（已定，别再问）：不上架、不公证、不买 99 美元账号；
   发源码/未签名包，谁要谁自己签（免费 Apple ID 自签 7 天可续）
④ 上 GitHub 前两件必修：`.git` 300 MB 历史里的编译产物要清；个人路径要脱敏
```

**验收**：CI 能出未签名产物 + 文档齐全；**不承诺**可直接安装的 ipa。

### 4.5 顺序与依赖

```
一期（台账/真bug/5处cfg/ffi探针/已用时）
   ├─→ 二期（用户能感觉到的缺口）
   ├─→ 三期（预览树/caps/--test/干扰数据/删源/光标）
   └─→ 四期
         ├─ Windows CLI（依赖 1.3 cfg 隔离）→ 可选 Windows GUI（用户自己编）
         ├─ Android（依赖 1.4 ffi 探针 + 环境安装授权）
         └─ 苹果（依赖「上 GitHub 前两件必修」）
```

**为什么平台排最后**：同一个问题在五个平台上修就是五倍成本；平台是放大器，不是修复器。

---

## 5. 验收口径（统一标准，别放松）

| 期 / 平台 | 算「做完」的条件 |
|---|---|
| 一期 | `bash tests/all.sh` 全绿 + 新探针绿 + 台账抽查 10 条一致 + Windows 目标能编译 |
| 二期 | 每个缺口有探针或在 `flutter test` 有断言；`flutter analyze` 不新增告警 |
| 三期 | 协议变化同步 `PROTOCOL.md` + 契约探针；新分支有单测 |
| Windows | wine 4 样本 + 坏包 + `--version`；真 Windows 上解压成功（用户协助） |
| Android | 真机全链路；SAF 拒绝有明确提示 |
| 苹果 | CI 能出未签名产物；文档齐全 |

**沿用铁律**：出包只走 `scripts/test-packages.sh`（33 项）；界面改完必须 `flutter build linux` 再 `cp` 引擎；
待办只改 `docs/TODO.md`。

---

## 6. 明确不做

| 不做 | 理由 |
|---|---|
| 迁 clap derive | 37 选项已过测，`cli_spec.rs` 已达成同样效果 |
| clap engine/env 动态补全 | 对「装完就能用」是倒退 |
| Ark 式插件化 | 单引擎够用，增加攻击面 |
| flutter_rust_bridge | 要改导出层 + 两套运行时 |
| AppImage | 本机 glibc 2.41 基线做出来没人能跑（配方留 `RELEASE-CHECK.md` §4） |
| 上架商店 / 买苹果开发者账号 | 花钱 0，发源码与未签名包 |
| Keka 深度拆解 | 上游闭源 |
| 界面 i18n 追到 34 种语言 | 架构决定，非缺陷；引擎 7 语言够用 |

---

## 7. 风险与回滚

| 风险 | 触发信号 | 应对 |
|---|---|---|
| cfg 隔离修完还有新 E0433 | 每修一处冒一处 | **一次全仓扫完再编**（已扫出 5 处，另 4 处 PermissionsExt 待确认） |
| 引 `window_manager` 带来依赖风险 | `flutter pub` 冲突 | 加依赖前查许可证与维护活跃度；不行就用本地 settings 存尺寸 |
| Android SDK 吃满磁盘 | `df` 掉到 20 G 以下 | 清 `target-full/`；删 `refs/外部-归档软件` 三份重复克隆（**删前必问用户**） |
| JDK 17 与系统 25 打架 | `flutter doctor` 报 Gradle JDK 错 | 独立目录 + `org.gradle.java.home`，不动系统默认 |
| 二期改动引入回归 | `tests/all.sh` 红 | 每项独立提交，红了单回滚 |
| CI 基线太新产物跑不起来 | 用户机报 glibc 符号缺失 | 钉 `ubuntu-22.04` / `macos-13` |

---

## 8. 文档索引（别找错地方）

| 想知道 | 去哪 |
|---|---|
| **能打勾的待办**（唯一真源） | `docs/TODO.md` |
| **往哪走、分几期、怎么验收** | **本文** |
| 代码现在长什么样 | `docs/INTERNALS.md` |
| 哪 14 条台账失真 + 逐条证据 | 本文 §1.2（据此重写 `REFERENCE-INDEX.md`） |
| 参考项目怎么拆、抄什么、行号 | `refs/tech/*.md`（5 份） |
| 每轮做了什么、踩了什么坑 | `HANDOFF.md` / `CHANGELOG.md` |
| 产品要什么样（用户视角） | `DESIGN.md`（2026-10-16 定稿仍有效） |
| 架构草案（**部分过时**） | `docs/ARCHITECTURE.md` |
