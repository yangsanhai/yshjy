# 参考索引：设计项 → 参考出处 → 代码落点

> 用途：**补任何一个功能前，先在这张表里查它**。查到就知道：设计怎么定的、
> 去 `refs/` 哪个文件哪一行看参考实现、抄什么、改我们哪个文件。
> 状态：✅ 已实现 · 🟡 部分 · ❌ 未做（括号里是计划批次）

---

## 0. 三类锚点怎么读

| 锚点形态 | 含义 | 例子 |
|---|---|---|
| `refs/analysis/07-竞品界面调研.md` §3.3 | 竞品**界面调研**的章节 | Bandizip 的「可学习点」 |
| `refs/tech/xadmaster.md:28`（条目 ①-02） | **技术台账**条目：每条都带参考源码的精确位置 | 见下 |
| `refs/sevenz-rust/src/encryption/password.rs:52-61` | **参考项目源码**真身（可直接打开读） | 密码转 UTF-16LE 的实现 |

台账条目本身就写明了源码位置，所以看到一个条目号，就等于拿到源码路径。
例：`refs/tech/xadmaster.md:28` 的 ①-02 写着 `XADZipParser.m:118-191`。

---

## 1. 解压核心（引擎）

| 设计项 | 设计出处 | 台账锚点 | 参考源码 | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|---|---|
| **密码多编码重试** | DESIGN §17.1 | `tech/对比-测试构建与编码.md:68`（2.1.13） | `sevenz-rust/src/encryption/password.rs:52-61` | 密码转 UTF-16LE；候选按编码展开逐个试 | `src/extract.rs:172,349`、`src/extract_rar.rs` | ❌ 第20批 |
| **Unicode Path 0x7075** | §17.3 | `tech/对比-测试构建与编码.md:64`（2.1.9） | `zip2/src/extra_fields/zipinfo_utf8.rs:1-96` | `UnicodeExtraField` + `unwrap_valid`（CRC 校验后采用） | `src/layout/zip.rs:436` `decode_name` | ❌ 第21批 |
| **EOCD 分块倒扫** | §17.6 / AUDIT | `tech/xadmaster.md:28`（①-02） | `XADZipParser.m:118-191`（图解 :176-183） | 64 KiB 一块从尾往前，每块留 20 字节重叠 | `src/layout/zip.rs:41,131` | ❌ 第21批 |
| **全文件签名扫描** | §13 武器① | `tech/7zip.md:45`（2.6） | `Archive/Zip/ZipIn.cpp:618,723-756` | `FindPK_4` + 内容校验（强判定） | `src/job.rs`、`src/scan.rs` | ✅ |
| **LOCALS 无目录兜底** | §13 武器③ | `tech/7zip.md:113`（5.1）、`:116`（5.4） | `Archive/Zip/ZipIn.cpp:2530,2735-2742` | CD 读出来对不上 → 退回扫 local header 重建 | `src/rescue.rs` | ✅ |
| **分段虚接（Concat）** | §13 武器② | `tech/7zip.md:10`、`tech/README.md:120` | `7zip/CPP/7zip/Common/StreamBinder.cpp` 思路 | 多段拼成一条可 seek 的逻辑流 | `src/unpack.rs`、`src/extract7z.rs` | 🟡 主路径有，E3 未全覆盖 |
| **结构校验防假签名** | §13 武器④ | `tech/7zip-工程层.md:50,54`（1.11/1.15） | `CPP/7zip/UI/Common/OpenArchive.cpp` | 1 MiB 头缓冲 → 候选队列 → 实际解析验证 | `src/scan.rs` | ✅ |
| **7z 头加密 vs 内容加密** | §17.4 | `tech/sevenz-rust+steganographier.md:51`（J-15） | `sevenz-rust/src/reader.rs:381-423` | 内容加密头明文可列表；头加密直接要密码 | `src/preview.rs:322`（现硬编码 `false`） | 🟡 第20批 |
| **双维度识别进度** | §17.6（D1） | `tech/7zip-工程层.md:91`（3.3） | `CPP/7zip/Archive/IArchive.h:177-181` | `SetTotal(files, bytes)` 两个维度 | `src/scan.rs`（无 ctx） | ❌ 第21批 |
| **嵌套包下钻一层** | §17.6 | `tech/xadmaster.md:91,92`（③-09/③-10） | XADMaster 递归解压 | 继承父级密码与编码，只下一层 | `src/drill.rs`（`extract.drill`） | ✅ 21批组C |
| **解压前内存预检** | §17.6 | `tech/对比-同名技术择优.md:306` | 7-Zip `MemoryRequest` | 按字典×块×线程估峰值，跟可用内存比 | `src/hardware.rs` | ❌ 第22批 |
| **炸弹限额改成询问** | §17.7 | `tech/7zip-工程层.md:77`（2.9） | 7-Zip 的预算协商 | 抛结构化请求（required/allowed），界面问一句 | `src/ask.rs` + `src/safety.rs` + `serve.rs` 的 answer | ✅ 21批组B |
| **条目 packSize 校验** | AUDIT §3-12 | `tech/7zip.md:68`（3.5） | 7-Zip 的 dataAfterEnd 判定 | 读满判定，多出/不足都报「拼接错位」 | `src/layout/zip.rs:58` | ❌ 第22批 |

---

## 2. 界面（GUI）

| 设计项 | 设计出处 | 参考锚点 | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|---|
| 智能解压三规则 | §3 补 / P2 | `analysis/07` §3.2 | Bandizip 的三个判断（单目录/单文件/散文件） | `gui/lib/pages/home_page.dart:619-627` 只写了说明 | ✅ 引擎侧 / 🟡 界面可切换 |
| 批量队列 + 三段式完成卡 | P1 | `analysis/07` §9.1（Sunpack `f08.png`） | 结果标题 / 量化副标题 / 直达按钮 | `home_page.dart:727-811` | ✅ |
| 失败卡可执行化 | P3 | `analysis/07` §1.3（v2 `print_final_completion`） | 「可能原因清单」升级成按钮 | `home_page.dart:474-503` | ✅ |
| 密码卡三件套 | P4 | `analysis/07` §4.1（眼睛图标）、§9.2（优先级五档）、§9.4（近期密码/不保存） | 眼睛切换、近期密码 chips、不保存勾选 | `home_page.dart:1260`（裸 obscureText） | 🟡 第20批 |
| 试密码可视化 + 速度/ETA | P4 | `analysis/07` §9.4、`:248`（要超过 Bandizip） | 「正在试第 3/12 个」、速度、剩余时间 | `home_page.dart:1017-1078`（只有已传/总量） | ❌ 第20批 |
| 输出目录 + 原包处理 | P5 / §17.2 | `analysis/07` §234-238 | Bandizip 路径对话框 + 安全默认三件套 | `home_page.dart:344`（不传 dest） | ❌ 第20批 |
| 设置页可写 | P5 | — | 路径可改、语言可选 | `settings_page.dart` | 🟡 |
| 预览树 + 部分解压 | P7 | `analysis/07` §7（PeaZip 结构树） | 树形结构 + 勾选 + 面包屑 | 预览页平铺 | ❌ 第22批 |
| 完成卡分享文案 | P8 | `analysis/07` §952-954、§1.3 | 「复制给对方：这是伪装包，用…打开」 | 完成卡无此按钮 | ❌ 第22批 |
| 主题 + 窗口尺寸记忆 | P10 | `analysis/07` §4.4、§7.3 | 跟随系统/深浅、记住尺寸 | — | ❌ 第23批 |
| 新手引导 | P11 | `analysis/07` §5.1（隐写者 ①–⑪ 编号步骤） | 三步浮层 | — | ❌ 第23批 |
| 历史页可点开文件 | — | `analysis/07` §130-131（别清空历史）、§135 | 条目可打开所在目录 | `history_page.dart:102-121`（无 onTap） | ❌ 第20批顺带 |

---

## 3. 打包（伪装）

| 设计项 | 设计出处 | 参考锚点 | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|---|
| 尾部拼接 / free 原子布局 | §14.1 / §15 | `analysis/steganographier-hide.md` §2.1、§2.2 | 字节级布局图（普通模式 vs ZArchiver 模式） | `src/pack/` | ✅ |
| 布局兼容性矩阵 | §15 | `analysis/05-压缩布局验证.md` §2.1/§2.2/§2.3 | 16 个样本的实测矩阵（含最意外的 L5 分散多段） | `src/pack/` | ✅ 尾部 + free 原子 |
| 分卷生成 | §14.2 | `analysis/steganographier-hide.md` §Q4（分卷怎么处理） | `.001/.002` 命名与切分 | `--split` 未实现 | ❌ 第20批 |
| 密码内嵌（自动带码） | §14.3 | `analysis/steganographier-hide.md` §Q5（密码怎么设） | 打包时写进说明书，解压端自动抓 | `src/pack/`、`src/password_hint.rs` | ✅ |
| 封面嵌入 / 素材 | §14.2 | `tech/README.md:109` | `Steganographier.py:1843-1889` 内置封面 | `src/pack/cover.rs` | ✅ 打包侧 |
| 说明书 + 校验和 | §14.2 | `analysis/steganographier-hide.md` §Q6 | 输出命名 / 说明书 / 大小-时长校验 | 部分 | 🟡 第20批 |
| 改 MP4 atom 保证封面可播 | §14.1 | `analysis/steganographier-hide.md` §Q3 | 改 atom 的具体做法 | `src/pack/` | ✅ |
| 干扰数据 / 假签名 | §14.2 | `analysis/05` §1.3（资源红线）、隐写者 §八 | 垃圾段、伪特征码 | 无 | ❌ 第22批 |
| **封面分离（解压侧）** | §3 / §17.5 | `tech/README.md:109`（同一套封面知识） | 反着切：`[0, 压缩层起点)` 写成可播文件 | `src/cover_extract.rs` | ✅ 解压侧 + 21批加 `list.cover` 预探 |

---

## 4. 极端场景 E1–E6（§13）

| 场景 | 结构 | 靠哪件武器 | 台账锚点 | 状态 |
|---|---|---|---|---|
| E1 前后超长垃圾 | `[垃圾][包][垃圾]` | 全文件签名扫描 | `tech/7zip.md:45` | ✅ |
| E2 包插在视频中间 | `[视频前][包][视频后]` | 全文件扫描 + 虚接 | `tech/7zip.md:45`、`:10` | ✅ |
| E3 包被视频隔开 | `[包前][视频][包后]` | 分段虚接 | `tech/7zip.md:10` | 🟡 需补测试 |
| E4 中间段真丢 | 数据缺失 | 救援模式 | `tech/7zip.md:113`（LOCALS） | ✅ 救援 |
| E5 头尾被切 | 截断 | 救援模式 | 同上 | ✅ 救援 |
| E6 一个文件多个包 | `[包A][包B]` | 全文件扫描 + 逐候选验证 | `tech/7zip-工程层.md:50,54` | ✅ |

---

## 5. 全平台（§18）

| 项 | 参考锚点 | 抄什么 | 落点 | 状态 |
|---|---|---|---|---|
| 三层暴露（core/ffi/json） | `docs/BACKEND-SPEC.md` §1.2 | 结构分层 | core/json ✅ + ffi ✅（`ffi/`） | ✅ |
| FFI 约束（简单类型/事件流/TaskHandle） | `docs/BACKEND-SPEC.md` §1.3 | 接口草案 | `ffi/src/lib.rs` + `include/yshjy.h`（JSON 进出、事件回调、Job 句柄） | ✅ 21批组D |
| Android 桥接 | `BACKEND-SPEC:29`（flutter_rust_bridge） | 编译 `.so` | `libyshjy_ffi.so` 可出；桥接层待接 | 🟡 地基已就绪 |
| iOS 桥接 | `BACKEND-SPEC:30`（静态库） | 静态库进 Xcode | `libyshjy_ffi.a` 可出；`FfiTransport` 待写 | 🟡 地基已就绪 |
| 移动端 seek 三档 | `docs/ARCHITECTURE.md` F1 | 直接 fd / 块缓存 / 先复制 | 待做 | ❌ |
| 传输层抽象 | DESIGN §18.1 | `EngineTransport` 接口 | `gui/lib/transport/engine_transport.dart`（ProcessTransport 已实现） | ✅ 21批组D |

---

## 6. 反面教材：明确不抄（附出处）

| 项目 | 出处 | 别抄什么 |
|---|---|---|
| Complex-unzip-tool-v2 | `analysis/07` §1.4 | 明文打印密码、`console.clear()`、静默隔离失败件 |
| TR-Supowe | `analysis/07` §2.4、§157 | 回车 → 1/2 → 编号的三段式菜单；异常串直接抛给用户 |
| Bandizip | `analysis/07` §3.4 | Windows 专属右键菜单结构（Linux 不适用） |
| ZArchiver | `analysis/07` §4.4 | FAB 一钮三义、没有回收站 |
| 隐写者 | `analysis/07` §5.5 | 11 元素平铺无焦点、能力缺陷写进菜单项、密码 ascii 过滤 |
| OnlyUnzip | `analysis/07` §9.2 | 12 条功能平铺、无过程图 |
| PeaZip | `analysis/07` §7 | 双栏（我们窗口小）、语言切换藏深层菜单 |
| 7-Zip | `analysis/07` §8 | 下拉组合框地址栏、零视觉设计 |
| 扩展名加权排序 | `tech/7zip.md:55`（2.16） | 我们的主场景就是扩展名撒谎，加权会排错 |
| 进程级降优先级 + 忙等 | `tech/7zip-工程层.md:79`（2.11） | 影响并发任务，已有 CancelToken |
| cgroup 内存限额探测 | `tech/7zip-工程层.md:76` | 桌面单机过度设计 |
| 解析外部 CLI `-slt` 输出 | `tech/peazip.md:25-30` | 我们走自研解码器，不适用 |

---

## 8. 终端 UI（CLI 外壳）与「解完删源」

> 2026-10-28 补：用户要求「别纯手搓，先看别人怎么做」。标杆项目已拉到
> **`refs/upstream/`**（`--depth 1`，共 22 MB）：`indicatif` / `inquire` / `dialoguer` /
> `ratatui` / `comfy-table`。下面是逐项对上我们的落点。

| 设计项 | 参考出处（本地真身） | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|
| **进度条模板** | `upstream/indicatif/src/style.rs:270-350`（模板变量：`{percent} {bytes} {total_bytes} {elapsed} {eta} {bytes_per_sec} {wide_msg}`）、`examples/download-speed.rs:10-12` | 模板变量集；`{wide_bar}` 自动铺满终端宽度；`progress_chars("#>-")` 指定填充字符；`{eta}` 由库自己算 | `src/main.rs` 的 `画进度行` | 🟡 手搓版已通（条/速度/剩余/项号），**缺 `{elapsed}` 已用时** |
| **多任务并发进度** | `upstream/indicatif/src/multi.rs`、`examples/multi-tree-ext.rs`、`examples/multi.rs`（截图见 README） | `MultiProgress` 给每个任务一条独立进度行，统一在顶部重绘 —— 我们 `-j 2` 时不再是两行互相覆盖 | 同上（需重写为 indicatif） | ❌ 未做 |
| **多选交互（`--pick`）** | `upstream/dialoguer/src/prompts/multi_select.rs:253-302`（键位：`↓/Tab/j` 下移、`↑/BackTab/k` 上移、`←/h` 上页、`→/l` 下页、`空格` 勾选、`a` 全选、`Esc/q` 取消、`Enter` 确认）；`upstream/inquire/KEY_BINDINGS.md`（键位文档写法）、`inquire/inquire/src/terminal/crossterm.rs:338-353`（终端后端抽象） | **别自己写 raw mode 循环**：直接用 `dialoguer::MultiSelect`，上面那套键位是业界共识；文档照 `KEY_BINDINGS.md` 的表格形式写一份中文的 | 新增 `src/pick.rs`，`--pick` 走它 | ❌ 未做 |
| **列表/表格输出** | `upstream/comfy-table/README.md:205,255-283`（宽度来源优先级：显式 `set_width` → `crossterm::terminal::size`；依赖 `unicode-width` 算中文宽度） | 列宽自适应算法；`--list` 的结果按「序号 / 大小 / 加密 / 路径」四列对齐，中文不歪 | `src/main.rs` 的 `--list` 输出 | ❌ 未做 |
| **解压后删除源压缩包** | `unzip -m`（move 语义，成功即删）；WinRAR 解压对话框「删除压缩文件：永不 / 总是 / 询问」；Bandizip 同功能 + 更安全的「移到回收站」变体；7-Zip 的 `-sdel` 是**压缩**后删源、解压侧没有该功能（p7zip issue #154 里官方建议自己 `rm`） | 「只在**完全成功**后删」「可改成移到回收站」「分卷要整组处理」「删之前说清不可恢复」 | 新增 `--delete-source` / `--trash-source`；界面解压页一个开关（默认关） | ❌ 未做 |

**两条教训（写在这里，免得下次又手搓）**：
1. 交互式终端界面（raw mode、分页、过滤、非 TTY 降级、退出复原）是**成堆的边界情况**，
   `dialoguer` / `inquire` 已经把它们全覆盖了 —— 手写 60 行只是把坑推迟到用户机器上。
2. 进度条同理，但我们有一个额外约束：**ETA 要复用引擎里那个 `EtaEstimator`**（α=0.3、样本 <3 不给估算），
   否则 CLI 和界面显示的剩余时间会不一致。用 indicatif 时只能用它的 `set_position`，ETA 自己塞进 `{msg}`。

---

## 7. 台账维护

`refs/tech/` 共 383 处 🔄，其中约三成其实已实现（例：`7zip-工程层.md:131-133`
退出码分档、`:154` ETA 平滑、`:117` 系统语言回退、`7zip.md:80` ZipCrypto 校验字）。
补功能前若发现"要做的东西已经在了"，顺手把那条改成 ✅，避免重复劳动。
