# yshjy 内部全景（代码地图 · 2026-10-28 实测生成）

> **这份文档跟其它文档的分工**（别混）：
> - `DESIGN.md` = **产品**要什么样（用户视角、功能清单、拍板记录）—— 2026-10-16 定稿，仍有效
> - `docs/ARCHITECTURE.md` = **2026-10-16 写代码前**的架构设计草案 —— **部分已过时**（见 §9）
> - **本文件 = 代码现在长什么样**（模块/机制/落点/探针），从真实源码扫描出来，不是手写计划
> - `docs/TODO.md` = 唯一待办真源 · `HANDOFF.md` = 每轮状态与证据 · `CHANGELOG.md` = 踩坑档案
>
> 生成方式：`python3 scripts/scan-src.py` 扫 `src/` 得出模块表与依赖矩阵；
> `--check` 只打印关键计数（行数/事件数/错误码数）。脚本随代码走，见 §10。**代码变了就重跑一遍，别手改数字。**

---

## 1. 一句话与主干

**一句话**：用户双击一个「其实是压缩包的 mp4」→ 认出它、定位包体、解开、把原视频也还给他。

主干路径（精确到函数，可顺着点进去读）：

```
双击 / CLI 调用
   │
   ├─ GUI 走 serve 协议：gui → ProcessTransport → yshjy --serve → serve.rs → api.rs::dispatch
   └─ CLI 走 main.rs::main → job::run_job
                                   │
   ①识别  job::identify(path, ctx)  ──→  Target::{Zip, SevenZ, Rar, Stream, Tar, …}
   │        ├─ probe.rs  文件类型探测（魔数/特征）
   │        ├─ scan.rs   全文件签名扫描（包藏在文件中间时）
   │        ├─ layout/   ZIP/7z 结构解析与偏移反推
   │        └─ rescue.rs 目录表坏了 → 重建文件列表
  ②定位  unpack::Source::with_offset(base, len)   把「文件里的某一段」变成逻辑流
  ③选路  unpack::find(fmt, shape) → &'static dyn InArchive
  ④解码  InArchive::unpack(...)  5 个实现之一
  ⑤落盘  io::atomic  原子写入（先 .part 再改名）+ restore 还原 mtime/权限
  ⑥收尾  report / trash / cover_extract（把原视频也交出来）
  ⑦事件  event/*  30 种事件实时吐给外壳，外壳只负责画
```

**这条链上最关键的一句设计**：`extract.rs` 的模块注释写着「把**定位**与**解码**彻底解耦」。
所以「包藏在视频第 3 MB 处」「包被视频切成两段」「分卷散落各处」这三个变态场景，
解码器一行都不用改 —— 只换 `Source` 的构造方式。

---

## 2. 五层架构

```
┌─ L3 外壳 ────────────────────────────────────────────────┐
│  main.rs  CLI              gui/lib  Flutter 界面         │
│  serve.rs 协议             ffi/     C ABI（iOS/Android） │
└───────────────┬──────────────────────────────────────────┘
                │ JSON 请求 / JSON 事件
┌─ L2 编排 ────┴───────────────────────────────────────────┐
│  api.rs   一次请求→一次结果（界面走这条）                │
│  job.rs   单任务全流程（CLI 也走这条）                   │
│  batch.rs 批量队列     drill.rs 嵌套下钻                 │
└───────────────┬──────────────────────────────────────────┘
                │
┌─ L1 能力 ────┴───────────────────────────────────────────┐
│  识别：scan probe rescue repair                          │
│  解码：unpack extract extract_stream                     │
│        extract7z extract_rar layout/zip                  │
│  打包：pack pack/cover pack/decoy pack_post              │
│  辅助：preview select resume restore seek                │
│        password_hint cover_extract trash                 │
└───────────────┬──────────────────────────────────────────┘
                │
┌─ L0 地基 ────┴───────────────────────────────────────────┐
│  error（24 个模块依赖）· ctx（10）· event（7）           │
│  progress（6）· safety（6）                              │
│  io/：window segment atomic interrupt patch blockcache   │
│  config i18n paths hardware tools cli_spec               │
└──────────────────────────────────────────────────────────┘
```

> 图上只画**层级与模块归属**；行数、文件数、被依赖次数一律看 §3（`python3 scripts/scan-src.py` 产出）。
> 图上写死数字必然漂 —— 这条已经栽过两次（man 页版本号、补全 18/35 项）。

**硬约束**（`src/lib.rs` 顶部原文）：
1. 不依赖任何 UI 框架 —— CLI / 桌面 / 手机同一套引擎
2. **引擎不产出面向用户的中文文案** —— 只给「错误码 + 结构化参数」，渲染交给外壳
3. 所有 I/O 走 trait 抽象 —— 便于测试，也便于手机端（SAF 不支持随机定位）
4. `#![forbid(unsafe_code)]` —— **unsafe 只允许在 `ffi/` 那个独立 crate 里出现**

---

## 3. 模块全表（52 个文件 / 29,584 行）

> 「被依赖」= 有多少个模块 `use crate::它`（数字是实测的，越大越底层）。

### L0 地基（改动风险最高，牵一发动全身）

| 模块 | 行 | 被依赖 | 职责 | 关键点 |
|---|---|---|---|---|
| `error.rs` | 754 | **24** | 错误码体系（三层结果模型） | 17 个 `ErrorCode`；引擎只说码不说人话 |
| `ctx.rs` | 416 | **10** | 解压上下文：事件/进度/取消三件套 | 所有解压入口都要它，取消从这一路下去 |
| `event.rs` | 685 | 7 | 引擎事件流（引擎→外壳） | **30 种事件**（见 §5） |
| `progress.rs` | 658 | 6 | 进度、取消、剩余时间估算 | `EtaEstimator` α=0.3、样本<3 不给估算；CLI 与 GUI **必须共用** |
| `safety.rs` | 516 | 6 | 安全护栏（路径穿越/空间/内存） | 内置不可绕过；限额超了走 `ask.rs` 问用户 |
| `io/atomic.rs` | 347 | — | 落盘原子写入守卫（`.part` → rename） | 所有写盘都过它 |
| `io/window.rs` | 138 | — | 偏移窗口：把 `[start,start+len)` 映射成 `0..len` | 「包在文件中间」的基石 |
| `io/segment.rs` | 254 | — | 分段虚接：多个分散段拼成连续流 | E2/E3 场景靠它 |
| `io/interrupt.rs` | 121 | — | 可中断读取 | 让「取消」立刻生效，不用等读完 |
| `io/patch.rs` | 218 | — | 补丁窗口：读时就地改字节，**绝不改磁盘** | 修坏头部不改原文件 |
| `io/blockcache.rs` | 363 | — | 块缓存：给「不能随便 seek」的源套上 `Read + Seek` | 32 KiB 块 / 8 MiB LRU；回退到已淘汰块**明确报错，不装成 EOF**（第三期 C） |
| `config.rs` | 703 | — | 配置与密码本 | 本机状态目录，跨平台路径见 `paths.rs` |
| `i18n.rs` | 587 | — | 7 语言文案 | 引擎只给码，文案在这一层 |
| `paths.rs` | 103 | — | 跨平台状态目录 | |
| `hardware.rs` | 369 | — | 硬件与资源档位 | 并发度/内存估算 |
| `cli_spec.rs` | 303 | — | **CLI 选项唯一真源**（43 个长选项） | `--help`/补全/man 全从它推导 |
| `tools.rs` | 138 | — | 外部工具检测（**可选增强，不是依赖**） | 有 rar 更好，没也能跑 |

### L1 能力

| 模块 | 行 | 被依赖 | 职责 |
|---|---|---|---|
| `unpack.rs` | 1084 | 2 | **统一解压器接口** —— 「加一个格式只改一处」的地基（§4.2） |
| `extract.rs` | 1357 | 2 | ZIP 解码；定位与解码彻底解耦 |
| `extract_stream.rs` | 1123 | 2 | 单流压缩（gzip/xz/zstd/bzip2/lz4/brotli）+ TAR |
| `extract_rar.rs` | 1009 | 1 | RAR（含加密、含分卷） |
| `extract7z.rs` | 778 | 1 | 7z（含分卷虚接） |
| `layout/zip.rs` | 840 | — | ZIP/ZIP64 结构解析与**偏移反推**；EOCD 分块倒扫 |
| `layout/mod.rs` | 92 | 6(被) | 结构解析入口 |
| `scan.rs` | 749 | — | 全文件签名扫描（`FindPK_4` 式强判定） |
| `probe.rs` | 499 | — | 文件类型探测（「你给我的是啥文件」） |
| `rescue.rs` | 1080 | — | 救援模式：目录表坏了也能重建文件列表 |
| `repair.rs` | 339 | — | 偏移修复（坏包多数只是头部几字节写错） |
| `preview.rs` | 768 | — | 预览：**不解压**列出包里有什么（用了 `job`，见 §9 偏差1） |
| `select.rs` | 350 | 6(被) | 只解用户勾的那部分 |
| `resume.rs` | 786 | — | 断点续传（条目级 + JSONL 记录 + 三重校验） |
| `restore.rs` | 261 | — | 还原 mtime / 权限 |
| `password_hint.rs` | 539 | — | 从旁边「说明.txt」自动抓解压码 |
| `cover_extract.rs` | 350 | — | 封面分离：把原视频也交出来 |
| `trash.rs` | 187 | — | XDG 回收站（**永不 rm**） |
| `seek.rs` | 266 | — | 「这个位置能不能跳着读」—— 移动端 seek 三档探测（第三期 C） |
| `pack/mod.rs` | 1249 | — | 伪装打包：把包藏进视频/图片 |
| `pack/cover.rs` | 170 | — | 内置封面素材 |
| `pack/decoy.rs` | 316 | — | 干扰数据 / 假签名（第三期 B）—— 噪声只写在压缩数据之外；「签名 + 30 字节零」别改回纯随机（会骗坏 7-Zip） |
| `pack_post.rs` | 261 | — | 打包后处理：分卷/校验和/自动说明书 |

### L2 编排

| 模块 | 行 | 被依赖 | 职责 |
|---|---|---|---|
| `api.rs` | 1045 | — | 引擎 API 层：一次请求→一次结果（**界面走这条**） |
| `job.rs` | 1556 | — | 单任务全流程（**CLI 也走这条**，与 api 是两条路，见 §4.6） |
| `batch.rs` | 822 | 3(被) | 批量队列，一个坏了不影响其余 |
| `drill.rs` | 206 | — | 嵌套包自动下钻（只下一层） |
| `ask.rs` | 140 | 1(被) | 双向询问：引擎问一句、用户答一句 |
| `report.rs` | 503 | — | 诊断包（`--report`） |
| `crash.rs` | 463 | — | 崩溃兜底，留下能报障的现场 |

### L3 外壳

| 模块 | 行 | 职责 |
|---|---|---|
| `main.rs` | 3045 | CLI：选项解析 / 进度渲染 / `--pick` / `--list` / 一切终端 UX |
| `serve.rs` | 379 | `--serve`：一行一个 JSON 的双向管道（GUI 靠它） |
| `pick.rs` | 167 | `--pick` 全屏勾选（dialoguer）；Ctrl-C 光标复原已修，中断接管全进程只装一次 |
| `lib.rs` | 58 | 模块出口 + 4 条硬约束 |
| `gui/lib` | 5732 | Flutter：`home_page`(2371) `pack_page`(676) `engine`(534) `settings`(529) `app_shell`(372) `entry_tree`(335) + 托盘/通知/历史/主题 |
| `ffi/` | 297+43 | C ABI：JSON 进 / 事件出 / Job 句柄；`yshjy.h` 手写（漂移风险见 TODO 表尾） |

---

## 4. 七个核心机制（每个都有：为什么 / 怎么实现 / 落点 / 探针）

### 4.1 识别三件套：扫描 → 结构校验 → 救援

**为什么**：用户的主场景是「扩展名撒谎」，所以不能信扩展名，只能信内容。

| 层 | 做法 | 落点 |
|---|---|---|
| ①廉价探测 | 读头部魔数，几分钱 | `probe.rs` |
| ②全文件扫描 | 找 `PK\x03\x04` / `7z¼¯'` / `Rar!` 等签名，**带结构校验**防假签名 | `scan.rs` |
| ③结构解析 + 偏移反推 | 把 CD/EOCD 读出来，反推「这个包在文件里的真实起点」 | `layout/zip.rs` |
| ④救援 | 目录表彻底坏 → 逐个扫 local header 重建条目表 | `rescue.rs` |
| ⑤修复 | 头部几字节错 → `io/patch.rs` 读时就地改，**磁盘不动** | `repair.rs` |

**探针**：`tests/extreme_tests.py`（E1–E6 六种极端布局）、`tests/protocol_rescue.py`（22 项）。

### 4.2 统一解压器：加一个格式只改一处

`src/unpack.rs` 的定义（真实签名）：

```rust
pub trait ReadSeek: Read + Seek {}
pub trait InArchive: Sync {
    fn formats(&self) -> &'static [ArchiveFormat];   // 认哪些格式
    fn shapes(&self) -> &'static [Shape];            // 认哪些布局
    fn caps(&self) -> Caps;                          // 能力位
    fn unpack(...);                                  // 解压
    fn list(...);                                    // 列表（预览用）
}
pub fn find(fmt: ArchiveFormat, shape: Shape) -> Option<&'static dyn InArchive>;
```

**5 个实现**（都是零状态 ZST，`find` 返回 `&'static`，没有堆分配）：

| 实现 | 覆盖 | 特殊之处 |
|---|---|---|
| `ZipUnpacker` | zip/zip64 | 偏移反推 + 救援 |
| `SevenZUnpacker` | 7z | 分卷虚接；头加密 vs 内容加密 |
| `RarUnpacker` | rar/rar5 | 库不给条目大小/CRC，原子写只能靠 Drop 改名（见 CHANGELOG 第12项） |
| `SingleStreamUnpacker` | gzip/xz/zstd/bzip2/lz4/brotli | 解出**一个**文件 |
| `TarStreamUnpacker` | tar 家族 | 解出**目录树** |

**两个枚举是这套设计的骨架**：

```rust
pub enum Shape { Plain, Volumes, Single, Tar }   // 布局形态
pub struct Caps {                                 // 能力位（对应用户能看到的开关）
    encrypted, encrypted_names, split_volumes,
    selective, restores_mtime, can_create,
}
```

**`Source` 是「定位」的抽象**（`unpack.rs:116`）：
`Source::new(reader)` / `.with_offset(base,len)` / `.with_volumes(&[PathBuf])` / `.with_packed_total(n)`。
变态场景全靠它，解码器无感。

### 4.3 原子写 + 断点续传

**为什么**：解压被打断绝不能留半个坏文件（用户会以为解好了）。

- **原子写**（`io/atomic.rs`）：先写 `xxx.yshjy-part`，成功后 `rename` 成正式名。
  中断 → 只剩 `.part`，正式名一个都不出现。**探针**：`tests/rar_atomic.sh`（SIGKILL 打断）。
- **续传**（`resume.rs`）：条目级记录（JSONL，**一条一次 write 不攒批不 fsync** —— 攒批被 kill -9 会一条不落）、
  三重校验（包没变 / 文件还在 / 大小对得上）、新事件 `RESUMED`。**探针**：`tests/resume.sh`（15 项，
  含**随机 kill 点 12/12 字节一致**）。
- **踩过的坑**（别犯，见 CHANGELOG）：跳过检查必须在同名策略**之前**；
  「改名成功→记录没写」有极窄临界区，修法是「续传会话里没记录却已存在 → 强制覆盖重解」。

### 4.4 事件与进度：引擎只管吐，外壳只管画

- 事件流（`event.rs`）：30 种事件名，JSON 出；GUI 与 CLI 都消费同一份。
- 进度（`progress.rs`）：**`EtaEstimator` 是唯一实现**（α=0.3 平滑、样本<3 不估算）。
  约束：CLI 与界面**必须显示同一个剩余时间** —— 所以 CLI 用 indicatif 时也只 `set_position`，
  ETA 自己塞进 `{msg}`（`docs/REFERENCE-INDEX.md:139-140`）。
- 双维度：`SetTotal(files, bytes)`（7-Zip 学来的），窄终端按「文件名→速度/剩余→项号」顺序丢字段。

### 4.5 伪装打包

- 布局（`pack/mod.rs`）：尾部拼接 / free 原子布局（16 样本实测矩阵见 DESIGN §15）
- 封面（`pack/cover.rs` + `assets/`）：内置素材，改 MP4 atom 保证封面可播
- 说明书 + 校验和 + 分卷（`pack_post.rs`）
- 密码内嵌：打包时写进说明书 → 解压端 `password_hint.rs` 自动抓
- 自解压：`assets/sfx/yshjy-sfx.exe` 必须真 PE（`build.rs` 里有硬断言，`YSHJY_ALLOW_NON_PE_STUB=1` 只能开发用）
- **探针**：`tests/pack_layout_tests.py`、`pack_progress.sh`、`protocol_sfx*.py`

### 4.6 ⚠️ 两条代码路径：`api.rs`（界面）vs `main.rs`（CLI）

**这是本项目最容易漏改的地方**，HANDOFF 里明确记过：「RAR 接进度事件时只改 `api.rs`，拿真二进制实测仍是 0 条事件」。

| 路径 | 入口 | 谁用 |
|---|---|---|
| 协议路径 | `serve.rs` → `api.rs::dispatch` → `job::run_job` | **GUI / FFI / 所有外壳** |
| CLI 路径 | `main.rs` → `job::run_job` | 终端用户 |

`job::run_job` 是共用主干，但**事件接线、参数解析、输出格式**两边各写一份。
改任何「用户能看到的行为」时，**两条路径都要测**。探针：`tests/contract/run.py`（协议）+ `tests/*.sh`（CLI）。

### 4.7 安全护栏（内置，不可绕过）

| 护栏 | 做法 | 落点 |
|---|---|---|
| 路径穿越 | 解出来的路径必须规整后仍在目标目录内 | `safety.rs` |
| 空间不足 | 解前估算，不够就报 `NotEnoughSpace`（带结构化参数） | `safety.rs` + `hardware.rs` |
| 炸弹限额 | **不硬拒，走 `ask.rs` 问用户**（结构化请求 required/allowed） | `safety.rs` + `ask.rs` + `serve.rs` 的 `answer` |
| 删源 | **永不 rm**，只移 XDG 回收站 | `trash.rs` |

---

## 5. 对外面（协议 / CLI / 错误）

**协议 11 个命令**（`api.rs` 分派 10 个 + `skip` 在 `serve.rs` 层）：`hello` `cancel` `report` `scan` `list` `extract` `pack` `batch` `rescue` `answer` `skip`

**30 种事件**：`ASK_USER` `DIAGNOSTIC` `ENTRY_FINISHED` `EXTRACT` `EXTRACT_PROGRESS` `HIDDEN_AT_OFFSET`
`JOB_FINISHED` `JOB_STARTED` `LAYOUT_GUESSED` `LIST` `LOCATE` `OFFSET_CORRECTED` `PACK` `PACK_PROGRESS`
`PARTIAL_DATA` `PASSWORD_FROM_NOTES` `PASSWORD_NEEDED` `PASSWORD_TRY` `PREVIEW_READY` `REBUILT_INDEX`
`MOVABLE_NOT_VERIFIED` `RESUMED` `RISKY_ENTRY` `SCAN_PROGRESS` `SLOW_PATH` `SPACE_SHORTAGE` `STAGE_FINISHED` `STAGE_STARTED`
`VERIFY` `VIDEO_UNPLAYABLE` `VOLUMES_MERGED`

**17 个错误码**（`error.rs`）：`PathNotFound` `InvalidArgument` `NotEnoughSpace` `PermissionDenied`
`PathTooLong` `DestIsFile` `NeedPassword` `WrongPassword` `HeadEncrypted` `NotArchive` `Truncated` `MissingVolume`
`DataLost` `EncryptedUnbreakable` `VideoUnplayable` `Cancelled` `Internal`

**CLI 43 个长选项**，唯一真源 `src/cli_spec.rs`；`--help` / 三份补全 / man 页 **全部从它推导**，
并由 `tests/cli_spec_match.sh` 三方对账（不一致就红）。

---

## 6. 测试矩阵：每条能力谁在守

> `bash tests/all.sh` 跑全套（41 通过 / 0 失败 / 5 跳过，约 191–203 秒；2026-10-29 实测）。

| 能力 | 守它的探针 |
|---|---|
| 编译 / 静态检查 | `cargo clippy -D warnings`、`bash -n`（右键脚本 + 打包脚本） |
| 单元 / 集成 | `cargo test --release`（334+12+2） |
| 极端布局 E1–E6 | `tests/extreme_tests.py` |
| 打包布局 | `tests/pack_layout_tests.py` |
| 协议契约 | `tests/contract/run.py`、`protocol_*.py`（cancel/level/select/serve_eof/sfx/rescue/cover_only/smoke） |
| 错误文案说人话 | `tests/errmsgs.sh` |
| 重名策略 | `tests/conflict.sh` |
| 断点续传 | `tests/resume.sh`（含随机 kill 12 轮） |
| 原子落盘 | `tests/rar_atomic.sh` |
| 分卷 | `tests/rar_volume.sh`、`zip_symlink.sh` |
| 进度 | `tests/pack_progress.sh`、`progress_multi.sh` |
| 全屏勾选 | `tests/pick.sh` |
| 默认打开方式 | `tests/mime_default.sh`（断言 desktop 里 14 种 MIME） |
| CLI 选项对账 | `tests/cli_spec_match.sh` |
| 模糊测试 | `fuzz/`（写盘路径，60 秒，2.38 亿次零崩溃） |
| 界面 | `cd gui && flutter test`（13 条）+ `flutter analyze` |
| 出包 | `scripts/test-packages.sh`（33 项） |

---

## 7. 「改 X 去哪」索引（施工时直接查这张）

| 我要改… | 主要落点 | 别忘了 |
|---|---|---|
| 加一个**解压格式** | `unpack.rs`（新 `InArchive` 实现 + `find` 表）+ 新 `extract_*.rs` | `Caps`、`Shape`、`error.rs` 错误码、`locales/*.toml` 七语言 |
| 加一个**打包格式** | `pack/mod.rs` | `pack_post.rs`、`--split` |
| 加**CLI 选项** | `src/cli_spec.rs` 一行 | 跑 `scripts/gen-cli-assets.py` + `tests/cli_spec_match.sh` |
| 改**错误文案** | `error.rs`（码）+ `i18n.rs` / `locales/*.toml`（文案） | 引擎层**不许**写中文句子 |
| 改**进度条** | `progress.rs`（算法）+ `main.rs`（渲染） | GUI 与 CLI 必须显示同一个 ETA |
| 改**界面** | `gui/lib/pages/*.dart` | 改完必须 `flutter build linux` 再 `cp` 引擎（HANDOFF 铁律） |
| 改**协议** | `api.rs` + `docs/PROTOCOL.md` | 同步 `contract/run.py`；**还要看 `main.rs` 那条路** |
| 加**事件** | `event.rs` + `api.rs`/`main.rs` 两处接线 | `docs/PROTOCOL.md` + 契约探针 |
| 改**安全策略** | `safety.rs` | 护栏不可绕过；要问用户走 `ask.rs` |
| 改**FFI 签名** | `ffi/src/lib.rs` **+ `ffi/include/yshjy.h`** | 手写头文件会漂移（TODO 表尾有探针方案） |
| 改**桌面集成** | `gui/linux/yshjy.desktop`、`scripts/install-nautilus.sh`、`scripts/build-deb.sh` | MIME 计数有硬断言（14） |

---

## 8. 已知设计债 / 需要留意的三件事

`python3 scripts/scan-src.py` 会列出全部「底层用了高层」的反向依赖，共 **3 处**（都逐条看过，
都算合理妥协，但要知道它们是哪天想拆引擎时会挡路的地方）：

1. **`preview.rs` → `job.rs`（L1 → L2）**：预览复用了任务的识别/打开逻辑。
   如果以后要把引擎裁成「只做预览」的轻量库，这块第一个要拆。
2. **`drill.rs` → `batch.rs`（L1 → L2）**：嵌套包下钻要把内层包再排进批量队列 ——
   本质是「递归 = 再起一个任务」，这个依赖是设计使然，不是失误。
3. **`safety.rs` → `ask.rs`（L0 → L2）**：安全护栏超限时要问用户，而 `ask.rs` 在编排层。
   方向上是「地基回调上层」，但 `ask.rs` 只用 `event`，耦合很浅。

**另外两件要留意的**：

4. **`main.rs` 3045 行是最大的单文件** —— CLI 的选项解析、进度渲染、`--pick`、`--list` 全在里面。
   想加终端功能时会越来越挤（`cli_spec.rs` 已经把**数据**抽走了，剩下的是**行为**）。
5. **`ffi/include/yshjy.h` 是手写的第二份真源**（43 行 vs 9 个导出）—— 见 TODO 表尾的探针方案。

---

## 9. 与 `docs/ARCHITECTURE.md` 的偏差（那份是 10-16 的设计稿）

`ARCHITECTURE.md` 有价值的部分是**约束与取舍**（§1 目标、§8 安全护栏、§11 ADR 摘要），
但下面这些**已经过时，看它时别被带偏**：

| ARCHITECTURE.md 写的 | 现在实际 |
|---|---|
| §3「模块划分（crate 结构）」 | 早不是那套划分了 —— 以**本文档 §3** 为准（52 文件 29,584 行） |
| §12「v0.1 实现顺序（建议）」 | 已完成到 0.9.28，那节只是历史 |
| §13「待办（写代码前要补的）」 | 已清空；真源是 `docs/TODO.md` |
| §357「GUI 选型：为什么是 Flutter + 协议而不是 FFI」 | 仍成立（桌面走协议），但**移动端已定走 ffigen + C ABI**，两条路并存 |
| §210「并发与进度模型」 | 仍是这个思路，但细节以 `progress.rs` / `batch.rs` 源码为准 |

---

## 10. 怎么验证这份文档没过时

```bash
# 1. 重跑扫描，对比 §3 的数字（行数/依赖数变了就更新本文档）
python3 scripts/scan-src.py          # 完整模块地图 + 依赖矩阵 + 反向依赖
python3 scripts/scan-src.py --check  # 只打印核对项（与本节期望值逐个对）

# 2. 文件数 / 总行数
wc -l src/*.rs src/**/*.rs | tail -1        # 期望 29,584 行 / 52 文件（2026-10-29）

# 3. 协议面
grep -n '"hello", "cancel"' src/api.rs      # 命令清单
grep -oE '"[A-Z][A-Z_]{3,}"' src/event.rs | sort -u | wc -l   # 期望 30
sed -n '/pub enum ErrorCode/,/^}/p' src/error.rs | grep -cE '^\s{4}[A-Z]'  # 期望 16

# 4. 统一接口
grep -c "impl InArchive for" src/unpack.rs   # 期望 5 个实现 + 1 个测试假解压器
```

> 铁律：**这份文档里的数字必须是扫出来的**，手写数字会像 man 页版本号一样漂。
> 重扫结果与文中不一致时，改文档，别改脚本让它吻合。

- **落盘检验**（`flush.rs`）：解压/打包/救援/封面**四条写盘路径共用的一个判断** ——
  目标是不是移动设备（`/usb` 路径 或 盘级 `removable=1`，移动硬盘别忘了后者）、
  开关 `[safety] flush_movable` 开没开。真等的时候优先 `sync -f <目标>`（只刷该文件系统），
  每 200 ms 读 `/proc/meminfo` 的 `Dirty+Writeback` 报真进度，并按本次产物字节数封顶
  （内核读数是全系统的，不封顶会显示「打了个 15 MB 的包，还剩 472 MB」）。
  教训：**回读自检挡不住没落盘** —— 自检读到的可能是页缓存，所以先落盘再自检。
