# 安卓版（APK）方案预案

> **状态：方案，未开工。** 环境没装、一行代码没改。
> 用户 2026-11-05 拍板"要做安卓版、发 GitHub"，本文是那之后整条路的思路归档。
> **待办不在这里** —— 唯一真源是 `docs/TODO.md`；本文只记「为什么这么定」和「要跨哪些坑」。
> 版本号以 `Cargo.toml` 为准，本文不写死。

---

## 1. 用户已拍板的决定（2026-11-05）

| # | 决定 | 原话/口径 |
|---|---|---|
| 1 | 要做安卓版，**发 GitHub 给通用安卓用** | 「我要发github所以是通用安卓什么小米啥的都可以用」 |
| 2 | 存储走「所有文件访问」 | 「b」（对应本文 §4.2 的 B 方案） |
| 3 | 鸿蒙**暂缓**（不是否决：手上没有鸿蒙设备，做出来验不了） | 「那就不管鸿蒙行不行」；2026-11-05 补充「鸿蒙不是不做而是我的设备做不了，苹果和苹果电脑也是」 |

**推论（必须写进 README，否则别人下了装不上）**：

- 华为纯血鸿蒙（HarmonyOS 5 / NEXT 起）**不认 APK**，只认 `.hap` → 明确写「暂不支持」。
- 小米/红米等开了「纯净模式」的机器，装未过审 APK 会弹「应用未上架」→ 写「选『继续安装』」。
- 其它安卓机（OPPO / vivo / 荣耀 / 三星 / 一加）不受影响。

---

## 2. 为什么这条路能走（本项目的既有地基）

| 项 | 现状 | 出处 |
|---|---|---|
| 引擎依赖 | **全是纯 Rust 库**（rars / sevenz-rust2 / zip / flate2 / lzma-rs / ruzstd / bzip2-rs …），没有绑系统 libarchive、没有调 7z/unrar 外部程序 | `Cargo.toml` |
| C ABI | **已有成品**：`crate-type = ["rlib","cdylib","staticlib"]`、`ffi/` 独立 crate、9 个 `extern "C"` 导出、JSON 进 JSON 出 | `ffi/src/lib.rs`、`ffi/include/yshjy.h` |
| 顺序读适配 | `io/blockcache.rs` 早就写了「给只能顺序读的源套上可 seek 的外观 —— 手机存储（SAF）/ 管道流」 | `src/io/mod.rs` 模块表 |
| seek 三档探测 | `src/seek.rs` 探测 + 协议 `scan.seek` 字段已就位（**手机上属于哪一档未实测**） | `docs/TODO.md` §安卓批 seek 清单 |
| 安卓脚手架 | `gui/android/` 目录已在（AGP 9.0.1 / Kotlin 2.3.20 / compileSdk 与 Flutter 默认一致） | `gui/android/` |

**唯一必须改的架构事实**：界面现在靠 `Process.start` 起子进程说话（`gui/lib/engine.dart` 头注释写的是「默认传输是 `Process.start` + 逐行 JSON（桌面/Android）」）。
**安卓不允许起子进程跑我们自己的二进制** → 必须换成进程内 FFI。
`FfiTransport` 目前在 `gui/lib/transport/engine_transport.dart` 里**只有注释、没有实现**。

---

## 3. 参考项目里挖到的（都真读过代码，不是抄结论）

### 3.1 rustdesk（Flutter + Rust，安卓真上架过）

`refs/upstream/rustdesk/flutter/android/app/build.gradle`

- `minSdkVersion 22` / `targetSdkVersion 36` / `compileSdkVersion 36`
- 签名走 `key.properties` 四件套（`keyAlias` / `keyPassword` / `storeFile` / `storePassword`），文件不存在就不签 —— 这个模式我们照抄
- Java 17 / Kotlin jvmTarget 17

### 3.2 localsend（安卓分区存储的标杆）

| 它在干什么 | 文件 | 对我们的意义 |
|---|---|---|
| 用 Flutter 官方 `file_selector` 选目录，拿回的是 `content://…/tree/…` | `app/lib/util/native/file_picker.dart` | 我们也是这条依赖，**选目录能直接用** |
| 把 tree URI 反解成真实路径（`primary:Documents` → `/storage/emulated/0/Documents`） | `packages/localsend_isolates/lib/util/content_uri_helper.dart` | 拿得到真路径才有机会用 `dart:io` 直接写 |
| 写 SAF 文件：Kotlin 建文件 → 取**可写 fileDescriptor** → 往里写 | `android/.../MainActivity.kt` + `file_saver.dart` | 引擎要支持「输出到 fd」，这条路才通 |
| 自研 `FastDocumentFile`：「Similar to AndroidX DocumentFile, but faster by querying all fields at once」 | `android/.../FastDocumentFile.kt` | 说明官方 DocumentFile 逐字段查询太慢，**解压写很多文件时这个开销是真实存在的** |

### 3.3 hiddify（别抄的地方 + 一条治病的）

`refs/tech/hiddify-core-c-abi.md`

- 它安卓走 `gomobile bind`（Go），**不走 ffigen** → 说明「ffigen 是安卓标准答案」这个说法不成立；ffigen 只适合我们这种「本来就有 C ABI」的情况。
- ⭐ **头文件是构建产物、不进仓库**：`go build -buildmode=c-archive` 顺带产 `.h`，一句 `mv` 改名。
  我们 `ffi/include/yshjy.h`（43 行）是**手写的第二份真源**，`ffi/src/lib.rs`（9 个导出）改了忘改头文件不报错 —— 这是本项目的既有隐患，做安卓前顺手治掉。

---

## 4. 关键设计

### 4.1 桥接：ffigen（不是 flutter_rust_bridge）

判据是「引擎本来就有 C ABI」→ 选 ffigen，`flutter_rust_bridge` 是从 Rust 侧生成桥接层，适合本来没有 C 接口的引擎（`docs/ROADMAP.md` §6 也把 frb 列进「明确不做」）。

链路：

```
ffi/ (cdylib)  --cargo-ndk-->  .so  --放进-->  gui/android/app/src/main/jniLibs/<abi>/
                                                   ↓
                              ffigen 读 yshjy.h → gui/lib/gen/yshjy_bindings.dart（生成物，进 .gitignore）
                                                   ↓
                              gui/lib/transport/ffi_transport.dart（实现 EngineTransport 接口）
```

### 4.2 存储：三条路，用户选 B

问题本质：**解压是「写很多个文件」**，安卓 10+ 分区存储不许随便写共享目录。

| 方案 | 做法 | 代价 |
|---|---|---|
| **A 私有目录** | 解到 `getExternalFilesDir()`，零权限、立刻能写 | 多一步「导出」；卸载 App 会连文件一起删 |
| **B 所有文件访问**（用户选） | 声明 `MANAGE_EXTERNAL_STORAGE` + `ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION` 跳系统设置页让用户勾「允许管理所有文件」 | 权限文案吓人；各 ROM 设置路径不同；引擎不用改（拿真路径直接写） |
| **C MediaStore** | `ContentResolver.insert(MediaStore.Downloads)` + `RELATIVE_PATH`，**零权限**写 `Download/子目录/` | 引擎要支持写 fd；1200 个文件 = 1200 次 insert，性能要实测；Android 9 及以下没有这条路 |

**已记录的担心（写在这里免得以后重吵）**：

1. B 的权限文案「允许此应用访问和管理您设备上的所有文件」对普通用户极其劝退 —— 与用户「给普通人用的工具别加专业感」的产品观冲突。
2. 各家路径：小米 设置→应用设置→应用管理→本App→权限→所有文件；华为 设置→隐私→权限管理→特殊访问权限；OPPO/vivo 又一套。
3. 开了这个权限，我们对「删错文件」的责任变重。

**折中建议（待用户最终点头，见 §7 待定项）**：默认 A（零权限、装上就能用）+ B 做成设置里的开关 +
「导出到下载」走 C。第一版不碰敏感权限，最快能装机。

### 4.3 引擎要开的两个新口子

| 口子 | 为什么 | 改哪 |
|---|---|---|
| 输入能吃 fd / 流 | SAF 给的是 `content://`，不是路径；`dart:io` 直接开会抛 `PathAccessException`（Android 11+） | `src/io/` 已有 trait 抽象，加一层「fd 来源」实现 |
| 输出能写 fd | 走 C 方案（MediaStore）或 localsend 那条 SAF 写法的必要条件 | `src/io/atomic.rs` 附近，原子写入的 `.yshjy-part` 中间态要保留 |

**不做也不会死的项**：B 方案下输出仍是真路径，这两个口子可以先只做「输入」。

### 4.4 其它平台差异（逐条对应到代码）

| 差异 | 影响 | 处理 |
|---|---|---|
| 不能起子进程 | `engine.dart` 默认传输失效 | 换 `FfiTransport` |
| `/proc/meminfo`、`/proc/mounts` 语义不同 | `src/hardware.rs` 的「机械盘还是固态」探测在手机上无意义 | 加 `cfg(target_os="android")` 直接返回「不适用」 |
| 调系统的 `sync` 命令 | `src/flush.rs` 用 `Command::new("sync")` 强制落盘 | 安卓分支改成进程内 `libc::sync()` 或文件级 fsync |
| TUI 依赖（crossterm / dialoguer / indicatif / comfy-table） | 全在 `src/main.rs`（bin），`lib.rs` 干净 | 编 `--lib` 时不拖进来；但 `Cargo.toml` 里是无条件依赖，需改成 `cfg(not(target_os="android"))` 或 optional，否则可能编译期报错 |
| `zstd` 依赖 C 库（`zstd-sys`） | 交叉编译要 NDK clang 参与；手机不做打包 | 安卓构建关掉打包相关 feature |
| 桌面专属界面件 | `desktop_drop`（拖拽）、`tray.dart`（托盘）、滚轮、Ctrl 快捷键、右键菜单 | 手机端按 `Platform.isAndroid` 隐藏，触摸版重排 |
| Edge-to-edge（Android 15+ 强制） | 界面会画到状态栏/导航栏底下 | Flutter 侧显式处理 `MediaQuery` padding / SafeArea |
| 长任务被杀 | 解 2G 的包要几分钟 | 前台服务 + `foregroundServiceType="dataSync"` + `FOREGROUND_SERVICE_DATA_SYNC` 权限（Android 14+ 漏了直接崩溃；Android 15 起 dataSync 有 6 小时上限） |
| 没有「拖文件进来」 | 手机最顺的入口其实在文件管理器 | 加 `VIEW` / `SEND` intent-filter：「长按假视频 → 打开方式 → 杨三海解压」 |

### 4.5 构建参数（照 Flutter 3.44.9 的默认值，实测自本机 SDK 源码）

`~/flutter/packages/flutter_tools/gradle/src/main/kotlin/FlutterExtension.kt`：

- `compileSdkVersion = 36`、`targetSdkVersion = 36`、`minSdkVersion = 24`、`ndkVersion = "28.2.13676358"`
- minSdk 24 = Android 7.0，覆盖绝大多数在用设备
- **16 KB 内存页**（Android 15 起、2025-11-01 后 Google 强制）：NDK r28 默认支持 → 只要用 r28 的 linker 就合规，别退回旧 NDK
- ABI：`arm64-v8a` 必出；`armeabi-v7a` 覆盖老机（体积各 +4~6 MB）；`x86_64` 只在模拟器调时用
- 签名：自签 keystore，**只生成一次，之后每个版本都用它签**（换了签名同包名装不上）；备份两份（本机 + 外置盘）

---

## 5. 分批计划（每批能单独验收）

| 批 | 内容 | 预估 | 验收 |
|---|---|---|---|
| 0 | 发 GitHub 的前置（清 `.git` 252MB 里的编译产物、个人路径脱敏、生成 keystore、定许可证） | 0.5–1 天 | `git clone` 下来体积正常；`grep` 无 `/home/user` |
| 1 | 装环境：Android commandline-tools + `platforms;android-36` + `ndk;28.2` + `cargo-ndk` + 三个 Rust target | 0.5–1 天 | `flutter doctor` Android 那行打勾；`cargo ndk --version` |
| 2 | **第一块 .so 能跑**（止损点） | 0.5 天 | ① `readelf` 看 16KB 对齐 ② Flutter `dlopen` 调一次 `--version` ③ 真机装上能打开 |
| 3 | 桥接 + 存储 | 1–2 天 | `FfiTransport` 通；真机「选文件 → 解 → 落在目标目录」；SAF 拒绝有明确提示 |
| 4 | 界面手机版 | 2–3 天 | 小屏布局、触摸操作、edge-to-edge 不压字、前台服务 + 通知、分享 intent |
| 5 | 打包发布 | 0.5 天 | 自签 APK 装到真机；Release 页附 sha256；README 写清纯净模式/鸿蒙 |

合计 **5–7 天**（不含突发）。`docs/ROADMAP.md` §4.3 原来估的 3–5 天没算触摸界面重做，按 5–7 天说更诚实。

**批 2 是止损点**：三件事任一不过，说明方案要换，立刻停下来汇报，不硬撑。

---

## 6. 风险清单

| 风险 | 触发信号 | 应对 |
|---|---|---|
| 磁盘不够 | 只剩 51 G（2026-11-05 实测，10-28 时 74 G）；SDK+NDK 吃 15–20 G，Gradle 缓存还要几个 G | 装到外置 NVMe 盘；或先清 `target-full/`（**清之前问用户**） |
| JDK 版本 | 系统默认 25；本机另有 JDK 21 / 8 | 项目级 `android/gradle.properties` 指 JDK 21（AGP 9 支持），**别改系统默认** |
| `local.properties` 记死旧 SDK 路径 | 换了 SDK 后报找不到 | 删掉让它重新生成（现在里面只有 `flutter.sdk=`，还没有 Android SDK 路径） |
| Flutter 3.44 + AGP 9 太新 | Gradle/AGP 报兼容错 | 优先降 AGP，最后才考虑降 Flutter（降 Flutter 会动界面） |
| 引擎编不过安卓 | 出现 `cfg` 缺失的 unix-only 调用 | 一次全仓扫完再编，别修一处编一次（`docs/ROADMAP.md` §7 同款教训） |
| SAF 只能顺序读、大包要整份复制 | 「会先复制一份，占 X MB」提示要提前给 | 已有 `io/blockcache.rs`；预检数字用 `safety::check_space` |
| 签名 keystore 丢失 | 以后发不了能覆盖安装的升级包 | 生成后立刻备份两份，密码记在用户自己的地方 |

---

## 7. 待用户拍板（都还没定）

1. **存储最终方案**：纯 B（一启动就要所有文件权限）还是 A+B 开关 + C「导出到下载」（本文 §4.2 的折中建议）。
2. **许可证**：`Cargo.toml` 现在写 `MIT`；用户以前说过「自己项目开源用 GPL-3.0」。发 GitHub 前必须定，已发布的版本改不了口。
3. **目标真机**：现在只有 iPhone。安卓机有没有 / 什么型号？（没真机 → 只能做到「能编出包」，验收层写「未验」）
   同一件事也是鸿蒙/苹果暂缓的唯一原因（用户原话「我的设备做不了」），不是技术判断。
4. **磁盘策略**：系统盘只剩 51 G，是否允许把安卓工具链装到外置 NVMe 盘。

---

## 8. 明确不做 / 暂缓（避免以后重吵）

> 下面分两种：**「不做」= 产品/成本上否决**；**「设备做不了」= 想做得有设备，不是否决**。
> 列清单时别把后者当欠账催，也别当前者跳过 —— 它等的是硬件，不是决定。

| 项 | 理由 |
|---|---|
| 鸿蒙 HAP | **不是不做，是设备做不了** —— 手上没有鸿蒙真机，做出来无法验收（规矩：没验过不能说做好）。技术上走得通（FlutterOH 有 3.44 线预览版；Rust 的 `aarch64-unknown-linux-ohos` 是 Tier 2）；等有鸿蒙设备再启。届时还要另议分发（HAP 要签名、侧载要电脑/工具），磁盘和工具链也得加第二套 |
| macOS / iOS | **不是不做，是设备做不了** —— 出包必须在 macOS 上编，手上没有苹果电脑。免费 CI（GitHub Actions macos runner）是现成解法，前提是先上 GitHub |
| 上 Google Play | 自签分发，不买开发者账号，不进商店审核 |
| `flutter_rust_bridge` | 引擎本来就有 C ABI，用 ffigen 即可（`docs/ROADMAP.md` §6 已列不做） |
| 手机版「打包 / 伪装成视频」 | `src/pack/mod.rs` 依赖 `ffmpeg`，手机上不需要这个能力 |
| 为华为单独做兼容层 | 卓易通那类方案不是我们能控制的东西 |

---

## 9. 本文证据的时效

- 平台约束（16 KB 页、前台服务类型、edge-to-edge、MediaStore、纯血鸿蒙）：查证于 **2026-11-05**，来源是 developer.android.com 与 pub.dev 官方页。
- Flutter 默认值：本机 `flutter --version` = 3.44.9，直接读 SDK 源码所得。
- 参考项目：`refs/upstream/` 里的浅克隆快照，行号会随上游变化，引用时按函数名/文件搜，别按行号。
