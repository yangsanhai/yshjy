# C ABI（iOS / Android / 其它外壳）

> 目标：让**不能起子进程**的平台（iOS 明令禁止 fork/exec）也能用同一个引擎。
> 本层只是传输：命令语义在 `src/api.rs`（与 `--serve` 完全同一份），这里不重复实现。

## 1. 构建

```bash
cd ~/hermes/yshjy/ffi
cargo build --release
# Linux:   target/release/libyshjy_ffi.so   + libyshjy_ffi.a
```

- **Android**：`cargo ndk -t arm64-v8a -o jniLibs build --release`（产物 `libyshjy_ffi.so`）
- **iOS**：`cargo build --release --target aarch64-apple-ios`，取 `.a` 进 Xcode；
  再 `xcodebuild -create-xcframework` 合并真机 + 模拟器两片
- **macOS / Windows**：同 `cargo build --release`，产物是 `.dylib` / `.dll`
- 头文件：`ffi/include/yshjy.h`

⚠️ 这一层**独立于根 crate**（自己的 `Cargo.toml`）：因为引擎本体 `#![forbid(unsafe_code)]`，
C ABI 必须碰裸指针，所以 unsafe 全部关在 `ffi/` 里，引擎的零 unsafe 承诺不被破坏。

## 2. 函数表（v1）

| 函数 | 作用 | 返回 |
|---|---|---|
| `yshjy_abi_version()` | ABI 版本，宿主启动先校验 | `1` |
| `yshjy_version()` | 引擎版本字符串 | `char*`（要 free） |
| `yshjy_hello()` | 协议版本 / 命令表 / 事件种类 / caps | JSON `char*` |
| `yshjy_request(json)` | 同步执行一条命令（阻塞） | 信封 JSON `char*` |
| `yshjy_job_submit(json, cb, user)` | 异步执行，事件走回调 | `YshjyJob*` |
| `yshjy_job_cancel(job)` | 请求取消（任意线程可调） | `0` 成功 |
| `yshjy_job_wait(job)` | 等结束取结果（可重复调） | 信封 JSON `char*` |
| `yshjy_job_free(job)` | 释放句柄（会先取消 + join） | — |
| `yshjy_string_free(s)` | 释放本库返回的字符串 | — |

信封与管道模式**逐字段一致**：

```json
{"id":1,"ok":true,"result":{"format":"zip","entries":[...]}}
{"id":1,"ok":false,"error":{"code":"NOT_ARCHIVE","note":"...","detail":{...}}}
```

事件回调收到的每条 JSON 也一样：

```json
{"type":"event","kind":"EXTRACT_PROGRESS","event":{...}}
```

## 3. 调用示例（C）

```c
#include "yshjy.h"

static void on_event(const char *json, size_t len, void *user) {
    (void)user;
    fwrite(json, 1, len, stdout);  /* 每行一条；len 不含结尾 0 */
    fputc('\n', stdout);
}

int main(void) {
    if (yshjy_abi_version() != YSHJY_ABI_VERSION) return 1;

    char *hello = yshjy_hello();
    puts(hello);
    yshjy_string_free(hello);

    YshjyJob *job = yshjy_job_submit(
        "{\"cmd\":\"extract\",\"path\":\"/tmp/游戏.mp4\",\"dest\":\"/tmp/out\"}",
        on_event, NULL);
    char *res = yshjy_job_wait(job);
    puts(res);
    yshjy_string_free(res);
    yshjy_job_free(job);
    return 0;
}
```

## 4. 线程与生命周期契约

1. **回调在 worker 线程上**触发，不在调用 `submit` 的线程上。宿主自己保证 `user` 指针
   在 `yshjy_job_free` 之前一直有效，并在回调里做线程同步。
2. **一次只有一条命令在跑**（与管道模式一致）；第二次 `submit` 会立刻返回 `BUSY` 错误。
   要排队请宿主自己排。
3. `yshjy_job_wait` 可以重复调用，第二次返回缓存的结果。
4. `yshjy_job_free` 未 wait 就调是安全的：会先 `cancel` 再 join，**但结果会丢**。
5. **绝不 unwind 穿过 FFI**：worker 用 `catch_unwind` 包住，panic 变成
   `{"code":"PANIC"}`，宿主进程不受影响。
6. 所有 `char*` 都是 `CString::into_raw` 出来的，必须且只能用 `yshjy_string_free` 释放。

## 5. 实测记录

2026-10-17，Linux x86_64，Python `ctypes` 直调 `libyshjy_ffi.so`
（组 D 时点；之后 `hello.features` 又加了 `answer` / `skip`，但它们在 C ABI 上会返回
`UNSUPPORTED` —— 见下一节）：

```
abi = 1
version = 0.9.20-c
hello.features = [hello, cancel, report, scan, list, extract, pack, batch]
list ok = True entries = 2
extract ok = True outcome = Success
events = 8 kinds = [ENTRY_FINISHED, EXTRACT_PROGRESS, STAGE_FINISHED, STAGE_STARTED]
files = [a/一.txt, b/二.txt]
cancel rc = 0，wait 返回合法 JSON 错误码 PATH_NOT_FOUND
```

## 6. 能力边界（v1 明确的「不做」）

- **没有交互式询问**：C ABI 里 `api::执行` 的 ask 参数固定为 `None`；
  限额命中按硬失败返回（`DATA_LOST` + `detail.reason`）。
  需要「问用户」请用 `--serve`（`ASK_USER` → `answer`）。
- **没有 `answer` / `skip`**：两者是 `--serve` 的状态机，C ABI 直接返回
  `UNSUPPORTED`；宿主想中止请调 `yshjy_job_cancel`。
- **一次只能跑一个任务**：第二次 `submit` 立刻返回 `BUSY`，排队由宿主负责。
