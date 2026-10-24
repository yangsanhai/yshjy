# 引擎 ↔ 界面 协议（v1）

> **这份文档是前端和后端的唯一接头处。**
> 只要这里的规则不变，引擎怎么升级、界面用 Flutter 还是 Tauri，都互不影响。

## 为什么要有这份东西

`docs/ARCHITECTURE.md` 定的原则是「一个引擎，三种外壳」。
7-Zip 的教训（`refs/tech/7zip-工程层.md` §6）是：它的几个壳之间**没有共享内核 API**，
靠命令行粘在一起，结果同一份「错误码 → 文案」映射写了 4 遍。
我们不走那条路 —— 引擎是进程内的库，外壳通过**这一份协议**驱动它。

## 兼容性承诺（改代码前先读这四条）

1. **只加不删**：命令、字段、事件类型可以新增；已发布的**不许删、不许改含义**。
2. **前端必须忽略不认识的字段和事件**（按 `kind` 分支，不认识的直接跳过）。
3. **引擎只给「码 + 参数」，永远不给人看的句子** —— 文案由前端查语言包。
4. 破坏性改动必须把 `protocol` 加 1，并在 CHANGELOG 里写清楚迁移方式。

## 传输

- 双向、**一行一个 JSON**（NDJSON），UTF-8。
- 前端 → 引擎：`stdin`
- 引擎 → 前端：`stdout`
- 启动：`yshjy --serve`
- 不开任何网络端口（桌面、手机、脚本都用同一套标准流；要远程自己套 SSH/管道）

## 命令（前端 → 引擎）

每条命令都必须带 `id`（整数，前端自己递增），响应会带回同一个 `id`。

```jsonc
// 握手：拿版本与能力
{"id": 1, "cmd": "hello"}

// 列内容（不解压）
{"id": 2, "cmd": "list", "path": "/home/me/游戏.mp4", "password": null}

// 解压（进度通过事件推回来）
{"id": 3, "cmd": "extract", "path": "/home/me/游戏.mp4",
 "dest": "/home/me/解压结果", "password": null,
 "select": null, "force": false,
 "cover": true, "extract": true}

// 只提取伪装封面（不解压内容）：界面里"只勾封面"就是这条
{"id": 9, "cmd": "extract", "path": "/home/me/游戏.mp4",
 "cover": true, "extract": false}

// 打包成伪装文件
{"id": 4, "cmd": "pack", "inputs": ["/home/me/我的游戏"],
 "out": "/home/me/我的游戏.mp4", "password": null, "tail": false}

// 取消当前任务（立即返回，真正停下要等引擎看到取消标志）
{"id": 5, "cmd": "cancel"}

// 扫一批路径（队列 / 多候选的数据源）：展开目录 → 归并分卷 → 只读文件头认类型
{"id": 6, "cmd": "scan", "paths": ["/home/me/下载", "/home/me/游戏.mp4"]}

// 批量解压（界面自己排队也用它；进度与单文件同一套事件）
{"id": 7, "cmd": "batch", "paths": ["/home/me/下载/"],
 "dest": "/home/me/解压结果", "password": null, "smart_dir": true}

// 生成诊断包（报障用；`no_paths: true` 脱敏路径）
{"id": 8, "cmd": "report", "path": "/home/me/游戏.mp4",
 "no_paths": false, "out_dir": "/home/me/桌面"}

// 回答引擎的询问（收到 ASK_USER 之后发；value 只能是三种）
{"id": 9, "cmd": "answer", "value": "continue"}   // continue / skip / cancel

// 批量里跳过当前这一项，继续下一个（不是整批取消）
{"id": 10, "cmd": "skip"}

// （config 命令留到 v1.1：先把主流程跑通，不做半成品接口）
```

## 命令能力表（`hello.features` 的权威来源）

| cmd | 参数 | 说明 |
|---|---|---|
| `hello` | — | 握手：协议版本、引擎版本、可用命令、事件种类、语言 |
| `list` | `path`, `password?` | 只列内容，不解压；有封面区时结果带 `cover`（大小/类型，不落盘） |
| `extract` | `path`, `dest?`, `password?`, `force?`, `cover?`, `extract?`, `drill?`, `ask?`, `select?`, `encoding?` | 解压单个目标；`cover:false` 不提封面，`extract:false` 只提封面；`drill:true` 解完后自动再解**一层**内层包（结果带 `nested`）；`ask:true` 表示前端能处理 `ASK_USER`（不写 = 限额命中直接失败）；**`select` = 文件级挑选**（v0.9.24 起）：字符串表达式（`all` / `1,3,5` / `*.exe` / `目录/**` / `required`）或正整数数组（序号，与 `PREVIEW_READY` 的 `index` 一致），不传 = 全解，非法表达式回 `BAD_PARAM`（**不会**静默全解）；`encoding` = 文件名代码页（`gbk` / `big5` / `sjis` / `utf-8`，`auto` 等同不传） |
| `pack` | `inputs[]`, `out`, `cover?`, `sfx?`, `password?`, `tail?`, `mode?`, `split?`, `checksum?`, `readme?`, `level?` | 打包；`mode:"plain"` = 普通 ZIP（无封面、无隐写，默认不写说明书），不传或 `"disguise"` = 伪装（老行为）；**`cover` = 外壳文件的绝对路径，伪装模式必填，任意类型**（引擎按文件头决定布局：MP4 走 free 原子，其他走尾部追加）；**`sfx:true` = 生成 Windows 自解压 exe**（输出必须 `.exe`，与 `cover`/`split`/`password` 互斥）；`split` 分卷字节数，`checksum` 默认 true，`readme` 默认跟随 mode；**`level` = 压缩级别 0-9**（0 = 仅存储最快、9 = 压缩最小，默认 6，结果里回显实际用的 `level`，越界回 `BAD_PARAM`） |
| `cancel` | — | 取消当前任务（整批停） |
| `answer` | `value` | 回答 `ASK_USER`：`continue` / `skip` / `cancel`；返回 `{answered, value}` |
| `skip` | — | 批量里只跳过**当前这一项**，下一个继续；返回 `{skipped}` |
| `scan` | `paths[]` | 批量探测：`kind` / `volumes` / `looks_like` / `mime` |
| `batch` | `paths[]`, `dest?`, `password?`, `smart_dir?`, `ask?`, `select?`, `encoding?` | 顺序批量解压，返回逐项结果；`ask` / `select` / `encoding` 与 `extract` 同规则（同一次批量里所有包都按这个挑选） |
| `report` | `path?`, `no_paths?`, `out_dir?` | 生成诊断 zip，返回落盘路径 |
| `bulk` 以外的新命令 | — | **只加不删**，前端忽略不认识的 |

（最后一行示例：以后新增命令照这个原则 —— **只加不删**，老前端忽略即可。）

## 响应（引擎 → 前端）

```jsonc
// 成功
{"id": 3, "ok": true, "result": { ... }}

// 失败：前端拿 code 查语言包，note 只用来写日志/反馈
{"id": 3, "ok": false, "error": {"code": "NOT_ARCHIVE", "note": "...", "detail": {...}}}
```

## 事件（引擎 → 前端，主动推送，没有 id）

```jsonc
{"type": "event", "kind": "EXTRACT_PROGRESS", "event": {"ExtractProgress": {...}}}
```

`kind` 是**稳定的机器标识**（前端按它分支）；`event` 里的结构见下方「事件字段」。

| kind | 什么时候发 | 前端该做什么 |
|---|---|---|
| `JOB_STARTED` | 任务开始 | 进入"处理中"状态 |
| `STAGE_STARTED` / `STAGE_FINISHED` | 定位 / 列清单 / 解压 各阶段 | 切换阶段文案；`millis` 可用于诊断 |
| `SCAN_PROGRESS` | 扫大文件时 | 进度条（可能没有总量） |
| `PREVIEW_READY` | 列清单完成 | 渲染列表（S3）；`entries` 里的 `index` 就是 `--select` 用的序号 |
| `EXTRACT_PROGRESS` | 解压中（≤10 次/秒） | 进度条 + 当前文件名 |
| `ENTRY_FINISHED` | 单条目完成 | 列表里打勾/标红 |
| `PASSWORD_NEEDED` | 需要密码 | 弹密码框（S4） |
| `SPACE_SHORTAGE` | 空间可能不够 | 提示（S5） |
| `DIAGNOSTIC` | 不打断的提醒 | 小提示条（如 `RISKY_ENTRY` = 解出可执行文件） |
| `ASK_USER` | 限额命中、需要用户决定 | 弹窗三选一（继续 / 跳过 / 取消），用 `answer` 回答；**60 秒不答按跳过** |
| `JOB_FINISHED` | 任务结束 | 收尾：成功/部分成功/失败/取消 |

事件字段的权威定义在 `src/event.rs`（`pub enum Event`），**那是唯一真源**。
前端不要依赖事件的出现顺序（除了「`JOB_STARTED` 先于 `JOB_FINISHED`」这一条）。

## 结果结构（`result` 里有什么）

`hello`：
```json
{"protocol": 1, "engine": "0.9.24-c",
 "features": ["hello","cancel","report","scan","list","extract","pack","batch","answer","skip"],
 "event_kinds": ["JOB_STARTED", "STAGE_STARTED", "STAGE_FINISHED", "...", "ASK_USER"],
 "languages": ["zh-CN", "..."]}
```
（`event_kinds` 的完整真值在 `src/api.rs` 的 `事件种类` 常量 —— 以代码为准。）

`list`：
```json
{"format": "zip", "complete": true, "total_size": 1234, "encrypted": 0,
 "entries": [{"index": 1, "path": "a.txt", "size": 5, "packed": 4,
              "is_dir": false, "encrypted": false, "is_risky": false,
              "modified": 1700000000}],
 "hints": ["NEEDS_PASSWORD"]}
```

`extract`：
```json
{"outcome": "Success", "entries_done": 4, "entries_total": 4,
 "bytes_written": 20480, "dest": "/home/me/解压结果", "millis": 1234,
 "cover": {"path": "/home/me/游戏.封面.mp4", "size": 125829120, "kind": "video"},
 "nested": [{"path": "/home/me/解压结果/示例包.7z.001", "dest": ".../示例包", "outcome": "ok"}],
 "note": null}
```
`outcome` 取值：`Success` / `Partial{...}` / `Cancelled{...}` / `Failed("CODE")`（原样序列化）。
**取消/跳过**走 `Cancelled` 且 `ok:true`（不是失败）；`cover` / `nested` 只在真产生时出现。

`pack`：
```json
{"total_size": 2048, "zip_size": 120, "cover_size": 1688,
 "entries": 1, "layout": "FreeAtom", "warnings": []}
```

`scan`（只读文件头，不解压）：
```json
{"given": 1, "expanded": 3,
 "items": [{"path": "/home/me/下载/游戏.mp4", "name": "游戏.mp4", "size": 12345,
            "kind": "Zip", "volumes": 1, "looks_like": "Zip",
            "mime": "application/zip", "archive": true}]}
```
`archive: false` 表示识别不出压缩格式（`kind == "UNKNOWN"`），界面上应显示成
「认不出，但可以拖进去试」，不要直接说失败。

`batch`：
```json
{"total": 3, "ok": 2, "partial": 0, "failed": 1, "cancelled": 0,
 "bytes": 20480, "elapsed_ms": 1234,
 "jobs": [{"id": 1, "path": "/home/me/a.mp4", "outcome": "ok",
           "bytes": 1024, "entries": 3, "elapsed_ms": 400, "dest": "/home/me/a"}]}
```
⚠️ `jobs[].outcome` 目前是**裸标识**：`ok` / `partial` / `cancelled` / 错误码的
Debug 名（如 `CrcError`）。前端**必须**按 `outcome` 走错误码映射（`_errText`），
不能直接 `toLowerCase()` 显示给用户（GUI-PLAN 组 4 待修）。

`report`：
```json
{"path": "/home/me/桌面/yshjy-report-20261017.zip"}
```

## 错误里的 `detail`（前端拼人话用）

失败时 `error.detail` 会带一组**结构化参数**。有了它，界面才能说
「你给的是个 MP4 视频」，而不是干巴巴一句「出错」：

```jsonc
{"id": 3, "ok": false, "error": {
  "code": "NOT_ARCHIVE",
  "note": "…技术备注，写给日志/开发者…",
  "detail": {
    "looks_like": "mp4",        // 引擎探测到的真实类型
    "looks_kind": "media",      // archive / media / document / program / unknown
    "ext": "mp4",               // 扩展名（小写，可能没有这个键）
    "ext_kind": "media",        // 扩展名"暗示"的类别
    "ext_mismatch": "false",    // "true" = **名字和内容不符**（伪装包的典型特征）
    "mime": "video/mp4",
    "size": "1688",             // 字节（字符串，避免大数精度问题）
    "brand": "isom"             // 类型特有的细节（MP4 的 brand、MKV 的 doctype…）
  }
}}
```

**为什么用字符串而不是数字/布尔**：`looks_like` 以后可能加值，`size` 可能超过
JS 安全整数，统一成字符串最省心；前端按需转换（`int.tryParse`）。

`ext_mismatch == "true"` 是**最该高亮的信号**：
名字写 `.mp4`、内容其实是 ZIP —— 这正是伪装包的样子，界面应当明说。

## 错误码

见 `src/error.rs` 的 `ErrorCode`（稳定字符串：`NOT_ARCHIVE` / `PASSWORD_REQUIRED` /
`PASSWORD_WRONG` / `IO` / `PERMISSION_DENIED` / `CANCELLED` / `INTERNAL` …）。
新增错误码是**兼容变更**；前端遇到不认识的码，用一句通用文案 + `note` 兜底显示即可。

## 一次完整会话长什么样

```
→ {"id":1,"cmd":"hello"}
← {"id":1,"ok":true,"result":{"protocol":1,"engine":"0.0.19", ...}}
→ {"id":2,"cmd":"list","path":"/home/me/游戏.mp4"}
← {"type":"event","kind":"STAGE_STARTED","event":{"StageStarted":{"job":2,"stage":"Locate"}}}
← {"type":"event","kind":"PREVIEW_READY","event":{"PreviewReady":{...}}}
← {"id":2,"ok":true,"result":{"format":"zip","complete":true,"entries":[...]}}
→ {"id":3,"cmd":"extract","path":"/home/me/游戏.mp4","dest":"/home/me/out"}
← {"type":"event","kind":"EXTRACT_PROGRESS","event":{...}}
← {"id":3,"ok":true,"result":{"outcome":"Success","entries_done":4,...}}
```

## 前端接入方式（三选一，都不用改引擎）

| 前端 | 怎么接 |
|---|---|
| Flutter（桌面 / Android） | `Process.start('yshjy', ['--serve'])`，按行读写；Android 端由平台通道调用同协议的本地实现 |
| Flutter（iOS） | **不能起子进程**：走 `FfiTransport`（`EngineTransport` 的另一种实现）+ C ABI，见 `docs/FFI.md` |
| Tauri / Electron | Rust 侧 spawn `--serve` 子进程，或用 `ipc::Channel` 转发 |
| 脚本 / 网页 | `yshjy --serve < 命令.ndjson > 事件.ndjson`，或直接 `yshjy --json` 单次模式 |

命令语义只有一份实现：`src/api.rs`。`--serve`（管道）和 C ABI（`ffi/`）都调它，
改命令只改那一处 —— 两个传输不会各走各的。

## 什么时候该升版本

- 删字段、改字段含义、改事件顺序契约 → **必须** `protocol + 1`
- 加命令、加字段、加事件类型、加错误码 → 不用升版本（前端忽略即可）

> 一句话：**前端只认 `kind`、`code` 和字段名，不认实现**。
> 这条守住了，"后端更新前端不用大改"就不是口号。
