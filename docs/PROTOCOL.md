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

// 打包成普通归档（不加外壳）：format 只能是 zip / tar.gz / tar.zst，且必须 mode:"plain"
// （tar 系不支持 password / split / sfx / decoy，给了回 BAD_PARAM）
{"id": 11, "cmd": "pack", "inputs": ["/home/me/照片"], "out": "/home/me/照片.tar.zst",
 "mode": "plain", "format": "tar.zst", "level": 6}

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
| `extract` | `path`, `dest?`, `password?`, `force?`, `verify_only?`, `smart_dir?`, `cover?`, `extract?`, `drill?`, `ask?`, `select?`, `encoding?` | 解压单个目标；**`smart_dir`（默认 `true`）＝智能解压**：包里只有一个顶层目录 / 只有一个文件时不额外套层，一堆散文件才建同名文件夹（＝CLI 默认）；**传 `false` 就永远多套一层「包名」文件夹**（＝CLI `--no-smart-dir`）。⚠️ 只有**不给 `dest`** 时才有区别 —— 给了 `dest` 两种模式都直接产物铺在 `dest` 下（历史行为，别指望它替你建子目录）；**`verify_only:true` = 只测不写**（＝CLI `--test`、界面「只检查」）：完整解码 + 校验，**一个字节都不落盘**。开着它时前端要自己把 `cover` 关掉（切封面是另一条真写盘的路径），并且结果里的 `entries_done` / `bytes_written` 是**解码量、不是落盘量**、`dest` 只是回显（目录不会存在）—— 别拿它当产物路径去摆「打开目录」；`cover:false` 不提封面，`extract:false` 只提封面；`drill:true` 解完后自动再解**一层**内层包（结果带 `nested`）；`ask:true` 表示前端能处理 `ASK_USER`（不写 = 限额命中直接失败）；**`select` = 文件级挑选**（v0.9.24 起）：字符串表达式（`all` / `1,3,5` / `*.exe` / `目录/**` / `required`）或正整数数组（序号，与 `PREVIEW_READY` 的 `index` 一致），不传 = 全解，非法表达式回 `BAD_PARAM`（**不会**静默全解）；`encoding` = 文件名代码页（`gbk` / `big5` / `sjis` / `utf-8`，`auto` 等同不传） |
| `pack` | `inputs[]`, `out`, `cover?`, `sfx?`, `password?`, `tail?`, `layout?`, `second?`, `mode?`, `format?`, `split?`, `checksum?`, `readme?`, `level?`, `decoy?`, `decoy_size?`, `decoy_signatures?` | 打包；`mode:"plain"` = 普通 ZIP（无封面、无隐写，默认不写说明书），不传或 `"disguise"` = 伪装（老行为）；**`cover` = 外壳文件的绝对路径，伪装模式必填，任意类型**（引擎按文件头决定布局：MP4 走 free 原子，其他走尾部追加）；**`sfx:true` = 生成 Windows 自解压 exe**（输出必须 `.exe`，与 `cover`/`split`/`password` 互斥）；`split` 分卷字节数，`checksum` 默认 true，`readme` 默认跟随 mode；**`level` = 压缩级别 0-9**（0 = 仅存储最快、9 = 压缩最小，默认 6，结果里回显实际用的 `level`，越界回 `BAD_PARAM`）；**`format` = 归档格式（v0.9.40 起，T17-4）**：`zip`（默认）/ `tar.gz` / `tar.zst`，**只有 `mode:"plain"` 能用** —— 伪装包只能是 zip（封面 + tar 系的包定位不出来：解压侧靠 ZIP 的 EOCD 认，2026-10-31 三种都探针实测过，界面会拒）。tar 系**不支持** `password`（加密）/ `split`（分卷）/ `sfx`（自解压）/ `decoy`（干扰数据），给了回 `BAD_PARAM` 并说明原因，**绝不静默忽略**。结果里的 `format` 回显实际用的格式（界面据此显示「打成 tar.gz」）；普通压缩时 `layout` / `cover_size` 无意义（`zip_offset` 为 0），前端别拿它当封面包裹的证据；**`layout` = 数据藏在哪（v0.9.42 起 L4、v0.9.43 起 L3）**：`tail`（尾部追加）/ `free`（free 原子，默认）/ `head`（**放在文件头部**）/ `mid`（**插在视频数据中间**），也认实测报告里的编号 `L1` / `L2` / `L3` / `L4`；不传 = 老行为（引擎按封面文件头自己选）。`mid` 只有封面是 MP4 且能解析出 `stco`/`co64` 时才成立，**做不到会降级并在结果里说明**（图片外壳→尾部追加；MP4 但结构特殊→free 原子），绝不写出一个播不了的假视频；`mid` 与 `decoy`（干扰数据）**互斥**，一起给回 `BAD_PARAM`（噪声会把视频数据切断）。**`head`（L3，v0.9.43 起）**= ZIP 包成 `free` 原子插在 `ftyp` 之后，后面所有字节整体后移 —— 全表 `stco`/`co64` 都要 `+插入长度`，**`mdat.size` 一个字节都不改**；代价是 EOCD 离文件尾极远（实测 118,902 B），`unzip` / Windows 资源管理器 / 手机解压器**全都找不到**，只有 7-Zip（从文件头扫）和本工具能解，所以界面必须把这个代价写在选项说明里；`head` **允许**与 `decoy` 同用（噪声写在整份产物的末尾，不碰任何偏移），做不到时同样降级并说明；**`two`（L7，v0.9.43 起）**= 一个文件里放两个**独立**包：封面 → ZIP1（本次 `inputs` 压出来的）→ ZIP2（`pack.second` 指定的现成 zip），两个包各自完整、都能单独解开。配套字段 **`second`**（字符串，现成 ZIP 的绝对路径，**只有 `layout:"two"` 认它**）：给别的布局回 `BAD_PARAM`，`two` 不给它、或给的不是 ZIP（文件头不是 `PK\x03\x04`）也回 `BAD_PARAM` 且**零产物**（校验在写盘之前）；`two` 与 `decoy` 互斥（噪声接在 ZIP2 后面会让它的目录区不在文件尾，连 unzip 都看不见第二个包）。结果里多两个字段：`second_offset`（第二个包的起点 = `cover_size + zip_size`）与 `second_size`，别的布局是 `null`；`layout` 回 `"TwoPackages"`。**两个包都能解**（本工具多候选扫描），但通用解压器只会看到其中一个：7-Zip 报第一个、`unzip`/`zipfile` 报最后一个，都不提示还有另一个 —— 界面必须写清这个代价；`tail:true` 等于 `layout:"tail"`，两者一起给还说不同的话也回 `BAD_PARAM`（不造两套优先级）；**`mkv`（L8，v0.9.43 起）**= 产物是**真 MKV**：借系统 `ffmpeg` 把封面视频重封装（`-c copy`，不转码），ZIP 原文进 `Attachments/AttachedFile/FileData`（`mimetype=application/zip`）。结果里 `layout` 回 `"MkvAttachment"`。**引擎会自动把 `out` 的后缀改成 `.mkv`** —— 调用方必须拿结果里的路径去打开文件，别按自己传的 `out` 猜；`out` 写别的后缀（`.mp4` 等）回 `BAD_PARAM`（内容是 MKV 却叫 .mp4 是骗人）。**没装 ffmpeg → 自动降级**（MP4 封面→`free` 原子、其它→`tail`）并在结果里说明，不会写出假 MKV；`mkv` 与 `decoy` 互斥（噪声会写坏 EBML 结构）。代价：`unzip` / 资源管理器 / 手机解压器**全都找不到**（ZIP 的 EOCD 离文件尾约 89 KB），只有 7-Zip 和本工具能解 —— 界面必须写清；**自定义显示名（v0.9.41 起，T18-2）不需要新字段** —— 产物文件名就是 `out` 的最后一段，界面直接改 `out`（引擎按它的扩展名选外壳素材），另加一个 `name` 只会在「out 与 name 谁赢」上造两套优先级 |
| `cancel` | — | 取消当前任务（整批停） |
| `answer` | `value` | 回答 `ASK_USER`：`continue` / `skip` / `cancel`；返回 `{answered, value}` |
| `skip` | — | 批量里只跳过**当前这一项**，下一个继续；返回 `{skipped}` |
| `pause` | — | **队列暂停**（T18-4，v0.9.44 起）：软停 —— **不打断**正在解的那个文件，在**下一个条目解完**时停住（已解完的条目一条不丢、也不会重解）。挂起点在 `ctx.entry_done`（所有格式共用）与批量循环的任务之间。返回 `{paused, delivered}`：`delivered:false` = 当时没有任务在跑（前端该把按钮复位）。⚠️ **回执只代表「请求送到」**，真停下来时引擎发 `PAUSED` 事件 —— 前端必须按事件翻按钮，否则就是「点了没反应」 |
| `resume` | — | **队列继续**：从暂停处接着跑（不重解已完成的条目）；返回同 `pause`。暂停中被 `cancel` 也能立刻退出（条件变量会被唤醒） |
| `scan` | `paths[]` | 批量探测：`kind` / `volumes` / `looks_like` / `mime` / **`candidates[]`** / **`seek` / `seek_label`** |
| `batch` | `paths[]`, `dest?`, `password?`, `smart_dir?`, `verify_only?`, `ask?`, `select?`, `encoding?` | 顺序批量解压，返回逐项结果；`ask` / `select` / `encoding` / `verify_only` 与 `extract` 同规则（同一次批量里所有包都按这个挑选） |
| `report` | `path?`, `no_paths?`, `out_dir?` | 生成诊断 zip，返回落盘路径 |
| `rescue` | `path`, `dest?`, `list_only?`, `encoding?` | **救援**（＝CLI `--rescue`）：中央目录坏了也按 local header 重建清单并抢救数据。`list_only:true` 只重建清单、**一个字节都不落盘**（界面可以先说「能救回 N 个」再动手）；不给 `dest` 时落到 `<包名>.rescue`；只认 ZIP 家族，认不出回 `NOT_ARCHIVE`（**不许**折成 `DATA_LOST`，否则用户会去重下好包）。结果带 `entries_total` / `entries_done` / `dest` / `files[]` / `skipped[]` / `rebuilt[]` |
| `bulk` 以外的新命令 | — | **只加不删**，前端忽略不认识的 |

（最后一行示例：以后新增命令照这个原则 —— **只加不删**，老前端忽略即可。）

## 能力位（`hello.caps`）

布尔位如实反映「引擎真的做出来了」，前端据此决定显不显示某些功能。**只加不删**。

| 键 | 类型 | 含义 |
|---|---|---|
| `preview` | bool | 能只列内容（`list`） |
| `selective` | bool | 能只解一部分（`extract.select`） |
| `resume` | bool | 断点续传：中断后重开能接着解 |
| `resume_level` | string | 续传粒度。`"entry"` = **条目之间**（已完整解出的文件不再重解，正在写的那个重来）；将来做了条目内部块级续传会变成 `"block"`，中间的 `"none"` 表示没有。前端按字符串分支，别假设是固定枚举值 |
| `background` | bool | 关掉窗口后引擎还在后台跑（藏到托盘继续解） |
| `seek_probe` | bool | 能探测「这个位置能不能跳着读」（`scan` 结果里的 `seek`）。**探到 ≠ 落地做完**：真到 `copy_first` 时复制那份要在手机上验，桌面侧只有代码与单测 |

`resume_level` 这套三档命名借自 itchio/savior（`none` / `entry` / `block`）——
用业界已有的词，比我们自己造一套好沟通。

### 按格式的能力表（`hello.formats`，2026-10-29 新增）

`hello.caps` 是**引擎级**的能力（能不能预览 / 能不能续传），而「这个压缩格式能不能选条目、
能不能给密码、能不能反向打包」是**按格式**不同的 —— zip 能选条目、gzip 这类裸流不能；
7z 有头加密（连清单都加密）、zip 没有。所以 `hello.formats` 给一张表：

```jsonc
{"id":1,"cmd":"hello"} →
{"id":1,"ok":true,"result":{
  "caps": {"preview":true,"selective":true,"resume":true,"resume_level":"entry",
           "background":true,"seek_probe":true},
  "formats": [
    {"format":"zip","shape":"plain","encrypted":true,"encrypted_names":false,
     "split_volumes":true,"selective":true,"restores_mtime":true,"can_create":true},
    {"format":"gzip","shape":"single","encrypted":false,"encrypted_names":false,
     "split_volumes":false,"selective":false,"restores_mtime":false,"can_create":false}
    /* …共 12 条 */
  ]
}}
```

| 字段 | 含义 |
|---|---|
| `format` | 机器标识：`zip` / `7z` / `rar4` / `rar5` / `tar` / `gzip` / `xz` / `zstd` / `bzip2` / `brotli` / `lz4` |
| `shape` | 数据形状：`plain`（单条流）/ `volumes`（多卷已拼好）/ `tar`（目录树）/ `single`（裸流解出一个文件） |
| `encrypted` | 支持解密 |
| `encrypted_names` | 可能连**文件名**都加密（7z `-mhe=on`）—— 无密码时连清单都列不出 |
| `split_volumes` | 支持分卷 |
| `selective` | 支持只解一部分（`extract.select`） |
| `restores_mtime` | 解压能还原原文件的修改时间 |
| `can_create` | 能反向打包成这种格式 |

**展示名不归引擎**：`format` / `shape` 都是机器标识（小写、无空格），中文/日文标签由前端查自己的语言包拼。
**为什么不用 `--caps` 子进程**：iOS 禁止 fork/exec，手机端那条路物理上跑不了 —— 能力表必须能从协议拿到。
**兼容性**：只加字段，不删不改；老前端不认识 `formats` 就忽略，行为完全不变，所以协议版本不升。
数据源与 CLI `--caps --json` **同一份**（`unpack::能力行()`），`tests/caps_protocol.py` 盯着两边逐字段一致。

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
| `RESUMED` | 这次是**接着上次没解完的**往下解（断点续传） | 提示一句「接着上次的进度来（已完成 N 项）」；否则用户会以为「一上来就有一大截进度、前面文件没动」是 bug |
| `PAUSED` | 队列暂停**真的停住了**（`paused:true`）/ 已恢复（`false`） | `true` 时把按钮从「暂停」翻成「继续」，并显示一句「已暂停」；`false` 时清掉那句。★ 别拿 `pause` 命令的回执当这个用 —— 回执到达时当前文件还在解 |
| `DIAGNOSTIC` | 不打断的提醒 | 小提示条（如 `RISKY_ENTRY` = 解出可执行文件） |
| `ASK_USER` | 限额命中、需要用户决定 | 弹窗三选一（继续 / 跳过 / 取消），用 `answer` 回答；**60 秒不答按跳过** |
| `PACK_PROGRESS` | 打包中 | 打包页进度条（打几 GB 的包时界面不能是死的） |
| `FLUSH_PROGRESS` | **等真落盘**期间每 ~200 ms | 显示「还剩约 X MB 待写入 · 已等 N 秒 · 请勿拔出设备」。`pending` 是内核待写入字节数（2026-10-30 起按本次产物字节数封顶）；**`pending` 会一路降到 0**，拿它画进度条，别自己编动画 |
| `FLUSH_FINISHED` | 等落盘结束 | `outcome` = `DONE`（已写入设备，可以安全拔出）/ `CANCELLED`（用户点了停止等待）/ `UNSUPPORTED`（平台不支持，退回全量 `sync` 也不行）。`millis` 是**等待耗时**，与解压/打包本身的用时**分开报** |
| `JOB_FINISHED` | 任务结束 | 收尾：成功/部分成功/失败/取消 |

事件字段的权威定义在 `src/event.rs`（`pub enum Event`），**那是唯一真源**。

**`FLUSH_*` 只会在「`[safety] flush_movable` 开着」+「目标是移动设备」时出现**，
其余情况一条都不会有（本机硬盘上永远不多等一秒）。前端**不要**在没有 FLUSH 事件时
自己补一个"校验中"——那会变成假动画。开关关着但目标是移动设备时，引擎改发
诊断 `MOVABLE_NOT_VERIFIED`，前端请提示「拔盘前请先安全弹出」。
前端不要依赖事件的出现顺序（除了「`JOB_STARTED` 先于 `JOB_FINISHED`」这一条）。

## 结果结构（`result` 里有什么）

`hello`：
```json
{"protocol": 1, "engine": "0.9.45",
 "features": ["hello","cancel","report","scan","list","extract","pack","batch","answer","skip","pause","resume"],
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
 "entries": 1, "layout": "FreeAtom", "warnings": [],
 "noise_size": 0, "noise_segments": 0, "noise_segment_size": 16384,
 "noise_signatures": false}
```
`noise_size` = 实际撒进去的干扰数据字节数（没开 `decoy` 就是 0）；`noise_segments` /
`noise_segment_size` / `noise_signatures` 是**回显你传的参数**，前端拿它显示
「撒了 N 段，共 X」。`decoy` 与 `mode:"plain"`、`sfx` 互斥（回 `BAD_PARAM`，不静默忽略）。

`scan`（只读文件头，不解压）：
```json
{"given": 1, "expanded": 3,
 "items": [{"path": "/home/me/下载/游戏.mp4", "name": "游戏.mp4", "size": 12345,
            "kind": "Zip", "volumes": 1, "looks_like": "Zip",
            "mime": "application/zip", "archive": true,
            "candidates": [{"format": "zip", "offset": 3145728, "confidence": "high",
                            "note": "LFH+CD ok", "chosen": false},
                           {"format": "zip", "offset": 6291581, "confidence": "high",
                            "note": "located", "chosen": true}],
            "seek": "full", "seek_label": "可以直接读"}]}
```
`candidates`（S10 多候选）是**一个文件里所有像归档的位置**：
- `offset` / `format` 直接来自定位时的全文件扫描（`scan::ScanHit`），**不是额外扫一遍**；
- `confidence` 只有三档机器标识 `high` / `medium` / `low`，句子由界面查文案渲染
  （`low` 是「签名像、但结构校验没过」，界面上要标成「可能是误报」）；
- `chosen: true` 是引擎最终采用的那一处，**永远恰有一条**（正常包只有这一条）；
- **`len(candidates) == 1` 时界面不许显示任何选择界面** —— 绝大多数包都是这种，
  弹出来只会打扰人。只有 ≥2 才是「找到 N 处可能藏着文件的地方」。
- 非包（`archive: false`）→ `candidates` 是**空数组**，不是 `null`。
- ⚠️ 「像归档」≠「能解开」：`confidence` 是签名校验的结论，不是解压验证过的。
  界面文案必须写「可能」，并且「都试试」要真的逐个去试（试失败不许报成功）。
`seek` 是**机器标识**（`full` / `block_cache` / `copy_first`），前端按它分支：
`full` 什么都不用做；`block_cache` 表示只能顺序读（引擎会套 32 KiB × 8 MiB 的块缓存）；
`copy_first` 表示要先复制一份本地副本才能解，**该提示占多少空间**（`size` 字段就是那个数）。
`seek_label` 只是现成的中文标签，**别拿它当状态机**。
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

见 `src/error.rs` 的 `ErrorCode`（稳定字符串，**17 个，全表以那里为准**）：
`PATH_NOT_FOUND` / `INVALID_ARGUMENT` / `NOT_ENOUGH_SPACE` / `PERMISSION_DENIED` /
`PATH_TOO_LONG` / `DEST_IS_FILE` / `NEED_PASSWORD` / `WRONG_PASSWORD` / `HEAD_ENCRYPTED` /
`NOT_ARCHIVE` / `TRUNCATED` / `MISSING_VOLUME` / `DATA_LOST` / `ENCRYPTED_UNBREAKABLE` /
`VIDEO_UNPLAYABLE` / `CANCELLED` / `INTERNAL`。

> ⚠ **这份表以前写错过，别再凭印象补**：老版本节写的是 `PASSWORD_REQUIRED` /
> `PASSWORD_WRONG` / `IO` —— **三个码引擎里都不存在**。界面照着它写判断条件，
> 结果加密包在界面上永远不出现解压码输入框（2026-10-30 真机撞上并修掉）。
> 改码表一律对照 `ErrorCode::as_str()`；界面侧判断别手写码名，
> 走 `gui/lib/humanize.dart` 的 `codeNeedsPassword` / `codeCanRescue`（有单测）。

新增错误码是**兼容变更**；前端遇到不认识的码，用一句通用文案 + `note` 兜底显示即可。

## 一次完整会话长什么样

```
→ {"id":1,"cmd":"hello"}
← {"id":1,"ok":true,"result":{"protocol":1,"engine":"0.9.45", ...}}
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
