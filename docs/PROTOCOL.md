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
 "select": null, "force": false}

// 打包成伪装文件
{"id": 4, "cmd": "pack", "inputs": ["/home/me/我的游戏"],
 "out": "/home/me/我的游戏.mp4", "password": null, "tail": false}

// 取消当前任务（立即返回，真正停下要等引擎看到取消标志）
{"id": 5, "cmd": "cancel"}

// （config 命令留到 v1.1：先把主流程跑通，不做半成品接口）
```

## 响应（引擎 → 前端）

```jsonc
// 成功
{"id": 3, "ok": true, "result": { ... }}

// 失败：前端拿 code 查语言包，note 只用来写日志/反馈
{"id": 3, "ok": false, "error": {"code": "NOT_ARCHIVE", "note": "..."}}
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
| `JOB_FINISHED` | 任务结束 | 收尾：成功/部分成功/失败/取消 |

事件字段的权威定义在 `src/event.rs`（`pub enum Event`），**那是唯一真源**。
前端不要依赖事件的出现顺序（除了「`JOB_STARTED` 先于 `JOB_FINISHED`」这一条）。

## 结果结构（`result` 里有什么）

`hello`：
```json
{"protocol": 1, "engine": "0.9.0", "features": ["list", "extract", "pack", "cancel"],
 "event_kinds": ["JOB_STARTED", "..."], "languages": ["zh-CN", "..."]}
```

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
 "bytes_written": 20480, "dest": "/home/me/解压结果", "millis": 1234}
```
`outcome` 取值：`Success` / `Partial{...}` / `Cancelled{...}` / `Failed("CODE")`（原样序列化）。

`pack`：
```json
{"total_size": 2048, "zip_size": 120, "cover_size": 1688,
 "entries": 1, "layout": "FreeAtom", "warnings": []}
```

## 错误码

见 `src/error.rs` 的 `ErrorCode`（稳定字符串：`NOT_ARCHIVE` / `PASSWORD_REQUIRED` /
`PASSWORD_WRONG` / `IO` / `PERMISSION_DENIED` / `CANCELLED` / `INTERNAL` …）。
新增错误码是**兼容变更**；前端遇到不认识的码，用一句通用文案 + `note` 兜底显示即可。

## 一次完整会话长什么样

```
→ {"id":1,"cmd":"hello"}
← {"id":1,"ok":true,"result":{"protocol":1,"engine":"0.9.0", ...}}
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
| Tauri / Electron | Rust 侧 spawn `--serve` 子进程，或用 `ipc::Channel` 转发 |
| 脚本 / 网页 | `yshjy --serve < 命令.ndjson > 事件.ndjson`，或直接 `yshjy --json` 单次模式 |

## 什么时候该升版本

- 删字段、改字段含义、改事件顺序契约 → **必须** `protocol + 1`
- 加命令、加字段、加事件类型、加错误码 → 不用升版本（前端忽略即可）

> 一句话：**前端只认 `kind`、`code` 和字段名，不认实现**。
> 这条守住了，"后端更新前端不用大改"就不是口号。
