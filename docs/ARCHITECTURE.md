# yshjy 架构设计文档

> 版本：v1（2026-10-16）　适用：v0.1（Linux CLI）
> 前置：`DESIGN.md`（需求）、`refs/analysis/`（19 份调研与验证报告）

---

## 1. 设计目标与约束

**目标**：一个引擎，三种外壳（CLI / 桌面 GUI / 手机 GUI），零重复逻辑。

**约束**（来自项目决策）：
| 约束 | 来源 |
|------|------|
| 纯 Rust 引擎，只用开源依赖 | 用户要求（RAR 只解不压） |
| 引擎不依赖任何 UI 框架 | 三平台复用 |
| 流式处理，解 8GB 不许超 64MB 内存 | 实测基线 + 用户"别爆掉"要求 |
| 必须能处理"偏移被篡改 / 前后垃圾 / 数据分段"的文件 | 实测 05/04 报告 |
| 安全护栏内置（路径穿越 / zip bomb / 内存耗尽） | 06 审查 |
| 允许桌面端为 7z 等格式引入受限内存的 native 依赖 | 现实权衡 |

---

## 2. 分层架构

```
┌───────────────────────────────────────────────────────────┐
│  外壳层（Shells）                                          │
│   yshjy-cli（v0.1）   yshjy-gui（Flutter）   yshjy-mobile  │
└──────────────────────────┬────────────────────────────────┘
                           │  只通过「事件流 + 结果结构」交互
┌──────────────────────────▼────────────────────────────────┐
│  引擎层 yshjy-core（无 UI、无 I/O 假设、可单元测试）        │
│                                                            │
│   probe ──► scan ──► layout ──► plan ──► extract ──►       │
│                                          unpack ──► verify  │
│   ▲                        │                               │
│   └── password / rescue ◄──┘        pack（反向：打包）      │
│                                                            │
│   基础设施：error / progress / safety / config / iounit    │
└───────────────────────────────────────────────────────────┘
```

**关键原则**
1. 引擎不知道"界面"存在，只发**事件**和返回**结构化结果**。
2. 引擎不打印任何东西（日志/文案由外壳决定）。
3. 引擎不假设文件系统（所有输入输出走 trait，便于测试与手机端适配）。

---

## 3. 模块划分（crate 结构）

```
yshjy/                       (workspace)
├── Cargo.toml
├── crates/
│   ├── yshjy-core/          引擎（v0.1 主战场）
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs        错误类型体系
│   │       ├── progress.rs     进度事件定义
│   │       ├── safety.rs       安全护栏（路径/内存/大小）
│   │       ├── config.rs       配置与密码本
│   │       ├── io/
│   │       │   ├── mod.rs
│   │       │   ├── segment.rs   SegmentReader（虚接多段）
│   │       │   ├── window.rs    Window（偏移映射）
│   │       │   └── patch.rs     PatchWindow（元数据补丁）
│   │       ├── probe/          外壳识别（MP4/MKV/图片/裸文件）
│   │       ├── scan/           全文件签名扫描（多候选、去假签名）
│   │       ├── layout/         结构解析与偏移反推
│   │       │   ├── zip.rs        ZIP/ZIP64（EOCD/CD/LFH）
│   │       │   ├── sevenz.rs     7z（StartHeader/NextHeader）
│   │       │   ├── rar.rs        RAR4/RAR5
│   │       │   └── tar.rs        TAR/GZ/XZ/ZST/BZ2 单流
│   │       ├── plan.rs         提取计划（段/偏移/密码候选/输出策略）
│   │       ├── extract.rs      流式提取
│   │       ├── unpack/         解压器分发
│   │       │   ├── zip.rs / sevenz.rs / rar.rs / tar.rs
│   │       ├── password/       候选生成 + 编码纠错 + 限量尝试
│   │       ├── rescue/         救援模式（损坏/缺卷/部分数据）
│   │       ├── verify.rs       校验（SHA-256 / CRC / 文件数）
│   │       ├── report.rs       结构化报告（供 CLI/GUI 渲染）
│   │       ├── pack/           打包（L1~L8 布局，规则驱动）
│   │       └── shell/          外壳文件处理（MP4 atom / MKV EBML / 图片 trailer）
│   ├── yshjy-cli/           CLI 外壳
│   │   └── src/main.rs        参数解析 → 调 core → 渲染进度与报告
│   └── gui/                 Flutter 前端 + `--serve` 协议（见 docs/PROTOCOL.md）
└── tests/                   集成测试（跑 tests/data 全样本）
```

---

## 4. 核心数据结构

```rust
// ── 输入 ────────────────────────────────────────────
pub struct Task {
    pub sources: Vec<PathBuf>,
    pub mode: Mode,                 // Auto | Extract | Pack
    pub options: Options,
}

pub struct Options {
    pub output_dir: Option<PathBuf>,
    pub password_candidates: Vec<String>,
    pub password_file: Option<PathBuf>,
    pub layout: PackLayout,         // 打包时用（L1~L8）
    pub dry_run: bool,
    pub rescue: bool,               // 允许救援模式产出残缺结果
    pub memory_limit: u64,          // 默认 64 MiB
}

// ── 探测 ────────────────────────────────────────────
pub enum ShellKind { Plain, Mp4, Mkv, Image(ImageKind), Pdf, Unknown }

pub enum ArchiveFormat { Zip, SevenZ, Rar4, Rar5, Tar, Gzip, Xz, Zstd, Bzip2, Unknown }

/// 一个「发现」= 某处存在一段疑似归档数据
pub struct Finding {
    pub shell: ShellKind,
    pub format: ArchiveFormat,
    pub segments: Vec<Segment>,      // 数据可能分散在多段 → 需虚接
    pub base_offset: u64,            // 基准偏移（用于结构内相对偏移修正）
    pub declared_len: Option<u64>,
    pub encrypted: bool,
    pub confidence: Confidence,      // 由结构校验得出，不是"见到签名就算"
    pub notes: Vec<Note>,            // 例："EOCD 距尾 118902 字节"
}

pub struct Segment { pub start: u64, pub len: u64 }

// ── 结果 ────────────────────────────────────────────
pub enum Outcome {
    Success,
    Partial { recovered: u64, lost: u64, reasons: Vec<String> },  // 救援模式
    NeedsPassword { hint: Option<String> },
    Failed { reason: FailureReason },
}

pub struct Report {
    pub findings: Vec<Finding>,
    pub entries: Vec<EntryInfo>,     // 解压前预览用
    pub outcome: Outcome,
    pub stats: Stats,                // 耗时/字节/峰值内存
    pub suggestions: Vec<String>,    // "建议用 7-Zip 打开" 这类下一步
}
```

---

## 5. 数据流管线

```
                 ┌─────────────┐
 文件路径 ──────► │ probe       │ 识别外壳类型（读头尾若干 KB）
                 └──────┬──────┘
                        ▼
                 ┌─────────────┐
                 │ scan        │ 全文件扫签名（分块+重叠），产出候选列表
                 └──────┬──────┘  ⚠️ 不能"见签名即认"，必须送去 layout 校验
                        ▼
                 ┌─────────────┐
                 │ layout      │ 解析结构 → 反推真实偏移 → 结构自校验
                 └──────┬──────┘  （ZIP: EOCD/CD/LFH；7z: StartHeader/NextHeader…）
                        ▼
                 ┌─────────────┐
                 │ plan        │ 制定提取计划：段、偏移、密码候选、输出策略
                 └──────┬──────┘  （含"选择不打草稿直接流式解压"的优化）
                        ▼
        ┌───────────────┴───────────────┐
        ▼                               ▼
 ┌─────────────┐                 ┌─────────────┐
 │ extract     │ 流式提取        │ rescue      │ 救援分支（结构损坏时）
 └──────┬──────┘ （SegmentReader）└──────┬──────┘
        ▼                               ▼
 ┌─────────────┐
 │ unpack      │ 分发给对应格式解压器（zip/7z/rar/tar…）
 └──────┬──────┘
        ▼
 ┌─────────────┐
 │ verify      │ SHA-256 / CRC / 文件数 / 空文件检查
 └──────┬──────┘
        ▼
 ┌─────────────┐
 │ report      │ 结构化结果 + 建议 → 交给外壳渲染
 └─────────────┘
```

**进度事件**（引擎 → 外壳，单向推送）
```rust
pub enum Event {
    Probing,
    Scanning { scanned: u64, total: u64 },
    Found { count: usize },
    Planning,
    Extracting { done: u64, total: u64 },
    Decrypting { attempt: usize, of: usize },
    Unpacking { entries_done: u64, entries_total: u64, current: String },
    Verifying,
    Finished { outcome: Outcome },
    Warning { code: WarnCode, detail: String },
}
```
- CLI：直接渲染成中文进度条 + 白话
- GUI：订阅同一事件流，转成界面动画

---

## 6. 并发与进度模型

- **单任务内**：提取与解压流水线化（生产者/消费者两线程，通过有界通道背压），避免"先整体提取再整体解压"的双倍 I/O。
- **哈希校验**：与解压并行（第三条线程读同样数据），或解压后单独跑（v0.1 简化为单独跑）。
- **多任务**：任务级并行（`--jobs N`），默认 1（避免磁盘争抢），批量模式可调到 2~4。
- **取消**：`Arc<AtomicBool>` 贯穿，并**下沉到 Reader 层**：
  `InterruptibleReader` 每读 1 MiB 检查一次取消标志，取消时返回 `io::Error`，
  让解压库自身中断。（解压库内部循环不受我们控制，纯靠外层检查会"点了取消没反应"。）
- **进度节流**：事件最多每 100ms 推一次（GUI 友好，避免刷屏）。

---

## 7. 错误处理体系

```rust
pub enum Error {
    // 用户可自行解决 → CLI/GUI 给出人话建议
    User(UserIssue),        // 需要密码 / 磁盘不够 / 文件不存在 / 输出目录无权限
    // 文件本身的问题 → 引导到救援模式或给出解释
    BadSource(BadSource),   // 结构损坏 / 偏移不可解 / 加密不可破 / 分卷缺失
    // 环境/内部问题 → 记录日志，建议提 issue
    Internal(Internal),     // IO / 依赖失败 / 逻辑不变量被破坏
}
```
**原则**：
- 每个 `Error` 只承载**错误码 + 结构化参数**，例如 `Err(NeedPassword { entries: 351, encrypted_names: true })`。
- **引擎绝不产出面向用户的中文句子**（⚠️ 2026-10-16 修正：原设计把文案放在引擎的 `report.rs`，
  这与"引擎不打印任何东西"自相矛盾）。渲染由**外壳**负责：CLI → 终端文案；GUI → 界面组件（图标+标题+建议+按钮）。
- 好处：① 引擎纯净可测；② 以后加英文只改外壳；③ 手机端不必通过 FFI 传大段中文。
- 内部错误必须带上下文（文件、偏移、阶段），便于用户导出诊断包。

---

## 8. 安全护栏（内置，不可绕过）

| 风险 | 措施 |
|------|------|
| **Zip Slip / 路径穿越** | 解压路径强制规范化；拒绝 `..`、绝对路径、盘符、超长名；写入前检查是否逃出输出目录 |
| **符号链接攻击** | 默认不创建软/硬链接；要创建时校验目标在输出目录内 |
| **zip bomb / 炸弹** | 声明大小与累计解压量双阈值；超过即中止并报警 |
| **内存耗尽** | 全局内存上限（默认 64 MiB）；7z 库需自建护栏（库自身限制形同虚设） |
| **同名覆盖** | 默认不覆盖，自动重命名并记录 |
| **可执行文件提示** | 解出 `.exe/.scr/.bat/.sh/.apk` 时在报告里高亮提醒 |
| **临时文件权限** | 临时目录 0700，解压内容不设 0777 |

---

## 9. 可扩展性设计

1. **格式规则表**（不硬编码分支）
   `formats.toml` 内建：格式名 / 签名 / 签名偏移 / 校验方式 / 解压器 key。
   新增格式 = 加表项 + 实现解压器，**不改管线**。
2. **布局规则化**（打包端）
   `pack/layouts.rs` 用声明式描述 L1~L8（段顺序、对齐要求、需改哪些字段），
   新增玩法 = 加一条规则。
3. **探测链**（probe）
   顺序执行 + 首个高置信度胜出；探测器可独立增删（学 unar 的有序表设计）。
4. **诊断包**（`--report` 导出）
   环境 + 参数 + 日志 + 关键偏移与结构摘要 → 用户报障只需贴一个文件。

---

## 10. 测试策略

| 层级 | 内容 | 数据 |
|------|------|------|
| 单元 | 偏移反推、结构解析、路径规范化、密码编码纠错 | 构造的小数据 |
| 集成 | **全样本回归**：跑 `tests/data/` 全部 59 个样本，断言"能解/预期失败" | 现有样本集 |
| 打包回环 | 生成 L1~L8 → 再用引擎解 → 比 SHA-256 | `pack_layout_tests.py` 移植 |
| 性能 | 大文件流式（内存峰值 ≤64MB）、扫描吞吐 | 合成大样本（不入库） |
| 安全 | 恶意路径、zip bomb、超长文件名 | 专门构造 |

**CI**（GitHub Actions）：`fmt + clippy + test + cargo-deny`（许可证/漏洞审计）。
样本集入库（22MB+15MB，可接受）或按需生成（脚本已可重跑）。

---

## 11. 关键设计决策（ADR 摘要）

| # | 决策 | 理由 |
|---|------|------|
| D1 | 引擎与外壳严格分离，靠事件流通信 | 三平台复用；GUI 可无痛替换 |
| D2 | 自研 ZIP 结构解析 + 自己算 base 偏移 | 实测：`zip` crate 的自动检测要 113 秒，`Known(Δ)` 只要 69 µs |
| D3 | 用 `SegmentReader` 虚接多段数据 | 应对"数据被切开分插"（E3 已验证可行） |
| D4 | 扫描必须"结构校验"通过才算发现 | 隐写者会故意撒假签名 |
| D5 | 分批分级预读（4KB → 512KB → 1MB） | 学 unar：便宜检查在前，昂贵扫描在后 |
| D6 | 默认布局 = L2 尾部 free 原子 | 实测：视频能播 + 8 种工具全通 |
| D7 | 不提供 L5/L6 布局 | 实测：L5 解压残废（1/4）、L6 视频播不了 |
| D8 | 打包自检必须 `ffmpeg rc==0 && stderr 为空` | 实测：rc=0 也会满屏花屏 |
| D9 | 密码只"找"不"破"：说明提取/密码本/常见库/用户输入 | 用户决策 + 不越界 |
| D10 | 解压前先列内容并标注加密状态 | 用户提议：让用户自己看到，透明优先 |

---

## 12. v0.1 实现顺序（建议）

```
1. io/（SegmentReader / Window / PatchWindow）+ error + safety   ← 地基
2. layout/zip.rs（ZIP64 偏移反推）+ scan/                        ← 核心能力
3. extract + unpack/zip + verify + report                        ← 第一个可跑通流程
4. yshjy-cli（参数 + 中文进度渲染）                               ← 用户第一次能用
5. 跑通 tests/data 里的 A1/A6/C1/D2a/D2b 五个样本                 ← 里程碑
6. 补 sevenz / rar / tar 家族 + 分卷 + 密码                       ← 格式覆盖
7. rescue + 极端场景（E1~E6）                                     ← 抗打击
8. pack（L1~L2 起步）                                            ← 反向能力
```

---

## 13. 待办（写代码前要补的）

- [ ] `docs/UI-SPEC.md`：异常状态界面 + 文案表（06 审查 B1/B2）
- [ ] `LICENSE`（MIT）+ `README.md` + `CONTRIBUTING.md` + `CHANGELOG.md`
- [ ] `rustfmt.toml` / `clippy.toml` / `deny.toml` + `.github/workflows/ci.yml`
- [ ] `DESIGN.md` 章节编号重排（当前 十一~十五 插在 四/五 之间）
- [ ] 竞品界面参考收集（Complex-unzip-tool-v2 / Bandizip / ZArchiver 截图）

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


---

## GUI 选型：为什么是「Flutter + 协议」而不是 FFI

| 维度 | 协议（`--serve`，本项目选它） | FFI（flutter_rust_bridge / cbindgen） |
|---|---|---|
| 引擎改接口后要做什么 | 不用动前端（只加不删） | 重新生成绑定、重编前端 |
| 换前端框架（Flutter→Tauri） | 前端重写，引擎一行不动 | 绑定全废 |
| 跨平台（桌面/手机） | 同一套标准流 | 每平台编动态库 + 打包 |
| 调试 | `yshjy --serve` 手喂 JSON，一眼看出是哪边的锅 | 需要原生调试器 |
| 性能 | 一次系统调用级开销（事件 ≤10 次/秒，可忽略） | 略快（差异在纳秒级） |

结论：解压工具是 **I/O 密集**不是调用密集，协议的开销可以忽略；
换来的是"前后端各自升级"的自由度 —— 这正是用户明确要的。

契约细节见 `docs/PROTOCOL.md`；界面实现见 `gui/`。

---

## GUI 文件组织规范（2026-11-01 定，按块拆分）

### 问题

`gui/lib/pages/home_page.dart` 原本 **3117 行**，其中 **2976 行是同一个类** `_HomePageState`。
改一处界面行为要在三千行里翻，而且字段、方法、UI 全耦合在一起。

### 参照：业界怎么组织 Flutter 桌面界面（实测，不是印象）

| 项目 | 最大页面文件 | 里面几个类 | 最大单个类 | 小部件放哪 |
|---|---|---|---|---|
| RustDesk（`flutter/lib/desktop/pages/desktop_setting_page.dart`） | 3376 行 | **26 个** | 759 行（`_SafetyState`） | `desktop/widgets/` |
| LocalSend（`app/lib`，排除 `gen/` 生成代码） | 851 行 | 一个小部件一个文件 | 851 行 | `widget/`（20+ 个独立文件） |
| **本项目（改之前）** | 3117 行 | **4 个** | **2976 行** | 全部塞在 State 里 |

**结论**：差距不在「文件多少行」，在**「一个类独占多少行」**。RustDesk 那个 3376 行的设置页里，
每个设置分组是一个独立 `StatefulWidget`（`_General` 600 行 / `_Safety` 759 行 / `_Display` 266 行 /
`_Account` 85 行），壳本身只有 280 行。我们相当于把人家 5 个类塞成了一个。

出处（补功能前先看这里）：
- `refs/upstream/rustdesk/flutter/lib/desktop/pages/desktop_setting_page.dart:399-2344`
- `refs/外部-归档软件/LocalSend/app/lib/widget/`（`big_button.dart` / `custom_progress_bar.dart` / `list_tile/`）

### 规范（后续所有界面改动照这个走）

1. **一个视图块 = 一个独立 widget 类**，目标体积 **300–800 行**（对齐 RustDesk 的实际值）。
2. **只收它真正需要的**：构造参数 + 回调，不传整个 State。对照：`_previewView` 引用了 38 个不同成员，
   这是下一块要解决的问题；`_queueView` 只用了 7 个，所以先拆它。
3. **小部件进 `gui/lib/util/` 或独立文件**（如 `format.dart`），不重复实现 ——
   `formatBytes` 原来在首页 / 打包页 / 历史页各写一份，现在首页那份已接到 `format.dart`。
4. **不用 `part` 拆类**：实测（2026-11-01）Dart 的 analyzer **不允许类的花括号跨文件**
   （试拆 `_shortPath` + 类闭合 → 309 个错误），`part` 只能放**顶层**内容
   （LocalSend 也是这么用的：`pages/about/packagers.dart` 里是 `const _packagers = {...}` 顶层数据）。
5. **每块独立提交、独立回滚**，验收 = `flutter analyze` 干净 + `flutter test` 全绿 +
   **反向验证一次**（把功能改坏看测试是否变红）+ 用户真机点一遍。

### 进度（13 块）

| # | 块 | 原行数 | 引用成员数 | 状态 |
|---|---|---|---|---|
| 1 | `_queueView` + `_queueStatusChip` | 213 | 7 | ✅ **已拆** → `pages/home_queue_view.dart`（`QueueItem` / `QueueView` / `_QueueStatusChip`），键盘 Enter / Ctrl+E / Del / ↑↓ |
| 2 | `_batchDoneView`（批量完成卡） | 92 | 4 | ✅ **已拆** → `pages/home_batch_done_view.dart`（`BatchDoneView`）。★这张卡**原来一条测试都没有**，抽出来后补了 5 条（成功/失败/点击定位/打开目录/无目录不显示按钮） |
| 3 | `_emptyView`（拖放区） | 62 | 3 | ✅ **已拆** → `pages/home_empty_view.dart`（`EmptyDropView`）。补 4 条测试，把两条只写在注释里的设计约定钉死：拖拽中必须换「松手就行」、第一屏只许一个 `FilledButton` |
| 4 | `_busyView` | 13 | 1 | ✅ **已拆** → `pages/home_busy_view.dart`（`BusyView`）。零状态依赖；注释里写清「转圈是无限动画，测试只能用定次 pump」 |
| 5a | `_crumbBar`（面包屑路径条） | 38 | 3 | ✅ **已拆** → `pages/preview_crumb_bar.dart`（`CrumbBar`）。★顺手修了「包名取法只认 `/`」的 Windows bug（改用 `baseName()`），补 5 条测试 |
| 5b+5c | `_fileList` + `_entryRow`（清单 + 每行） | 77+116 | 6+4 | ✅ **已拆（两块合成一块）** → `pages/preview_file_list.dart`（`FileListView`）。**4 处 setState 改成 3 个回调，状态全留在父层** —— `_picked` 要传给引擎、`_crumb` 要算缩进基准和面包屑，下沉就变成两份真相。补 7 条交互测试 |
| 5d-1 | `_miniBtn` + `_pickedBytes` + `_smartDirHint`（纯零件） | 19+8+11 | 3 | ✅ **已拆** → `pages/preview_parts.dart`（`MiniButton` / `pickedBytes` / `smartDirHint`）。★两个纯函数从「必须有整页才能测」变成 10 条单测就能钉死 |
| 5d-2 | `_previewView` 主体 | 325 | 26 | ✅ **已拆 4 段** → `preview_password_box.dart`（A 密码区）/ `preview_pick_card.dart`（B 勾选卡）/ `preview_header.dart`（C 备注 + D 摘要）。主体 **340 → 236 行**，剩下是组装层 + 多候选提示 / 智能解压预告 / 工具行 / 底部按钮。★「25 个参数」不用解决 —— 那是把整块当一个单位才有的问题，分段后自然消失 |
| 6 | `_progressView` + `_flushView` | 115+60 | 17+5 | ✅ **已拆** → `home_progress_view.dart`（`ExtractProgressBar` / `ExtractStatusLines` / `ExtractActionRow` + 顶层纯函数 `etaText`）+ `home_flush_view.dart`（`FlushView`）。`_progressView` 主体 115 → 50 行（组装层）。★`etaText` 原来读 `_paused` / `_speed` 两个 State 字段，改成带参数的纯函数后才可单测 |
| 7 | `_doneView`（单个文件的完成屏） | 164 | 12 | ✅ **已拆** → `pages/home_done_view.dart`（`DoneView`，10 个参数：`result` / `verifyMode` / `nestedArchives` / `notes` / `fileName` / `password` / `entryHint` + 3 回调）。★**别和第 2 块的 `BatchDoneView` 混淆**：那个是整批总账，这个是单文件的一张卡。10 条测试钉住三个真机踩过的坑（校验模式不能给「打开目录」、只勾封面时解出 0 个文件是正常的、嵌套包不自动解） |
| 8 | `_errorView` + `_spaceShortageBlock` | 200+62 | 16+3 | ✅ **已拆** → `home_space_shortage.dart`（`spaceShortageBlock` + 纯函数 `spaceLine`）+ `home_error_view.dart`（`ErrorHead` / `ErrorPasswordRetry`）。失败页 200 → **75 行组装层**。★错误页点常用码是「填入+立刻重试」，预览页只是填入 —— 两处别混 |
| 9 | `_scaffold` + `_body` | 41+13 | — | ✅ **只抽 AppBar** → `pages/home_app_bar.dart`（`HomeAppBar`）。★**`_scaffold` / `_body` 故意不抽**：它们是骨架 + 相位分派（`_body` 一个 switch 调八个视图方法），抽出去只能变成「传八个 Widget 参数」的搬运机 —— 判据看引用成员数，不看行数 |
| 10 | 纯函数区（`_kindName` / `_shortPath` / `_typeHint` / `_entryHint`） | ~85 | 0（纯函数） | ✅ **已拆** → `pages/home_hints.dart`（`kindName` / `typeHint` / `entryHint` / `shortPath`）。★顺手修了 `shortPath` 只认 `/` 的 Windows bug（改 `[/\\]`），和 5a 面包屑那个是同一类 |

> 顺序按「引用成员数从小到大」，不按行数 —— 耦合少的好拆、好验。`_previewView` 放中间，
> 因为它需要先决定「哪些状态留在 State、哪些下沉」，是唯一需要设计的一块。

**★ 本批结果（13/13 全部拆完）**：`home_page.dart` **3117 → 1790 行（-42.6%）**、
测试 **213 → 337 条**、`flutter analyze` 干净。新文件 12 个（`pages/` 下 11 个 + `pages/home_hints.dart`）。

**没拆的两处（故意的，不是漏了）**：
- `_body()` 的相位分派（8 个分支调 8 个视图方法）—— 抽出去就是「传 8 个 Widget 参数」；
- `_miniBtn` 之类已经被更早的块吃掉（5d-1 的 `MiniButton`），表格里不再重复列。

**下一批的入口**：`_pick` / `_open` / `_extract` / `_scan` 这些**动作方法**还没动（它们改状态、
调引擎、跳相位），属于「逻辑」而不是「视图」，拆它们要先把引擎客户端抽成可注入的接口。

### 已做的第 1 块留下的两条经验

- **抽出独立 widget 最大的好处是可测性**：`QueueView` 只依赖「一个列表 + 六个回调」，
  测试里构造假队列 + 计数回调就能验键盘，**不用起假引擎、不碰文件 I/O**，也不会在假时钟里留 pending Timer。
- **键位不要自己拍**：Enter 来自 `docs/UI-SPEC.md` §4b.6「Enter 确认」；`Ctrl+E` 对齐 PeaZip
  （`refs/外部-归档软件/PeaZip/peazip-sources/res/share/lang/en.txt:1414`）。加任何新快捷键前先查这两处。
