# hiddify-core 的 C ABI 拆解（补拆 §10.2 第 2 项）

> 来源：`refs/upstream/hiddify-app/hiddify-core/`（branch `v3`，commit `1dd899b` 2026-01-30，2.4 MB）
> **注意**：这个目录原本是空的（submodule 用 `ssh://` 地址没初始化），2026-10-28 用
> `git clone --depth 1 --branch v3 https://github.com/hiddify/hiddify-core.git` 补下。
> 读完范围：`platform/desktop/{custom.go,cmd_interface.go,grpc_interface.go}` 全部、
> `Makefile` 全部构建目标、`../pubspec.yaml` 的 ffigen 段。
> 结论先行：**它和我们不是一条路**（Go + gomobile/gRPC），但**有 3 条值得抄**，
> 其中「头文件是构建产物、不进仓库」直接治我们一个已知漂移隐患（见 §6.1）。

---

## 1. 先说清它是什么：这**不是** Rust 项目

| 事实 | 证据 |
|---|---|
| 核心是 **Go**（`go.mod` / `go build`） | `hiddify-core/go.mod`、`Makefile:97` |
| 底层代理引擎是 sing-box（也是 Go） | `Makefile:46-51` 引 `github.com/sagernet/sing-box/experimental/libbox` |
| 桌面端用 **cgo `//export`** 导出 C 函数 | `platform/desktop/custom.go` 等处 |
| 移动端**完全不走 C ABI** | `Makefile:43-53` 用 `gomobile bind` 出 `.aar` / `.xcframework` |

所以「移动端桥接走 ffigen」这条结论的**前提要说清**：ffigen 只用在**桌面端**
（Linux / macOS / Windows）。Android 走的是 gomobile 生成的 Java 绑定，iOS 走 xcframework
的 ObjC 头。我们（Rust）走 ffigen 是**一回事**，因为我们只有一个 C ABI 要暴露，
不需要为 Java/ObjC 各生成一套 —— 这是我们比它简单的地方。

---

## 2. 头文件是**构建产物**，不在仓库里（⭐ 最重要的一条）

`hiddify-core/bin/` 里只有一个 `.gitkeep`，**没有任何 `.h`**。

```makefile
# Makefile:40-41（目标 headers —— 注意指向 desktop2，历史遗留）
headers:
	go build -buildmode=c-archive -o $(BINDIR)/ ./platform/desktop2

# Makefile:107-113（macos：c-shared + 把自动生成的头改名）
macos: prepare macos-amd64 macos-arm64
	lipo -create ... -output $(BINDIR)/$(LIBNAME).dylib
	cp $(BINDIR)/$(LIBNAME).dylib ./$(LIBNAME).dylib
	mv $(BINDIR)/$(LIBNAME)-arm64.h $(BINDIR)/desktop.h     # ← :111
```

`go build -buildmode=c-archive` / `c-shared` 会**顺带产出同名 `.h`**，
里面是 `extern` 声明的全部导出函数。然后一句 `mv` 改名叫 `desktop.h`。

**为什么这么干**：头文件永远不会和实现漂移 —— 因为它是从**实现**生成的。
反观我们：`ffi/include/yshjy.h`（43 行）是**手写的第二份真源**，
`ffi/src/lib.rs`（297 行，9 个 `extern "C"`）改了而头文件忘了改 → 只有等到 Dart 侧
ffigen 生成绑定报错、或者更糟（签名一致但语义变了）才会发现。

> 我们项目已经在这类「手抄副本」上栽过两次：man 页版本号（0.9.28 修）、
> 补全 18/35 项（0.9.27 修）。这块是同一种病，只是还没发作。

---

## 3. 导出面：11 个函数，全部 `*C.char` 进出

`grep -rn "^//export"` 的结果（`platform/desktop/`）：

| # | 函数 | 位置 | 签名要点 |
|---|---|---|---|
| 1 | `parseCli` | `cmd_interface.go:14` | `(argc C.int, argv **C.char) *C.char` —— 把 C 的 argv 转成 Go 切片 |
| 2 | `StartCoreGrpcServer` | `grpc_interface.go:6` | `(*C.char listenAddress) *C.char` |
| 3 | `cleanup` | `custom.go:55` | 空函数（信号处理全被注释掉了） |
| 4 | `setup` | `custom.go:75` | **7 个标量参数**：`(baseDir, workingDir, tempDir *C.char, mode C.int, listen, secret *C.char, statusPort C.longlong, debug bool) *C.char` |
| 5 | `freeString` | `custom.go:97` | `(str *C.char)` —— 配套的内存释放器 |
| 6 | `start` | `custom.go:104` | `(configPath *C.char, disableMemoryLimit bool) *C.char` |
| 7 | `stop` | `custom.go:117` | `() *C.char` |
| 8 | `restart` | `custom.go:126` | 同 `start` |
| 9 | `GetServerPublicKey` | `custom.go:139` | `() *C.char` |
| 10 | `AddGrpcClientPublicKey` | `custom.go:148` | `(*C.char) *C.char` |
| 11 | `closeGrpc` | `custom.go:159` | `(mode C.int)` —— 唯一不返回错误的 |

**约定：`*C.char` 返回值 = 错误消息；空串 `""` = 成功。** 全部由
`emptyOrErrorC` (`custom.go:62-73`) 统一生产：

```go
func emptyOrErrorC(err error) *C.char {
	if err == nil {
		return C.CString("")          // 成功：空串（调用方也要 free！）
	}
	log.Error(err.Error())
	str := C.CString(err.Error())
	defer C.free(unsafe.Pointer(str))  // ⚠️ 见 §4
	return str
}
```

---

## 4. ⚠️ 上游的两处疑点（**别抄**）

### 4.1 `emptyOrErrorC` 是 use-after-free（`custom.go:70-72`）

```go
str := C.CString(err.Error())
defer C.free(unsafe.Pointer(str))   // defer 在本函数返回前执行
return str                          // 返回的指针指向已释放内存
```

Go 的 `defer` 在函数返回**之前**执行。所以调用方拿到的是一个**悬垂指针**：
运气好读到垃圾字节，运气差崩溃。这是真缺陷，不是我们读错（同样形态在
`cmd_interface.go` 里没有，那段是直接 `return C.CString(...)`，是对的）。

**对我们的意义**：我们的 `yshjy_string_free` 是**调用方**释放，Rust 侧从不主动 free 返回值 ——
方向是对的，别在 Rust 里加「顺手 free 一下」的思路。

### 4.2 参数一多就是 7 个标量（`custom.go:75`）

`setup(baseDir, workingDir, tempDir, mode, listen, secret, statusPort, debug)` ——
加一个字段就要改 C 签名、Go 实现、Dart 绑定、所有调用点。

**我们的做法更好**：`yshjy_request(const char *json)` 一个字符串进，
新增字段只在 JSON 里加，**ABI 一行不用动**。这是我们的设计优势，写在这里免得以后有人
为了「性能」把手搓的标量签名引进来。

### 4.3 `cleanup` 是空函数（`custom.go:55-59`）

整个信号处理逻辑被注释掉（`custom.go:18-53` 大段注释）。说明他们把「进程退出时清理」
交给了 gRPC 关闭 + Flutter 侧主动 `stop()`。**不学**：我们的 `yshjy_job_free` 里
是「先取消、再等 worker 退出」，比「指望调用方记得调 stop」稳。

---

## 5. 桌面端**不用** C ABI 传状态，用 gRPC + 本地端口

`Makefile` 里 `StartCoreGrpcServer` + `GetServerPublicKey` + `AddGrpcClientPublicKey`
三件套的用途：核心起一个**本地 gRPC 服务**（带自签公钥认证），Flutter 侧用 gRPC 客户端连。

C ABI 只负责**生命周期**（setup / start / stop / restart / close），
**状态与进度全走 gRPC**。

我们的选择相反：`yshjy_event_cb` 函数指针 + JSON 事件（`ffi/include/yshjy.h:22-23`），
**进程内直接回调**。对比：

| 维度 | hiddify（gRPC） | 我们（事件回调） |
|---|---|---|
| 音频/视频级高频事件 | 可跨进程、可远程调试 | 进程内零拷贝，更省 |
| 依赖 | 要起端口（可能被防火墙/沙箱挡）、要管公钥 | 无 |
| 语言门槛 | 每种外壳都要 gRPC 客户端 | 一个函数指针 |
| 调试 | 能用 grpcurl 直接戳，**这是它的优点** | 要打日志 |
| 我们的场景 | 解压是短任务，事件只在解压期间有 | ✅ 回调更合适 |

**结论**：不换。但记一条它的优点 —— **如果将来要「GUI 崩溃不影响引擎」或「多语言外壳共用
一个常驻引擎进程」，gRPC 那条路是现成答案**，别从零设计。

---

## 6. 对照我们 `ffi/include/yshjy.h` + `ffi/src/lib.rs`

| 维度 | hiddify-core | 我们 | 判定 |
|---|---|---|---|
| 头文件来源 | **构建期生成**（`go build` 自动出 `.h`），不入仓 | 手写维护（43 行），与 `lib.rs` 9 个导出**两份真源** | ❌ 我们该改（见 §7 第 1 条） |
| 参数传递 | 7 个标量（setup） | 单个 JSON 字符串 | ✅ 我们对 |
| 错误表达 | 空串 = 成功，其余 = 错误文本 | `{"ok":false,"error":{...}}` 结构化 | ✅ 我们对（能带错误码/可读文案） |
| 回调用 user 指针 | 无回调 | `yshjy_event_cb(json, len, void *user)` | ✅ 我们对 |
| 内存释放 | `freeString`，**成功返回的空串也必须 free** | `yshjy_string_free`，同样规则 | 🟡 半斤八两，**但都容易漏** |
| ABI 版本 | 无版本函数 | `yshjy_abi_version() -> u32` | ✅ 我们对 |
| 句柄管理 | 无（全局单例） | `YshjyJob *` + cancel/wait/free | ✅ 我们对（能并发多任务） |
| 移动端 | `gomobile bind` 出 `.aar`/`.xcframework` | `ffi/` 的 C ABI → ffigen 生成 Dart 绑定 | ✅ 我们对（一套 ABI 全覆盖） |
| 静态库 | `c-archive`（Makefile `headers`） | `crate-type = ["staticlib"]`（iOS 要求） | ✅ 已对 |

**一句话**：ABI **形状**我们全面胜出（JSON + 事件 + 句柄 + 版本号都是更成熟的设计），
唯一输的是**头文件的产生方式**（它自动、我们手抄）。

---

## 7. 可抄的 3 条

### 7.1 🔴 头文件改成「从实现生成 + 探针兜底」

目标：`ffi/include/yshjy.h` 不再是手写第二份真源。

两条路，**推荐先做便宜的 B**：

**A. `cbindgen`（Rust 生态标准做法）**
```toml
# ffi/cbindgen.toml
language = "C"
include_guard = "YSHJY_H"
[export]
include = ["YshjyJob", "yshjy_event_cb"]
```
`cbindgen --config ffi/cbindgen.toml --crate yshjy-ffi -o ffi/include/yshjy.h`
代价：我们的头文件里有**中文注释 + 手写的契约说明**（`/* 约定：... */`），cbindgen 会全冲掉，
那些注释是我们最值钱的部分（写清了谁负责 free、回调不能阻塞）。→ **不建议全自动覆盖**。

**B. 探针（半小时）**
新增 `tests/ffi-header-sync.sh`：
1. 从 `ffi/src/lib.rs` 抓 `pub unsafe extern "C" fn (\w+)` 的函数名清单；
2. 从 `ffi/include/yshjy.h` 抓 `\b(yshjy_\w+)\s*\(` 的声明清单；
3. 两边 `sort` 后 `diff`，**不一致就红**，并打印「只在源码里 / 只在头文件里」两栏；
4. 顺手断言 `YSHJY_ABI_VERSION` 与 `yshjy_abi_version()` 返回值都是 `1`。

这样手写注释保住了，漂移也治住了。**与 `tests/cli_spec_match.sh` 同一个思路**（用探针守双向一致性）。

### 7.2 🟡 每个平台「一套产物、一个 make 目标」

`Makefile` 的结构值得照抄成文档：`android:` / `ios:` / `macos:` / `linux-custom:` 各一个目标，
每个目标产出**该平台原生的那一种**东西（`.aar` / `.xcframework` / `.dylib`+`.h` / 可执行文件），
不搞「一个命令出所有平台」。

对我们的意义：`docs/TODO.md` 那张多平台表里，「谁出包」一栏应该配一个这样的目标名，
而不是散在 `scripts/release.sh` 的分支里。**低优先，等多平台真开工时再定。**

### 7.3 🟢 `publish_to` 之外的 ffigen 配置就 6 行

`hiddify-app/pubspec.yaml:197-203`：

```yaml
ffigen:
  name: "HiddifyCoreNativeLibrary"
  description: "Bindings to Hiddify Core Library"
  output: "lib/gen/hiddify_core_generated_bindings.dart"
  headers:
    entry-points:
      - "hiddify-core/bin/desktop.h"
```

**我们照抄这 6 行**（改路径为 `ffi/include/yshjy.h`、输出 `gui/lib/gen/yshjy_bindings.dart`），
`generated_bindings.dart` **进 `.gitignore`**（它的惯例是构建产物）。等做 Android 那一步直接用。

---

## 8. 顺带记下的两个事实（省得以后再查）

1. **`headers` 目标指向 `desktop2`（不存在的目录）** —— `Makefile:40-41` 是坏的，
   真正能用的是 `macos-amd64/arm64` 那两条 `c-shared` 命令 + `mv`。别照抄 `headers`。
2. hiddify-core **没有** `#include <hiddify.h>` 这种总头文件，头文件按「构建时在哪个目录」命名
   （`libhiddify-core-arm64.h`），所以它才要 `mv` 改名。我们用固定路径 `ffi/include/yshjy.h`，
   本来就比它清楚。

---

## 9. 验收方式

1. `bash tests/ffi-header-sync.sh` → 绿（新探针，见 §7.1-B）。
2. `cd ffi && cargo build --release` 仍能出 `.so`（现有流程不破）。
3. 头文件里**中文契约注释一行不少**（`diff` 前先看行数：43 → 至少 43）。
