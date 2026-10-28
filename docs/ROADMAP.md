# yshjy 对照审计与全平台路线（2026-10-28）

> **这份文档只记「往哪走、为什么这么走、每步怎么验收」。**
> 执行条目（能打勾的那些）**只记在 `docs/TODO.md`** —— 项目铁律：待办只留一个文件一张表，
> 本文不重复列表，只解释取舍。状态随时以 TODO 为准。

---

## 0. 一句话结论

项目本身比台账写的健康得多（29 项「未做」里有 14 项其实早就做完了），
**真正挡住「全平台」的不是功能，是三件事**：

1. `src/extract.rs:499` 直接调 `std::os::unix::fs::symlink`，没有 cfg 隔离 → **Windows 目标今天编不过**（实测 E0433）
2. Android 前置环境一个都没装（JDK 17 / SDK / NDK / cargo-ndk），要 15–20 GB 磁盘
3. 台账（`docs/REFERENCE-INDEX.md`）的状态列落后两三个版本，照着它排活会白干

所以路线是（**2026-10-28 用户拍板：先把现有问题修干净，再谈平台**）：
**修台账与三个小阻塞（半天）→ 补用户能感觉到的缺口（1–2 天）→ 第三批设计项 → 平台（Windows → Android → 苹果）**。
理由：同一个问题在五个平台上修就是五倍成本；平台是放大器，不是修复器。

---

## 1. 审计方法（可复现）

三个来源对照，不靠印象：

| 来源 | 内容 | 怎么用 |
|---|---|---|
| **参考项目拆解**（5 份，1123 行） | clap / hiddify C ABI / libarchive / Ark+FileRoller / 其余四项 | 找「业界的做法」与「我们少的那块」 |
| **内部全景** `docs/INTERNALS.md` | 49 模块 / 依赖矩阵 / 7 个机制 / 30 事件 / 16 错误码 | 找「我们自己的结构问题」 |
| **台账逐条核实** | `REFERENCE-INDEX.md` 里 64 条状态，其中 **32 条 ❌/🟡/⏸ 全部逐条 grep 源码核实** | 分清「真缺口 / 台账失真 / 已做没记」 |
| **环境实测** | rustup target / mingw / wine / JDK / SDK-NDK / flutter / 磁盘 / 交叉编译真跑一遍 | 让「能不能做」有实据 |

> 核实脚本：`~/.hermes/cache/scratch/audit_ledger.sh`（一次性，逻辑已记在本文 §2.2 表里）。

---

## 2. 审计结论：四类，按「该不该马上动」排

### 2.1 真 bug（1 条，🔴 该马上修）

**bash 补全在没装 bash-completion 的机器上，把带空格的文件名劈成两个候选。**
实测：目录里只有 `有 空格.zip` → 我们的 fallback 给出 `有` + `空格.zip` 两条（错）；
装了 bash-completion 时（走 `_filedir`）是 1 条（对）；参考项目 clap 的 `IFS=$'\n'` 写法也是 1 条（对）。
修两行（`scripts/gen-cli-assets.py:295`、`:312`）。细节见 `refs/tech/clap-complete-mangen.md` §6.1。

### 2.2 台账失真（14 条：标 ❌ 但代码里已经做了）—— **这是最大的问题**

| 台账项 | 台账状态 | 实测证据 |
|---|---|---|
| 密码多编码重试 | ❌ 第20批 | `src/extract.rs:178` 字节候选按「现代→中文Windows→7z」排序 |
| Unicode Path 0x7075 | ❌ 第21批 | `src/layout/zip.rs:375` 注释与实现都在（version+crc32+utf8_name） |
| EOCD 分块倒扫 | ❌ 第21批 | `src/layout/zip.rs:128` 尾部窗口（默认 1 MiB）倒着找 EOCD；还留了「不要盲抄参考项目 SIMD」的实测结论 |
| 双维度识别进度 | ❌ 第21批 | `src/event.rs:521-523` 事件带 `total_bytes` + `total_files` |
| 解压前内存预检 | ❌ 第22批 | `src/hardware.rs:92` `available_mem_mib` |
| 试密码可视化 | ❌ 第20批 | GUI 消费 `PASSWORD_TRY` 事件（`home_page.dart:100` 注释 + 实现） |
| 完成卡分享文案 | ❌ 第22批 | `home_page.dart:1814` 「复制解压方法」按钮 + `:1810` 提示 |
| 新手引导 | ❌ 第23批 | `home_page.dart:110` 首帧后弹引导 |
| 历史页可点开文件 | ❌ 第20批 | `history_page.dart:128` `onTap` 打开所在目录 |
| 分卷生成 | ❌ 第20批 | `src/pack_post.rs:19-44` `split: Option<u64>` + `volumes`（**注意**：`pack/mod.rs:666`「暂不支持分卷」说的是**自解压**分支的限制，别误读） |
| 多任务并发进度 | ❌ 未做 | `main.rs:1238` 真出现第二个任务才切 indicatif `MultiProgress`（0.9.27） |
| 多选交互 `--pick` | ❌ 未做 | `cli_spec.rs:71` + `src/pick.rs`（0.9.27） |
| 列表/表格输出 | ❌ 未做 | `main.rs:872` comfy-table（0.9.27） |
| 补全与 man 自动生成 | ❌ 未做 | `scripts/gen-cli-assets.py` + `cli_spec.rs` 唯一真源（0.9.27，man 页 0.9.28 还修了版本号漂移） |

**结论**：`REFERENCE-INDEX.md` 的 §8 / §9 两张表整段是「开工前」拍的，0.9.27–0.9.28 做完后没回写。
**必须把状态列重写一遍**，否则下一次照着它排活就是重复劳动。这是第一批要做的第一件事。

### 2.3 真缺口（9 条：确认代码里没有 / 只有半截）

| # | 缺口 | 用户能感觉到什么 | 来源 |
|---|---|---|---|
| 1 | **条目 packSize 校验** | 包被拼接错位时我们可能"解出垃圾数据"而不是报错 | 台账 §1 + libarchive 的 `entry_bytes_declared` |
| 2 | **窗口尺寸/位置记忆** | 每次开都要手动拉大窗口 | 台账 §2 |
| 3 | **密码卡「不保存」勾选** | 公共电脑上没法避免记住密码（近期密码已有 `_recentPw`，但没「这次别记」） | 台账 §2 |
| 4 | **原包处理开关（界面）** | 引擎有 `config.output.on_done == "trash"`，但界面上没有「解完把原包移回收站」的开关 | 台账 §2 |
| 5 | **CLI 的删源开关** | `--delete-source` / `--trash-source` 都没有（只有配置项） | 台账 §8 |
| 6 | **预览树 + 部分解压（界面）** | 引擎有 `select`，但预览是平铺列表，没有树形勾选 | 台账 §2 |
| 7 | **进度条「已用时」** | 只有速度/剩余，没有 elapsed（clap 参考里 `{elapsed}` 是标准字段） | 参考拆解 §2 |
| 8 | **干扰数据 / 假签名（打包）** | 打出来的伪装包没有"伪特征码"，反检测弱一档 | 台账 §3 |
| 9 | **移动端 seek 三档** | Android/iOS 上大包解压要全量复制才动（SAF 不支持随机定位） | 台账 §5 |

> 另有 3 条「半截」：7z 头加密判定（有 `encrypted` / `encrypted_count` 字段，但「头加密时无密码连清单都列不出」这条还没做专门提示）、
> `prerm`（`build-deb.sh` 有 postinst + postrm，未见 prerm）、GitHub 发布流水线（只有 `ci.yml` 单平台检查，没有矩阵/发布）。

### 2.4 工程债（4 条）

| # | 债 | 为什么现在不动 | 什么时候必须动 |
|---|---|---|---|
| 1 | 3 处反向依赖（preview→job、drill→batch、safety→ask） | 都是合理妥协，拆了反而绕 | 想把引擎裁成「只预览」的轻量库时 |
| 2 | `main.rs` 2670 行（最大单文件） | 终端 UX 全挤在这，但功能都测过了 | 再加两个终端功能时，先拆 `--pick`/`--list` 出去 |
| 3 | `ffi/include/yshjy.h` 手写第二份真源 | 头文件里那些中文契约注释很值钱，不方便全自动 | **做 Android 前**补一个 `tests/ffi-header-sync.sh` 探针 |
| 4 | **`src/extract.rs:499` 用 `std::os::unix::fs::symlink` 无 cfg 隔离** | 只影响 Windows 目标 | **马上**（它是 Windows 路线的第一个阻塞） |

---

## 3. 优化清单：分三批（批次不是日期，是依赖顺序）

### 第一批：半天内能全做完（风险低，收益立现）

| 项 | 为什么排第一 |
|---|---|
| ① 重写 `REFERENCE-INDEX.md` 状态列（14 条失真） | 不修，后面所有人（包括我）都在拿错地图打仗 |
| ② bash 补全 IFS 保护 + 新探针 `tests/completions.sh` | 唯一真 bug，且改动只有两行 |
| ③ `src/extract.rs:499` cfg 隔离（+ 全仓扫一遍 unix-only API） | Windows 路线的前置，不做后面全堵着 |
| ④ `tests/ffi-header-sync.sh` 探针 | Android 前置（Dart 绑定依赖头文件准确） |
| ⑤ 进度条补「已用时」 | 一个字段，跟参考项目对齐 |

### 第二批：用户能感觉到的缺口（1–2 天）

| 项 | 用户感知 | 备注 |
|---|---|---|
| ⑥ 窗口尺寸/位置记忆 | 高（每次开都舒服） | 要加 `window_manager` 依赖，先跑 `cargo deny`/`flutter pub` 检查 |
| ⑦ 密码卡「这次不保存」勾选 | 中 | 已有 `_recentPw`，加一个布尔就够 |
| ⑧ 界面「解完把原包移回收站」开关 | 中 | 引擎早就支持（`on_done=trash`），只差界面一行 |
| ⑨ CLI `--trash-source`（+ `--delete-source`） | 中 | 引擎有 `trash.rs`，只是没接 CLI 选项 |
| ⑩ 条目 packSize 校验（按声明大小封顶） | 高（数据正确性） | 语义照 libarchive `entry_bytes_declared`，见 `refs/tech/libarchive.md` §4 |

### 第三批：要设计的（等第二批清完再说）

| 项 | 为什么要单独一批 |
|---|---|
| ⑪ 预览树 + 部分解压（界面） | 树形控件 + 勾选状态 + 与 `select` 协议对接，是一个小功能模块 |
| ⑫ 干扰数据 / 假签名 | 要定「打多少、怎么打、会不会影响播放」的规则，属产品决策 |
| ⑬ 移动端 seek 三档 | 只在 Android/iOS 有意义，跟平台路线一起做 |
| ⑭ `caps` 按格式声明（Ark 的做法） | 改协议面（`PROTOCOL.md` 升版），影响 GUI + FFI 两侧 |

---

## 4. 全平台路线（这是本文的重点）

### 4.1 现状实测（2026-10-28，全部真跑过）

| 项 | 状态 | 证据 |
|---|---|---|
| Linux 终端 + 桌面 | ✅ **已完成并装机** | `dpkg -l yshjy` = 0.9.28；`tests/all.sh` 29 通过 |
| Windows 交叉编译目标 | ✅ 已装 | `x86_64-pc-windows-gnu` + `x86_64-pc-windows-gnullvm` |
| mingw 链接器 | ✅ | `/usr/bin/x86_64-w64-mingw32-gcc` |
| wine（本机验证用） | ✅ 10.0 | `wine-10.0 (Debian)` |
| **Windows 实际编译** | ❌ **失败** | `error[E0433]: cannot find unix in os` @ `src/extract.rs:499` |
| Flutter | ✅ 3.44.9 stable | `~/flutter/bin/flutter`（不在 PATH，脚本里用绝对路径） |
| 平台脚手架目录 | ✅ 五个都在 | `gui/{android,ios,linux,macos,windows}` 已 `flutter create` 生成 |
| JDK | ⚠️ **25**（太新） | Android Gradle 要 17；apt 仓库暂无 17 |
| Android SDK / NDK | ❌ 都没装 | 无 `ANDROID_HOME`；`~/Android/Sdk` 不存在 |
| `cargo-ndk` | ❌ 没装 | |
| 磁盘 | ⚠️ 可用 67 G | Android 全套约吃 15–20 G，够但不宽裕（项目自身 16 G，其中 refs 1.5 G） |

### 4.2 Windows：最快见效，但要先拆一个阻塞（预计 1 天）

```
① 修 cfg 隔离：src/extract.rs:499 的 symlink
   #[cfg(unix)]   → std::os::unix::fs::symlink
   #[cfg(windows)] → std::os::windows::fs::symlink_file（符号链接在 Windows 要管理员权限）
                    失败时退回「复制一份」并在事件里标 SLOW_PATH（不静默）
② 全仓扫 unix-only API：grep -rn "std::os::unix\|std::os::linux\|libc::" src/
   （`io/atomic.rs` 的 rename、`safety.rs` 的权限、`hardware.rs` 的内存读取都要过一遍）
③ cargo build --release --target x86_64-pc-windows-gnu
④ wine 实测：解 zip / 7z / rar 各一个 + 一个伪装包（`.mp4`）
⑤ 打包：zip 里放 exe + README-Windows.txt（中文、含"双击后可能被 SmartScreen 拦"的说明）
⑥ GUI 版：**只能在 Windows 上编**（Flutter Windows 需要 MSVC 工具链）→ 写 `docs/BUILD-Windows.md` 交给用户在自己那台 Win 机器上编，我们不承诺
```

**验收标准（缺一不算完）**：wine 里 4 个样本全成功 + 一个故意损坏的包能给出人话错误 + `yshjy.exe --version` 正确。

### 4.3 Android：前置最重，分两步走（预计 3–5 天）

**第一步：环境（需要用户授权 sudo / 下载，约 15–20 G）**

```
① JDK 17：apt 没有 → 用 Adoptium tarball 解到 ~/ 下（不碰系统 JDK 25，避免影响别的项目）
② Android commandline-tools → sdkmanager 装 platform-tools、platforms;android-34、ndk;26.x
③ rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
④ cargo install cargo-ndk
```

**第二步：桥接（这就是参考项目拆解用上的地方）**

```
⑤ cargo ndk -t arm64-v8a -o gui/android/app/src/main/jniLibs build --release
   （编的是 ffi/ 那个 crate，不是主 crate —— 它已经是 cdylib）
⑥ Dart 侧 ffigen：照 hiddify 的 6 行配置（refs/tech/hiddify-core-c-abi.md §7.3）
   entry-points: ffi/include/yshjy.h → 输出 gui/lib/gen/yshjy_bindings.dart（进 .gitignore）
⑦ 实现 FfiTransport（gui/lib/transport/ 里已有接口与注释，只差实现）
⑧ 分区存储政策（**必须先定，别边写边想**）：
   私有目录（App 自己的 files/）→ 直接写
   外部目录 → 走 SAF 授权后写
   **不许静默失败**（拿不到权限就明确报错并给"换个目录"的按钮）
⑨ flutter build apk --release（自己生成 keystore，不上架）
```

**验收标准**：真机装 APK → 从"文件管理器分享到 yshjy"→ 解一个伪装包 → 进度条动 → 结果落 SAF 选的目录。
（**注意**：第 9 条 seek 三档在这步才需要，否则大包会先全量复制 —— 这是已知性能折衷，不是 bug。）

### 4.4 苹果（macOS GUI + iOS）：必须靠 mac 机器，我们不本机做（预计 1–2 天，卡在 CI）

```
① 本机能编的：ffi/ 的静态库（crate-type 已含 staticlib）—— 可交叉编 aarch64-apple-ios，但**链接与签名不行**
② 真正出包必须在 macOS：GitHub Actions 的 macos runner（公开仓免费额度够）
   工作流照 localsend 的拆法：一平台一个 reusable workflow + release.yml 汇总
   **钉死老基线**（与 AppImage 那次同一个坑：基线太新 = 用户跑不起来）
③ 苹果口径（用户 2026-10-28 已定，别再问）：不上架、不公证、不买 99 美元账号，
   发源码 / 未签名包，谁要谁自己签（免费 Apple ID 自签 7 天有效期，可续）
④ 上 GitHub 前**两件必修**（早先探测过，别重查）：`.git` 300 MB 历史里有编译产物要清；个人路径要脱敏
```

**验收标准**：iOS 侧只承诺"源码 + 编译文档 + CI 配置可跑通"，**不承诺**我们能出可直接安装的 ipa。

### 4.5 顺序与依赖

```
第一批（修台账/补全/cfg隔离/ffi探针/已用时）
   │
   ├──→ Windows CLI  ← 只依赖第一批（cfg 隔离是关键）
   │        └→ (可选) Windows GUI：用户自己在 Win 上编
   │
   └──→ Android ← 依赖第一批的 ffi 探针 + 环境安装（要用户授权）
            └→ 依赖 ⑬ seek 三档（大包体验）

苹果：与上面并行不冲突，但必须等「上 GitHub 前两件必修」做完
```

**推荐动手顺序**（用户 2026-10-28 拍板，以这条为准）：

```
第一批（修台账/真 bug/阻塞）  →  第二批（用户能感觉到的缺口）  →  第三批（要设计的）
                                                              ↓
                                    Windows CLI → Android → 苹果
```

**理由**：平台是放大器不是修复器 —— 现在修一处，五个平台各省一次；现在不修，
等 Windows 出包、Android 出 APK 之后再回头改，每个平台都要重编译重验收一遍。
所以 §4 的全平台路线**保留结论、但排在 §3 三批之后执行**；§4.2 的 cfg 隔离例外 ——
它是**第一批**里的活（因为顺手做掉最省事，且不影响其它任何东西）。

---

## 5. 每批的验收口径（统一标准，别放松）

| 批次 | 算「做完」的条件 |
|---|---|
| 第一批 | `bash tests/all.sh` 全绿 + 新探针绿 + `REFERENCE-INDEX` 状态列与代码一致（抽查 10 条全对） |
| Windows | wine 跑 4 个样本 + 1 个坏包 + `--version`；zip 包能在真 Windows 上解压（用户协助验证） |
| 第二批 | 每个缺口都有探针或在 `flutter test` 里有断言；`flutter analyze` 不新增告警 |
| Android | 真机 APK：分享→解压→进度→落盘 全链路；SAF 拒绝时有明确提示 |
| 苹果 | CI 能出 dmg/未签名 ipa（哪怕只在 CI 里验证）；文档齐全 |

**铁律（沿用项目现有约定）**：
- 出包只用 `scripts/test-packages.sh`（33 项全绿才算）
- 界面改完必须 `flutter build linux` 再 `cp` 引擎（HANDOFF 里记过踩坑）
- 待办只改 `docs/TODO.md`，本文只改路线

---

## 6. 明确不做（写下来免得反复讨论）

| 不做 | 理由 |
|---|---|
| 迁移到 clap derive | 37 个选项已过测，`cli_spec.rs` 已经拿到同样效果；迁移动的是"已验收的路" |
| 引 flutter_rust_bridge | 要走 Rust 侧改导出层 + 两套运行时；ffigen 直吃现成 C ABI |
| 插件化架构（Ark 那样动态加载） | 单引擎够用，插件化只增加攻击面，与"常用版依赖越少"冲突 |
| AppImage | 本机 glibc 2.41 基线做出来没人能跑；配方留在 `docs/RELEASE-CHECK.md` §4 |
| 上架任何商店 / 买苹果开发者账号 | 用户口径：花钱 0，发源码与未签名包 |
| Keka 深度拆解 | 上游闭源（仓里只有 Bin/ + 翻译），已记档 |
| 追多语言到 34 种 / 现在接界面 i18n | 架构决定，非缺陷；引擎已 7 语言够用 |

---

## 7. 风险与回滚

| 风险 | 触发信号 | 应对 |
|---|---|---|
| Windows 上 unix-only API 不止一处 | 修完 `extract.rs:499` 又冒新的 E0433 | 一次全仓 `grep std::os::` 扫完再编，别逐个试 |
| Android SDK 吃满磁盘（67 G 可用） | `df` 掉到 20 G 以下 | 先清 `target-full/`（旧编译产物）+ `refs/外部-归档软件` 里三份重复克隆（283 MB 里的百来 MB，**删前问用户**） |
| JDK 17 与系统 JDK 25 打架 | `flutter doctor` 报 Gradle JDK 错 | 用独立目录 + `org.gradle.java.home` 指过去，不动系统默认 |
| CI 基线太新导致产物跑不起来 | 用户机上报 glibc 符号缺失 | 钉 `ubuntu-22.04` / `macos-13`，与 localsend 一致 |
| 第二批改动引入回归 | `tests/all.sh` 红 | 每项独立提交，红了单回滚（项目本来就是纯本地 git） |

---

## 8. 与其它文档的关系（别找错地方）

| 想知道 | 去哪 |
|---|---|
| 现在能打勾的待办 | **`docs/TODO.md`**（唯一真源） |
| 代码现在长什么样（模块/机制/落点） | `docs/INTERNALS.md` |
| 设计承诺 vs 实际（含 14 条失真） | `docs/REFERENCE-INDEX.md`（**状态列按本文 §2.2 重写**） |
| 参考项目怎么拆的、抄什么 | `refs/tech/*.md`（5 份） |
| 每轮做了什么、踩了什么坑 | `HANDOFF.md` / `CHANGELOG.md` |
| 往哪走、为什么、平台怎么切 | **本文** |
