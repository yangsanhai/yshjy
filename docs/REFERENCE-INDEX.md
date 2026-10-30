# 参考索引：设计项 → 参考出处 → 代码落点

> 用途：**补任何一个功能前，先在这张表里查它**。查到就知道：设计怎么定的、
> 去 `refs/` 哪个文件哪一行看参考实现、抄什么、改我们哪个文件。
> 状态：✅ 已实现 · 🟡 部分 · ❌ 未做（括号里是计划批次）
>
> ✅ **2026-10-29：状态列已重写**。上一版有 **14 条失真**（标 ❌/🟡 但代码里早就做完了，
> 差点让人把已完成的批次重做一遍）。这 14 条现在**每条都带实测证据（文件:行）**，
> 是拿 `grep` 一条条核过的，不是照抄上一份审计结论 —— 复核过程见 `docs/TODO.md` 一期的第一项。
> 以后补功能前照样先查这张表；**发现「要做的东西已经在了」，就把状态改掉，别让它再漂一次**。

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
| **密码多编码重试** | DESIGN §17.1 | `tech/对比-测试构建与编码.md:68`（2.1.13） | `sevenz-rust/src/encryption/password.rs:52-61` | 密码转 UTF-16LE；候选按编码展开逐个试 | `src/extract.rs:190`、`src/extract_rar.rs` | ✅ 0.9.26 · `extract.rs:190 密码候选`（ASCII 短路；非 ASCII 展开 UTF-8→GBK→UTF-16LE） |
| **Unicode Path 0x7075** | §17.3 | `tech/对比-测试构建与编码.md:64`（2.1.9） | `zip2/src/extra_fields/zipinfo_utf8.rs:1-96` | `UnicodeExtraField` + `unwrap_valid`（CRC 校验后采用） | `src/layout/zip.rs:436` `decode_name` | ✅ 0.9.26 · `zip.rs:375-388`（读 extra，先验 CRC32 再采用） |
| **EOCD 分块倒扫** | §17.6 / AUDIT | `tech/xadmaster.md:28`（①-02） | `XADZipParser.m:118-191`（图解 :176-183） | 64 KiB 一块从尾往前，每块留 20 字节重叠 | `src/layout/zip.rs:41,131` | ✅ 0.9.26 · `zip.rs:149`（64 KiB 一块 + 20 B 重叠，按位置推进） |
| **全文件签名扫描** | §13 武器① | `tech/7zip.md:45`（2.6） | `Archive/Zip/ZipIn.cpp:618,723-756` | `FindPK_4` + 内容校验（强判定） | `src/job.rs`、`src/scan.rs` | ✅ |
| **LOCALS 无目录兜底** | §13 武器③ | `tech/7zip.md:113`（5.1）、`:116`（5.4） | `Archive/Zip/ZipIn.cpp:2530,2735-2742` | CD 读出来对不上 → 退回扫 local header 重建 | `src/rescue.rs` | ✅ |
| **分段虚接（Concat）** | §13 武器② | `tech/7zip.md:10`、`tech/README.md:120` | `7zip/CPP/7zip/Common/StreamBinder.cpp` 思路 | 多段拼成一条可 seek 的逻辑流 | `src/unpack.rs`、`src/extract7z.rs` | 🟡 主路径有，E3 未全覆盖 |
| **结构校验防假签名** | §13 武器④ | `tech/7zip-工程层.md:50,54`（1.11/1.15） | `CPP/7zip/UI/Common/OpenArchive.cpp` | 1 MiB 头缓冲 → 候选队列 → 实际解析验证 | `src/scan.rs` | ✅ |
| **7z 头加密 vs 内容加密** | §17.4 | `tech/sevenz-rust+steganographier.md:51`（J-15） | `sevenz-rust/src/reader.rs:381-423` | 内容加密头明文可列表；头加密直接要密码 | `src/preview.rs:322`（现硬编码 `false`） | 🟡 第20批 |
| **双维度识别进度** | §17.6（D1） | `tech/7zip-工程层.md:91`（3.3） | `CPP/7zip/Archive/IArchive.h:177-181` | `SetTotal(files, bytes)` 两个维度 | `src/job.rs:311` | ✅ 0.9.26 · `job.rs:311` emit `ScanProgress{total_bytes,total_files}` |
| **嵌套包下钻一层** | §17.6 | `tech/xadmaster.md:91,92`（③-09/③-10） | XADMaster 递归解压 | 继承父级密码与编码，只下一层 | `src/drill.rs`（`extract.drill`） | ✅ 21批组C |
| **解压前内存预检** | §17.6 | `tech/对比-同名技术择优.md:306` | 7-Zip `MemoryRequest` | 按字典×块×线程估峰值，跟可用内存比 | `src/hardware.rs:179`、`src/extract7z.rs:239` | ✅ 0.9.26 · `extract7z.rs:239 内存预检`（字典峰值 + 256 MB 预留 vs 可用内存；仅 7z 需要） |
| **炸弹限额改成询问** | §17.7 | `tech/7zip-工程层.md:77`（2.9） | 7-Zip 的预算协商 | 抛结构化请求（required/allowed），界面问一句 | `src/ask.rs` + `src/safety.rs` + `serve.rs` 的 answer | ✅ 21批组B |
| **条目 packSize 校验** | AUDIT §3-12 | `tech/7zip.md:68`（3.5） | 7-Zip 的 dataAfterEnd 判定 | 读满判定，多出/不足都报「拼接错位」 | `src/layout/zip.rs:58` | ✅ **已实现**（`src/rescue.rs` 按 `packSize` 逐条校验，29 处） —— 2026-10-30 核实 |

---

## 2. 界面（GUI）

| 设计项 | 设计出处 | 参考锚点 | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|---|
| 智能解压三规则 | §3 补 / P2 | `analysis/07` §3.2 | Bandizip 的三个判断（单目录/单文件/散文件） | `gui/lib/pages/home_page.dart:619-627` 只写了说明 | ✅ 引擎侧 / 🟡 界面可切换 |
| 批量队列 + 三段式完成卡 | P1 | `analysis/07` §9.1（Sunpack `f08.png`） | 结果标题 / 量化副标题 / 直达按钮 | `home_page.dart:727-811` | ✅ |
| 失败卡可执行化 | P3 | `analysis/07` §1.3（v2 `print_final_completion`） | 「可能原因清单」升级成按钮 | `home_page.dart:474-503` | ✅ |
| 密码卡三件套 | P4 | `analysis/07` §4.1（眼睛图标）、§9.2（优先级五档）、§9.4（近期密码/不保存） | 眼睛切换、近期密码 chips、不保存勾选 | `home_page.dart:1260`（裸 obscureText） | 🟡 第20批 |
| 试密码可视化 + 速度/ETA | P4 | `analysis/07` §9.4、`:248`（要超过 Bandizip） | 「正在试第 3/12 个」、速度、剩余时间 | `home_page.dart:198,1017-1078` | ✅ 0.9.26 · `home_page.dart:198` 吃 `PASSWORD_TRY`（「正在试第 i/M 个」+ 速度/ETA） |
| 输出目录 + 原包处理 | P5 / §17.2 | `analysis/07` §234-238 | Bandizip 路径对话框 + 安全默认三件套 | `home_page.dart:556,617,810` | ✅ 0.9.26 · 输出目录 `dest: _outDir`（`home_page.dart:556,617,810`）+ 原包处理（设置页 keep/trash，`config.rs:155`） |
| 设置页可写 | P5 | — | 路径可改、语言可选 | `settings_page.dart` | 🟡 |
| 预览树 + 部分解压 | P7 | `analysis/07` §7（PeaZip 结构树） | 树形结构 + 勾选 + 面包屑 | 预览页平铺 | ✅ **已实现**（`gui/lib/entry_tree.dart` 33 处 + `select` 已接线） —— 2026-10-30 核实 |
| 完成卡分享文案 | P8 | `analysis/07` §952-954、§1.3 | 「复制给对方：这是伪装包，用…打开」 | 完成卡无此按钮 | ✅ 0.9.28 · `home_page.dart:1836`「复制解压方法」 |
| 主题 + 窗口尺寸记忆 | P10 | `analysis/07` §4.4、§7.3 | 跟随系统/深浅、记住尺寸 | `theme_mode.dart`（尺寸记忆未做） | 🟡 主题已做（`theme_mode.dart` / `main.dart:69`，设置页可切）；**窗口尺寸/位置记忆未做**（要引 `window_manager`，见 `docs/TODO.md` 第二期） |
| 新手引导 | P11 | `analysis/07` §5.1（隐写者 ①–⑪ 编号步骤） | 三步浮层 | — | ✅ 0.9.26 · `app_shell.dart:114` 三步引导（只弹一次，标记写在 state 目录） |
| 历史页可点开文件 | — | `analysis/07` §130-131（别清空历史）、§135 | 条目可打开所在目录 | `history_page.dart:128` | ✅ 0.9.26 · `history_page.dart:128` onTap 定位到输出目录 |

---

## 3. 打包（伪装）

| 设计项 | 设计出处 | 参考锚点 | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|---|
| 尾部拼接 / free 原子布局 | §14.1 / §15 | `analysis/steganographier-hide.md` §2.1、§2.2 | 字节级布局图（普通模式 vs ZArchiver 模式） | `src/pack/` | ✅ |
| 布局兼容性矩阵 | §15 | `analysis/05-压缩布局验证.md` §2.1/§2.2/§2.3 | 16 个样本的实测矩阵（含最意外的 L5 分散多段） | `src/pack/` | ✅ 尾部 + free 原子 |
| 分卷生成 | §14.2 | `analysis/steganographier-hide.md` §Q4（分卷怎么处理） | `.001/.002` 命名与切分 | `src/pack_post.rs:83`、`--split` | ✅ 0.9.26 · `pack_post.rs:83 split_file` + CLI `--split`（`cli_spec.rs:77`） |
| 密码内嵌（自动带码） | §14.3 | `analysis/steganographier-hide.md` §Q5（密码怎么设） | 打包时写进说明书，解压端自动抓 | `src/pack/`、`src/password_hint.rs` | ✅ |
| 封面嵌入 / 素材 | §14.2 | `tech/README.md:109` | `Steganographier.py:1843-1889` 内置封面 | `src/pack/cover.rs` | ✅ 打包侧 |
| 说明书 + 校验和 | §14.2 | `analysis/steganographier-hide.md` §Q6 | 输出命名 / 说明书 / 大小-时长校验 | 部分 | 🟡 第20批 |
| 改 MP4 atom 保证封面可播 | §14.1 | `analysis/steganographier-hide.md` §Q3 | 改 atom 的具体做法 | `src/pack/` | ✅ |
| 干扰数据 / 假签名 | §14.2 | `analysis/05` §1.3（资源红线）、隐写者 §八 | 垃圾段、伪特征码 | 无 | ✅ **已实现**（`src/pack/decoy.rs` + CLI `--decoy*` + `tests/decoy.sh`） —— 2026-10-30 核实 |
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
| **多任务并发进度** | `upstream/indicatif/src/multi.rs`、`examples/multi-tree-ext.rs`、`examples/multi.rs`（截图见 README） | `MultiProgress` 给每个任务一条独立进度行，统一在顶部重绘 —— 我们 `-j 2` 时不再是两行互相覆盖 | `main.rs:1238-1260` | ✅ 0.9.27 · `main.rs:1246` indicatif `MultiProgress`（延迟到真的出现第二个任务才切） |
| **多选交互（`--pick`）** | `upstream/dialoguer/src/prompts/multi_select.rs:253-302`（键位：`↓/Tab/j` 下移、`↑/BackTab/k` 上移、`←/h` 上页、`→/l` 下页、`空格` 勾选、`a` 全选、`Esc/q` 取消、`Enter` 确认）；`upstream/inquire/KEY_BINDINGS.md`（键位文档写法）、`inquire/inquire/src/terminal/crossterm.rs:338-353`（终端后端抽象） | **别自己写 raw mode 循环**：直接用 `dialoguer::MultiSelect`，上面那套键位是业界共识；文档照 `KEY_BINDINGS.md` 的表格形式写一份中文的 | `src/pick.rs`（0.9.27 已落地） | ✅ 0.9.27 · `src/pick.rs` + `cli_spec.rs:71`（dialoguer `MultiSelect`，键位照抄） |
| **列表/表格输出** | `upstream/comfy-table/README.md:205,255-283`（宽度来源优先级：显式 `set_width` → `crossterm::terminal::size`；依赖 `unicode-width` 算中文宽度） | 列宽自适应算法；`--list` 的结果按「序号 / 大小 / 加密 / 路径」四列对齐，中文不歪 | `src/main.rs:869-905` | ✅ 0.9.27 · `main.rs:872` comfy-table（中文按显示宽度对齐、窄终端折行） |
| **解压后删除源压缩包** | `unzip -m`（move 语义，成功即删）；WinRAR 解压对话框「删除压缩文件：永不 / 总是 / 询问」；Bandizip 同功能 + 更安全的「移到回收站」变体；7-Zip 的 `-sdel` 是**压缩**后删源、解压侧没有该功能（p7zip issue #154 里官方建议自己 `rm`） | 「只在**完全成功**后删」「可改成移到回收站」「分卷要整组处理」「删之前说清不可恢复」 | 新增 `--delete-source` / `--trash-source`；界面解压页一个开关（默认关） | ✅ **已实现**：CLI 两个参数（含二次确认）+ 界面设置页「原包处置」= `output.on_done`（`trash` / 保留）—— 2026-10-30 核实 |

**两条教训（写在这里，免得下次又手搓）**：
1. 交互式终端界面（raw mode、分页、过滤、非 TTY 降级、退出复原）是**成堆的边界情况**，
   `dialoguer` / `inquire` 已经把它们全覆盖了 —— 手写 60 行只是把坑推迟到用户机器上。
2. 进度条同理，但我们有一个额外约束：**ETA 要复用引擎里那个 `EtaEstimator`**（α=0.3、样本 <3 不给估算），
   否则 CLI 和界面显示的剩余时间会不一致。用 indicatif 时只能用它的 `set_position`，ETA 自己塞进 `{msg}`。

---

## 9. 全平台 GUI 与发布流水线（2026-10-28 第二批）

> 由来：用户定下「核心已完成、剩下是把全平台都能用起来」，要求先看业界怎么做再动手。
> 全部拉到 **`refs/upstream/`**（`--depth 1`；tauri 与 frb 用 `--sparse` 只取需要的目录，共约 260 MB）。

| 目录 | 来源 | 体量 | 为什么选它 |
|---|---|---|---|
| `rustdesk/` | rustdesk/rustdesk ⭐127k | 29M | Rust 核心 + Flutter 界面，Win/macOS/Linux/Android/iOS 全齐；打包脚本（`build.py` + `res/DEBIAN` + `msi/` + `PKGBUILD` + `osx-dist.sh`）与 CI 都在仓库里 |
| `localsend/` | localsend/localsend ⭐96k | 27M | Flutter 全平台 + `packages/localsend_isolates/rust_builder`；CI 拆成「一平台一个 reusable workflow + release.yml 汇总」 |
| `hiddify-app/` | hiddify/hiddify-app ⭐33k | 114M | Flutter + Rust 核心，**用 ffigen 直连 C ABI**（不是 flutter_rust_bridge）—— 跟我们 `ffi/` 的 JSON 进出设计同路 |
| `ouch/` | ouch-org/ouch ⭐3.8k | 1.1M | Rust 压缩/解压 CLI 标杆：clap derive + **构建期生成补全与 man 页** + 说人话的错误 |
| `tauri-bundler/` | tauri-apps/tauri（sparse） | 4.1M | 各平台安装包的权威实现：deb / rpm / AppImage(linuxdeploy) / dmg / msi / nsis |
| `cargo-dist/` | axodotdev/cargo-dist ⭐2.1k | 15M | 纯 Rust CLI 的发布流水线（shell/powershell/npm/homebrew 安装器 + CI 矩阵） |
| `fastforge/` | LeanFlutter/flutter_distributor（已改名 fastforge） | 21M | Flutter 桌面一键出 deb/rpm/AppImage/exe/dmg |
| `flutter_rust_bridge/` | fzyzcjy（sparse：codegen + docs + 集成示例） | 12M | 另一条 FFI 路（rustdesk 走这条），需要自动生成桥接时才看 |
| `clap/` | clap-rs/clap ⭐16.8k | 6.5M | `clap_complete` / `clap_mangen` 源码 |
| `trash-rs/` | Byron/trash-rs ⭐255（2026-10-30 拉） | 474K | 跨平台回收站：Windows 走 COM `IFileOperation`（`src/windows.rs:92`）。**W3 的唯一可抄来源**——我们 `src/lib.rs` 是 `#![forbid(unsafe_code)]`，自己写不了 COM |
| `time/` | time-rs/time（2026-10-30 拉） | 3.1M | Windows 本地时区偏移的正确写法：`src/sys/local_offset_at/windows.rs` **不碰 Bias，让 Windows 算**。**W1 照它改**（我们只取「现在」，可以再简化成 `GetLocalTime - GetSystemTime`） |

### 9.1 终端外壳（CLI）

| 设计项 | 参考出处（本地真身） | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|
| **补全与 man 页不手写** | `upstream/ouch/build.rs:20-39`（`include!("src/cli/args.rs")` 拿到同一份 clap 定义，`clap_mangen::generate_to` + `clap_complete::generate_to` 写进 `OUCH_ARTIFACTS_FOLDER`）、`upstream/ouch/Cargo.toml:77-79`（`clap_complete` / `clap_complete_nushell` / `clap_mangen`） | 补全和 man **由 clap 定义在构建期生成**，打包时塞进 deb；`scripts/completions/` 手写副本整个退休 | `scripts/gen-cli-assets.py` + `src/cli_spec.rs` | ✅ 0.9.27/0.9.28 · `scripts/gen-cli-assets.py` 从 `src/cli_spec.rs` 生成（自研生成脚本，不是 clap 构建期；同样达到「不手写、不漂移」） |
| 表格输出 | comfy-table（§8 已有） | 列宽自适应 | `--list` | ✅ 0.9.27（见 §8「列表/表格输出」） |
| 多任务进度 | indicatif（§8 已有） | `MultiProgress` | `-j 2` | ✅ 0.9.27（见 §8「多任务并发进度」） |
| 交互多选 | dialoguer（§8 已有） | `MultiSelect` 键位 | `--pick` | ✅ 0.9.27（见 §8「多选交互」） |

### 9.2 全平台 GUI

| 设计项 | 参考出处（本地真身） | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|
| **移动端桥接走哪条路** | `upstream/hiddify-app/pubspec.yaml:197-203`（`ffigen:` 配置：`entry-points: hiddify-core/bin/desktop.h` → 输出 `lib/gen/hiddify_core_generated_bindings.dart`）、`:43`（`ffi: ^2.1.2`）、`:139`（`ffigen: ^19.1.0`）；对照 `upstream/rustdesk/build.py:200`（走 `flutter_rust_bridge_codegen` 从 Rust 侧生成 bridge） | **走 ffigen，直打我们已有的 `include/yshjy.h`**，不引 flutter_rust_bridge —— 我们 C ABI 已经是 JSON 进 / 事件出，桥就在 Dart 侧生成，引擎一行不用改 | 新增 `gui/lib/gen/yshjy_bindings.dart` + `FfiTransport` | ❌ 未做，**计划改了**（`docs/BACKEND-SPEC.md:29` 原写 flutter_rust_bridge） |
| Android 分区存储 | `upstream/localsend/app/lib/util/native/channel/android_channel.dart:10-11`（「Android 10+ 必须用 SAF 访问文件」）+ 全仓 SAF 用法 | 一律「App 私有目录直接写 / 外部目录走 SAF 授权后写」，**不做静默失败** | 移动端「解压到哪」的设计前提 | ❌ 待定设计（先定政策再写码） |
| 仓库布局 | `upstream/localsend/`（`app/` + `packages/…/rust_builder` + `server/`）、`upstream/hiddify-app/`（`android/ ios/ linux/ macos/ windows/` + `hiddify-core/`） | Rust 核心独立 crate + Flutter app 一层 + 每平台目录 | 我们 `src/` + `ffi/` + `gui/` 已经同构 | ✅ 已同构 |
| 桌面打包脚本 | `upstream/rustdesk/build.py:737`（deb）/`:849`（从已有二进制组 deb）/`:934`（dmg）/`:958`（Manjaro）/`:969`（Windows）；`upstream/rustdesk/res/DEBIAN/{preinst,postinst,prerm,postrm}` 四件套 | `postinst` 用 `$1 = configure` 分派、`preinst` 里先停服务、`postrm purge` 才删用户配置 —— **按参数分派**正是我们那个「postinst 写错 HOME」事故的根本教训 | `scripts/build-deb.sh`（补 prerm/postrm + 参数分派） | 🟡 部分 |

### 9.3 发布流水线

| 设计项 | 参考出处（本地真身） | 抄什么 | 我们的落点 | 状态 |
|---|---|---|---|---|
| **工作流怎么拆** | `upstream/localsend/.github/workflows/`：15 个平台文件（`build_linux_deb_x64.yml` …）+ `release.yml` 只做 `uses:` 汇总（文件头注释写明「各平台单独工作流与发版工作流不会漂移」）+ `winget.yml` | 一平台一个 reusable workflow，`release.yml` 只汇总；版本号各工作流自己从 `pubspec.yaml` 取 | 新建 `.github/workflows/` | ❌ 未做 |
| 单文件骨架 | `upstream/localsend/.github/workflows/build_linux_deb_x64.yml:1-45`（`workflow_call` + `ubuntu-22.04` 老基线 + 装 gtk3/ayatana + `subosito/flutter-action` + rust toolchain） | **出包别用 `ubuntu-latest`**：基线太新正是我们 AppImage 评估翻车的原因（本机 glibc 2.41）；22.04 是 glibc 2.35 | 同上 | ❌ |
| 纯 Rust CLI 发版 | `upstream/cargo-dist/dist-workspace.toml:1-20`（`installers = ["shell","powershell","npm","homebrew"]`、`targets = [...目标三元组...]`、`publish-jobs`） | 将来要让终端用户 `brew`/`npm` 一键装时用它，别手写安装脚本 | 暂不做 | ⏸ |
| 安装包规范 | `upstream/tauri-bundler/crates/tauri-bundler/src/bundle/linux/debian.rs:204-215`（`Depends` 只认配置里给的，**不自动推 ldd**）、`.../linux/appimage/linuxdeploy-plugin-gtk.sh`、`.../windows/nsis/`、`.../macos/dmg/` | 依赖闭包必须自己算（我们 `build-deb.sh` 已经这么做，方向是对的）；AppImage = linuxdeploy + gtk 插件 + 老基线 | `scripts/build-deb.sh` | ✅ 已对 |

**三条结论（写死，免得下次重新推演）**：
1. 终端那几件「补齐」活儿的本质是**删手写副本、换成 clap 派生**（补全 + man 页一次解决漂移）。
2. 移动端桥接**选 ffigen 不选 flutter_rust_bridge**：我们 `ffi/` 的 C ABI 已就绪，ffigen 只在 Dart 侧生成绑定；flutter_rust_bridge 要从 Rust 侧改导出层。
3. 出包 CI **必须钉老 Ubuntu 基线**（22.04），否则做出来的包在用户机器上跑不起来 —— 与 AppImage 那次评估同一个坑。

---

## 10. 覆盖对账：下载了的参考项目，哪些真拆过、哪些漏了（2026-10-28）

> 由来：用户问「原先拆过一遍，看看有没有漏的」。做法：把 `refs/` 下**下载了什么**与
> `analysis/` + `tech/` + `docs/` 里**写了什么**逐项对账。判定标准：
> **「拆过」= 有专门文档或带行号的台账**；只被提名字一律算「没拆」。

### 10.1 已系统拆解（不用再动）

| 参考项目 | 拆解文档 |
|---|---|
| SteganographierGUI（隐写者） | `analysis/steganographier-hide.md`、`-reveal.md`、`tech/sevenz-rust+steganographier.md`、`refs/README.md` §2 |
| zip2（zip-rs） | `analysis/zip2.md`、`tech/zip2.md` |
| 7zip | `analysis/7zip-scan.md`、`tech/7zip.md`、`tech/7zip-工程层.md` |
| XADMaster（unar） | `analysis/xadmaster.md`、`analysis/formats-unar.md`、`tech/xadmaster.md` |
| PeaZip | `tech/peazip.md`、`tech/peazip-工程层.md`、`analysis/07` §7 |
| sevenz-rust | `analysis/sevenz-rust.md`、`tech/sevenz-rust+steganographier.md` |
| 竞品界面 12 个（`ui-reference/`） | `analysis/07` §1–§9.3 **全有专门小节**（含 OnlyUnzip 截图抓不到这类缺口都记了） |
| indicatif / dialoguer / comfy-table | §8 台账（带源码行号） |
| hatchzip / delete_when_unzip / ReclaimArc | `docs/INPLACE-EXTRACT.md`（三档路线拆解） |

### 10.2 ✅ 原「漏的」9 项 —— **2026-10-28 全部补拆完毕**

> 补拆共产出 **5 份文档 / 9 条可落地结论 / 1 个真 bug**。判定标准不变：
> 「拆过」= 有专门文档或带行号的台账。

| # | 项目 | 拆解文档 | 最值钱的产出 |
|---|---|---|---|
| 1 | **clap_complete + clap_mangen** | `tech/clap-complete-mangen.md` | 🔴 **我们 bash 补全的真 bug**：没有 bash-completion 包时，带空格的文件名被劈成两个候选（实测 2 个 vs 期望 1 个）；修法 `IFS=$'\n'` 包 `compgen -f` |
| 2 | **hiddify-core 的 C ABI** | `tech/hiddify-core-c-abi.md` | 头文件是**构建产物**（不进仓）→ 我们手写 `ffi/include/yshjy.h` 有漂移风险，给了探针方案；另记 upstream 一处 use-after-free 别学 |
| 3 | **libarchive** | `tech/libarchive.md` | `entry_bytes_declared`（按头部声明大小给输出封顶）→ 直接对应我们待办「条目 packSize 校验」 |
| 4 | **Ark + FileRoller** | `tech/ark-fileroller.md` | 右键菜单**按目标目录可写性裁剪**（只读时不给「解压到此处」）；另确认 GNOME 官方也在从 CLI 后端迁到内建 libarchive |
| 5 | **Keka** | 同上文档 §1 | ❌ **无法拆：上游闭源**。仓库只有 `Bin/`（unar/lbzip2/lzip/plzip 四个外部 CLI）+ 34 种翻译，零源码 |
| 6 | **NanaZip** | `tech/keka-nanazip-inquire-etc.md` §2 | Core/Codec/Base 三层；`Sfx.Console`/`Sfx.Setup`/`Sfx.Shared` 三档自解压 |
| 7 | **inquire** | 同上 §3 | 🔴 **`--pick` 被 Ctrl-C 后光标不恢复的根因确认**：dialoguer 用 `console` 后端 → Ctrl+C 走 SIGINT；crossterm 后端会当普通按键读进来。多出「解法 B」 |
| 8 | **fastforge / cargo-dist** | 同上 §4/§5 | 打包流水线四段分层（分析/构建/打包/发布）；`x86_64-unknown-linux-musl` 是 glibc 基线问题的根治方向 |
| 9 | **flutter_rust_bridge** | 同上 §6 | 确认不选的 3 条理由（要改导出层 / 两套运行时 / 藏起 ABI 版本号） |

> `ratatui` 仍是有意排除（不做鼠标），不算漏。

### 10.3 顺手发现的两笔冗余（**没删，等用户点头**）

- `外部-归档软件/` 里的 **`LocalSend`** 与 **`RustDesk`** 是 2026-10-16 的浅克隆，与 `upstream/` 新拉的同名项目重复。
- `外部-归档软件/PeaZip` 与 `refs/PeaZip` 重复（后者才是被拆解的那份）。
- 合计约占 283 MB 里的一百多 MB；按项目铁律**先问不删**。
- 另：`外部-归档软件/clone.log` 里记的克隆路径是 `/home/user/hermes/studio/refs/`（后来整体移过来了），
  路径已过时，别拿它当准。

### 10.4 补拆过程中新发现的 4 个待办（**进 `docs/TODO.md`，不插队，加表尾**）

1. 🔴 `scripts/completions/yshjy.bash` 的 fallback 没有 IFS 保护（真 bug，实测见 `tech/clap-complete-mangen.md` §6.1）
2. 🟡 新增探针 `tests/ffi-header-sync.sh`（头文件与 `ffi/src/lib.rs` 双向对账）
3. 🟢 `complete` 那行补 `-o nosort -o bashdefault -o default`（带 bash 4.4 判断）
4. 🟢 `--test`（测试包完整性）作为独立能力 —— Ark 的 `SupportsTesting` 提醒的缺口

---

## 7. 台账维护

`refs/tech/` 共 383 处 🔄，其中约三成其实已实现（例：`7zip-工程层.md:131-133`
退出码分档、`:154` ETA 平滑、`:117` 系统语言回退、`7zip.md:80` ZipCrypto 校验字）。
补功能前若发现"要做的东西已经在了"，顺手把那条改成 ✅，避免重复劳动。
