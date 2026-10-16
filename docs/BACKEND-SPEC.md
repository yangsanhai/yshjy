# yshjy 后端补充设计（BACKEND-SPEC）

> 版本：v1（2026-10-16）　补全 `ARCHITECTURE.md` 遗留的 6 项设计缺口
> 目的：写代码前把"没想清楚的地方"全部想清楚，避免边写边返工

---

## ① 手机端 / GUI 与引擎的对接方案

### 1.1 为什么要现在定
引擎的 **API 形状**由"谁调用它"决定。如果按 CLI 写死了，将来接 Flutter 要重构。
所以现在就把"跨语言桥接"考虑进去。

### 1.2 三层暴露（从内到外）

```
yshjy-core        原生 Rust API（CLI 直接用，性能最好）
     │
yshjy-ffi         C ABI 稳定接口（给 Flutter / 其他语言）   ← 新增设计
     │
yshjy-json        事件/结果的 JSON 序列化（跨语言通用）      ← 新增设计
```

### 1.3 桥接技术选型（结论）

| 平台 | 方案 | 理由 |
|------|------|------|
| **桌面**（Linux/Win/macOS） | GUI 直接链接 `yshjy-core`（`flutter_rust_bridge`） | 同机进程内调用，最快 |
| **Android** | `flutter_rust_bridge` 编译为 `.so` | 成熟，支持异步与事件流 |
| **iOS**（最低优先） | 同上（静态库） | 沙箱限制另议 |

**关键设计约束（为了桥接友好）**
1. **引擎 API 不暴露复杂泛型/生命周期**——跨 FFI 边界只能用简单类型。
2. **所有跨边界数据是 `serde` 可序列化的结构**（Report / Event / Options）。
3. **进度用"事件流"而非回调链**：引擎产出 `Event` → FFI 层转成 Dart `Stream`。
4. **取消用 `handle`**：`start_task() -> TaskHandle`，`cancel(handle)`。
5. **不假设文件系统**：手机端要用 SAF/Content URI → 输出走 `OutputSink` trait（见⑤）。

```rust
// yshjy-ffi 对外接口草案（C ABI 友好）
pub extern "C" fn yshjy_start(
    json_options: *const c_char,      // JSON 字符串（Options）
    on_event: extern "C" fn(*const c_char),  // 事件回调（JSON）
) -> *mut TaskHandle;

pub extern "C" fn yshjy_cancel(h: *mut TaskHandle);
pub extern "C" fn yshjy_free_handle(h: *mut TaskHandle);
```

> 这样 Android 端只需处理"JSON 进、JSON 出 + 回调"，不需要理解 Rust 内部结构。

---

## ② 配置与密码本的存储设计

### 2.1 位置（XDG 标准，跨平台由 `directories` crate 解析）

| 平台 | 配置目录 | 密码本 |
|------|---------|--------|
| Linux | `~/.config/yshjy/` | `~/.config/yshjy/passwords.txt` |
| Windows | `%APPDATA%\yshjy\` | 同目录 |
| macOS | `~/Library/Application Support/yshjy/` | 同目录 |
| Android | 应用私有目录 | 同目录 |

**可覆盖**：环境变量 `YSHJY_CONFIG_DIR`；CLI `--config <path>`。

### 2.2 配置格式（TOML，人类可读可手改）

```toml
# ~/.config/yshjy/config.toml
version = 1

[general]
language = "zh-CN"          # 中文优先，预留多语言
default_mode = "auto"       # auto | extract | pack
jobs = 1                    # 并行任务数（磁盘友好）

[password]
file = "passwords.txt"
auto_from_notes = true      # 从同目录"说明.txt"里自动抓解压码
max_candidates = 12         # 上限；**硬上限 64**，配置改不出爆破模式
# 注：原本这里还有个 try_common（尝试内置常见解压码库），已**刻意删除** ——
#     内置常见密码表等于自带小字典，越过安全边界（见 README「本工具不做的事」）

[output]
dir = "~/yshjy-output"      # 默认输出目录
conflict = "rename"         # rename | skip | ask
keep_intermediate = false   # 是否保留中间提取产物

[safety]
memory_limit_mb = 64        # 解 8GB 也不许超这个值
zip_bomb_ratio = 100        # 解压后/前 比值上限
warn_executables = true     # 解出 exe/sh 时高亮提醒
follow_symlinks = false     # 默认不跟随（防攻击）

[layout]
default = "L2"              # 默认打包布局
```

### 2.3 密码本格式（`passwords.txt`）

```
# 每行一个密码，UTF-8，允许中文
# '#' 开头为注释行
# 不做任何编码过滤（⚠️ 隐写者的 bug 就是 ascii 过滤吞掉中文密码）
demo-password
wangyihan2024
动漫之家
```

**兼容性**：隐写者的 `modules/PW.txt` 是 `Tab 分隔 + ascii 过滤`。我们的导入功能应：
- 读取时**按 Tab 取第一列**（兼容）
- 但**不过滤非 ASCII**（修正它的 bug）
- `yshjy --import-passwords <file>` 一键导入

---

## ③ 打包布局的规则描述（L1~L8 数据化）

### 3.1 设计目标
新增/调整打包玩法时**只改数据表，不改代码**。

### 3.2 规则结构

```rust
pub struct LayoutSpec {
    pub id: LayoutId,                 // L1..L8
    pub name: &'static str,           // "尾部追加"
    pub summary: &'static str,        // 一句话说明（给界面显示）
    pub segments: Vec<SegSpec>,       // 段的顺序与内容
    pub abilities: Abilities,         // 需要哪些额外能力
    pub compat: Compat,               // 实测兼容性（决定界面上是否警告）
    pub warnings: Vec<WarnCode>,      // 选中时必须展示的警告
    pub stability: Stability,         // Stable | Advanced | Disabled
}

pub enum SegSpec {
    ShellWhole,              // 外壳文件原样
    ShellRange(Range),       // 外壳的一段（如"前 80%"）
    FreeHeader { #[serde(default)] padded: Option<usize> },  // free 原子头（可带随机填充）
    Payload,                 // 压缩数据
    PayloadPart(u32, u32),   // 压缩数据的第 n/m 块（分散布局用）
    Random(usize),           // 随机填充
    FakeSignatures(usize),   // 假签名数量（防扫描器）
    Trailer,                 // 尾部空 mdat
    MkvAttachment,           // 作为 MKV 附件轨
}

pub struct Abilities {
    pub rewrite_mdat_size: bool,     // 要改 mdat 尺寸
    pub rewrite_stco: bool,          // 要改 stco/co64 偏移表
    pub chunk_boundary: bool,        // 插入点必须卡在 chunk 边界
}

pub enum Stability { Stable, Advanced, Disabled }
```

### 3.3 实测结论直接写进表（本次实验数据）

```rust
// L1 稳定：尾部追加
LayoutSpec { id: L1, name: "尾部追加", stability: Stable, warnings: vec![],
  segments: vec![ShellWhole, Payload],
  abilities: Abilities::none(), compat: Compat::all_tools_ok() }

// L2 稳定 + 推荐默认：尾部 free 原子（多 8B 头 + 随机 pad）
LayoutSpec { id: L2, name: "尾部 free 原子", stability: Stable, warnings: vec![],
  segments: vec![ShellWhole, FreeHeader{padded: Some(217)}, Payload],
  abilities: Abilities::none(), compat: Compat::all_tools_ok() }

// L3 高级：ftyp 后 free 原子（unzip / 资源管理器找不到 → 必须警告）
LayoutSpec { id: L3, name: "ftyp 后 free 原子", stability: Advanced,
  warnings: vec![NeedScanTool],          // "建议用 7-Zip 或我们的工具解"
  segments: vec![ShellHead, FreeHeader{padded: None}, ShellTail], ... }

// L4 高级：插在视频中间（技术亮点，但要改两处 + 卡 chunk 边界）
LayoutSpec { id: L4, name: "插在视频中间", stability: Advanced,
  warnings: vec![NeedScanTool],
  segments: vec![ShellRange(head), Payload, ShellRange(tail)],
  abilities: Abilities { rewrite_mdat_size: true, rewrite_stco: true, chunk_boundary: true } }

// L5 / L6 禁用（实测：L5 解压残废、L6 视频播不了）
LayoutSpec { id: L5, name: "分散多段", stability: Disabled,
  warnings: vec![BreaksExtraction],   // 7z 只能解出 1/4
  segments: vec![ShellHead, PayloadPart(0,2), ShellMid, PayloadPart(1,2), ShellTail] }
LayoutSpec { id: L6, name: "前后加垃圾", stability: Disabled,
  warnings: vec![BreaksVideo],        // 视频 rc=183
  segments: vec![Random(5<<20), Payload, Random(5<<20)] }

// L7 高级：两个包（通用工具只看到第一个/最后一个）
LayoutSpec { id: L7, name: "双包", stability: Advanced, warnings: vec![OnlyOneVisible] }

// L8 可选：MKV 附件轨（unzip 家族全废）
LayoutSpec { id: L8, name: "MKV 附件", stability: Advanced, warnings: vec![NeedMkvTool],
  segments: vec![MkvAttachment] }
```

### 3.4 打包后自检（硬性要求，来自实测教训）

```
1. 结构自检：重新解析生成的文件 → 必须能定位到数据
2. 视频自检：ffmpeg 解码 → 必须 rc==0 且 stderr 为空（rc=0 也会花屏！）
3. 回环自检：用自己的引擎解一遍 → SHA-256 必须一致
三样全过才算打包成功；否则报错并保留中间产物供排查
```

---

## ④ 诊断包格式（用户报障用）

> **落地状态（2026-10-17，v0.9 第 8~9 批）**：已实现，见 `src/report.rs`。
> 与本文档设计的**两处差异**（第一处是设计本身有漏洞，实现时修掉了）：
> 1. **meta.json 不记「完整命令行」，只记脱敏后的** —— 用户可能是用
>    `yshjy -p 我的解压码 包.mp4` 调用的，那完整命令行就是**明文密码**。
>    现在 `-p` / `--password` 的值一律抹成 `<已抹掉>`；代理环境变量也只记
>    "有没有设"、不记值（值可能长成 `http://user:pass@host`）。
> 2. **findings.json 不含文件名清单**（只给数量、加密数、总大小）——
>    设计里没写这条，但它同样会泄露"你下载了什么"。
>
> 落地时另加了 `--no-paths`（路径统一换成 `<path-1>` 这类占位符）。
> 实测（生成一份带假密码的逐字段检查）还逮到三个实现问题并修掉：
> `--report -p 密码 包` 会把**密码**当成目标文件、磁盘余量恒为 `null`、
> zip 条目时间戳是 1980。

### 4.1 触发方式
```bash
yshjy --report              # 生成诊断包
yshjy --report --no-paths   # 路径脱敏（分享到公开论坛时用）
```

### 4.2 内容

```
yshjy-report-20261016-193012.zip
├── meta.json          版本 / OS / CPU / 内存 / 磁盘余量 / 完整命令行 / 是否用代理
├── findings.json      识别结果：外壳类型 / 格式 / 段偏移 / 基准偏移 / 置信度 / 备注
├── log.txt            运行日志（默认级别；含耗时与关键偏移）
├── config.snapshot.toml   当时生效的配置（脱敏：不含密码）
└── README.txt         这个包是什么、可以贴到哪、里面不含什么
```

### 4.3 隐私红线（必须遵守）

```
❌ 绝不包含：文件内容、任何密码、加密哈希之外的隐私
⚠️ 路径：默认含（便于定位），--no-paths 时替换为 <path-1> <path-2>
✅ 必须在 README.txt 里明确写清"包含什么/不包含什么"
```

---

## ⑤ 解压器统一接口（trait 设计）

> **落地状态（2026-10-17，v0.9 第 1~3 批）**：已实现，见 `src/unpack.rs`。
> 与本文档设计的**三处差异**（都是落地时收敛的，不是漏做）：
> 1. **能力用数据表，不用 `QueryInterface`**：`Caps` 结构体直接可读（界面要画"支持加密/分卷"
>    不需要先试探）；7-Zip 的 QI 思路留在这里当参考，Rust 里向下转型易错，不抄。
> 2. **`Source` 只有「一条流 + base/len + 分卷列表 + 压缩总大小」**，**没有 `path`** ——
>    落地时发现 RAR 也能从流读（新增 `extract_rar_stream_ctx`），没有任何 handler 需要路径，
>    字段就删了（接口最小化）。
> 3. **读写分离照做**：`trait InArchive` 已落地；写侧（`trait OutArchive`）还没做，
>    当前用 `Caps::can_create` 表达"能不能反向打包"。
>
> 落地时另加了两样本文档没写的东西：**`Shape`**（源形状：普通 / 多卷 / 单流 / tar 家族，
> 注册表按「格式 + 形状」查 handler）和 **`HANDLERS`** 静态有序注册表（顺序即优先级）。
> 编排层（`job.rs`）里唯一按格式取实现的地方是 `unpack::find_or_err` —— 加格式不用改它。

### 5.1 核心 trait

```rust
/// 输入：可读可定位的数据源（可能是虚接的多段、可能带偏移）
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// 输出：抽象目的地（本地文件系统 / 手机 SAF / 内存 / 测试）
pub trait OutputSink {
    fn create(&mut self, entry: &EntryInfo) -> Result<Box<dyn Write>>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    /// 安全：实现方必须校验路径不逃出根目录
    fn resolve_safe(&self, path: &str) -> Result<SafePath>;
}

/// 统一解压器
pub trait Unpacker {
    fn format(&self) -> ArchiveFormat;
    fn capabilities(&self) -> Caps;

    /// 列内容（解压前预览用；加密包在无密码时也应尽量能列出文件名）
    fn list(&self, src: &mut dyn ReadSeek, pw: Option<&Password>)
        -> Result<Vec<EntryInfo>>;

    /// 解压（支持选择性提取）
    fn unpack(
        &self,
        src: &mut dyn ReadSeek,
        out: &mut dyn OutputSink,
        pw: Option<&Password>,
        selection: &Selection,
        ctx: &UnpackCtx,               // 含取消标志、内存上限、进度回调
    ) -> Result<UnpackStats>;

    /// 健康检查（能否完整解出；用于自检与救援判断）
    fn test(&self, src: &mut dyn ReadSeek, pw: Option<&Password>) -> Result<TestReport>;
}

pub struct Caps {
    pub encrypted: bool,        // 支持解密
    pub split_volumes: bool,    // 支持分卷
    pub selective: bool,        // 支持只解部分条目
    pub stream_input: bool,     // 能否流式输入（不支持则需临时文件）
    pub name_encrypted: bool,   // 是否可能加密文件名（7z -mhe=on）
}
```

### 5.2 配套类型

```rust
pub struct EntryInfo {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: Option<u64>,
    pub encrypted: bool,         // ← 界面上的 🔒 就是读它
    pub crc32: Option<u32>,
    pub modified: Option<i64>,
}

/// 密码类型：内部统一处理编码问题（GBK ↔ UTF-8 自动互转）
pub enum Password {
    Raw(Vec<u8>),        // 已是正确的字节
    Text(String),        // 需要按候选编码表展开
}

pub struct UnpackStats {
    pub entries_total: u64,
    pub entries_done: u64,
    pub bytes_written: u64,
    pub failed_entries: Vec<(String, String)>,   // 救援模式用
}
```

### 5.3 各格式的实现要点（来自 spike 实测）

| 格式 | 实现方式 | 关键点 |
|------|---------|--------|
| ZIP/ZIP64 | `zip` crate + **自研偏移定位** | 必须用 `Known(Δ)`，禁用它自带的 Detect（113 秒 vs 69 微秒）；开 `legacy-zip` |
| 7z | `sevenz-rust2` | 包名是 `sevenz-rust2`；非零偏移要自己包 `Window`；**内存上限要自建** |
| RAR | `rars` | 用 `find_archive_start` 而非 `detect_archive_family`；自带 8MiB SFX 扫描 |
| TAR/GZ/XZ/ZST/BZ2 | `tar` + `flate2`/`xz2`/`zstd`/`bzip2-rs` | 单流，注意多流拼接（tar.gz 里可能有多个成员流） |
| 分卷 | **自己实现 `SegmentReader`** | 7z 分卷是裸字节切分；zip 分卷要看 `.z01` 链 |

---

## ⑥ 日志分级与内容约定

### 6.1 分级（CLI 开关）

```
--quiet    只输出最终结果（脚本/管道用）        exit code 表达成败
（默认）    进度 + 关键警告 + 结果
-v         详细：每阶段耗时、识别细节、候选列表
-vv        调试：偏移计算、结构 dump、密码尝试记录
--log FILE 同时写入文件（可与诊断包联动）
```

**退出码约定**（脚本可用）：
```
0  成功（或部分成功且用户未要求严格模式）
1  用户问题（路径不存在/磁盘不够/参数错误）
2  文件问题（结构损坏/密码错误/分卷缺失）
3  内部错误（bug，建议附诊断包提 issue）
```

### 6.2 两类文本必须分离（重要）

| 类型 | 面向 | 语言 | 位置 |
|------|------|------|------|
| **用户文案** | 用户 | 中文白话（CLI 可含 emoji） | **外壳侧渲染**（引擎只发错误码 ⚠️ 2026-10-16 修正） |
| **工程日志** | 开发者/诊断 | 中性英文 + 错误码 | `log` 宏，落 `log.txt` |

例：
```
用户看到：  ⚠️ 这个包有密码保护，需要你提供密码
日志记录：  WARN [PWD_REQUIRED] zip entries encrypted, flags=0x0001, candidates=0
```

**理由**：用户看得懂 + 开发者查得动，两者不能混在一起。

### 6.3 必须记日志的节点（诊断价值）

```
· 识别：外壳类型、扫描到的候选数、每个候选的置信度与理由
· 定位：算出的 base 偏移、Δ 值、结构自校验结果
· 提取：段列表、实际读取字节数
· 解压：每个失败条目的原因（救援模式的关键数据）
· 校验：期望值 vs 实际值
· 性能：各阶段耗时、峰值内存
```

---

## 附：本次补全后的后端设计清单

| # | 项目 | 状态 |
|---|------|------|
| 1 | 手机端/GUI 对接方案 | ✅ 三层暴露 + FRB + FFI 草案 |
| 2 | 配置与密码本存储 | ✅ XDG 路径 + TOML 字段 + 密码本格式（并不做 ascii 过滤） |
| 3 | 打包布局规则化 | ✅ `LayoutSpec` 结构 + L1~L8 数据表（实测结论已写入） |
| 4 | 诊断包格式 | ✅ 内容清单 + 隐私红线 |
| 5 | 解压器统一接口 | ✅ trait + 配套类型 + 各格式实现要点 |
| 6 | 日志分级与内容 | ✅ 四级 + 退出码约定 + 用户文案/工程日志分离 |

**后端设计至此 → 100%**（可开工）

---

## ⚠️ 三层贯通审查修正记录（2026-10-16）

> 详见 `refs/analysis/08-三层贯通审查.md`（发现 9 个问题，3 个为真问题）

### 后端相关修正
| # | 问题 | 修正 |
|---|------|------|
| B1 | 取消不及时 | 取消检查**下沉到 Reader 层**（`InterruptibleReader` 每 1MiB 检查，返回 io::Error 让库中断） |
| B2 | 试密码污染输出 | **两步走**：先只解"第一个条目的首块"验证 CRC，通过后才正式解压；验证产物写临时区、失败即丢 |
| B3 | 校验重复读 8GB | 用 `HashingWriter` 包装输出流，**边解压边算 SHA-256**，单次 I/O 完成 |
| B4 | 虚接随机访问慢 | 构造时建**段索引表**（按起始偏移排序）+ seek 二分查找 + 段内指针缓存 |

### 前端 / 桥接相关修正
| # | 问题 | 修正 |
|---|------|------|
| F1 | 手机端存储可能不支持 seek | 三档方案：① 直接 fd 路径（支持 seek）② 不支持时**块缓存** ③ 兜底**先复制到私有目录**（须先做空间预检）；选文件后立即探测 seek 能力 |
| F2 | 进度事件跨语言开销 | 100ms 内**合并推送**；列表类数据（如 351 条目）**一次性批量传**，不逐条传 |
| F3 | 错误码→界面映射缺失 | 引擎只发码，外壳渲染；UI-SPEC 补「错误码 → 界面形态」表 |
| M1 | 回调线程安全 | Rust 侧事件进**有界队列**；FRB 用 Stream（内部处理线程切换）；取消由 handle 控制 |
| M2 | "本地服务"备选多余 | **删除**该方案，桥接统一用 flutter_rust_bridge |

### 待验证（不阻塞 v0.1）
- [ ] **Rust → Android 构建链**最小验证（返回字符串 → 编译 .so → Flutter 调用），v0.1 后尽快做
- [ ] 实测"百度网盘 App 分享过来的文件能否 seek"
