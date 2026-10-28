# 待办清单（**唯一真源**）

> 项目里所有「还没做的事」只记在这一个文件，别处（HANDOFF / 计划文档）不再记待办，
> 只记历史与证据。**做完一项就在这划掉一行**，证据写进 `CHANGELOG.md`。
>
> 排序依据：**阶段 0 体检的实测证据**（详见 `docs/AUDIT-PHASE0.md`），
> 原则是「真人会撞上的概率优先」，不看实现难度。
> 每次开工只做最上面那一行的「下一项」，出现的新问题不插队，加到表尾。

最后更新：2026-10-28（黑屏重启后复验：`tests/all.sh` 24 通过 / 0 失败；表尾加一条偶发待查；
之后补：装机版本已核实为 **0.9.26**、帮助三条补勾、补全条目改成「构建期自动生成」并数准 35/18/17）

**当前状态**：**0.9.26 已出包并已装机**（`dpkg -l yshjy` = 0.9.26；`dist/yshjy_0.9.26_amd64.deb` 9.7M + 两档 tar.gz；
`scripts/test-packages.sh` **33 项全绿**）；`bash tests/all.sh` 24 通过 / 0 失败；
依赖漏洞已清零（cargo audit 0 漏洞 / cargo deny 四项全绿）。
**梯队四已全清，阶段 0 的 T5 也已完成（2026-10-28）** —— 0.9.26 的 deb 与 tar.gz 都在 `dist/`，验收 33 项全绿。
**当前该做的（2026-10-28 最终口径）**：**多平台适配整块排最后**，先清「优化建议」那节的清单
（终端 4 条 + 界面 2 条，见下文 §优化建议）。用户原话：「这个是最后做的，你先记录就行，我们还有一些事情没干」。

### ⏸ 最后做：多平台适配（整块延后；已探测清楚，到时候别重复调研）

| 平台 | 到时候要做什么 | 谁出包 | 本机现状（2026-10-28 实测） |
|---|---|---|---|
| Linux 终端 + 桌面 | ✅ 已完成 | 本机 | 0.9.26 已装机，`tests/all.sh` 24 通过 0 失败 |
| 安卓 | 装 SDK + NDK + **JDK 17**（现装 25 太新，apt 仓库暂无 17）→ `cargo-ndk` 编 `.so` → `ffigen` 生成 Dart 绑定 → `flutter build apk`；分区存储照 localsend：私有目录直写、外部走 SAF、**不许静默失败** | 本机 | SDK/NDK 都没装；磁盘余 74G（够，约吃 15–20G）；`dl.google.com` 直连 200 能下 |
| Windows | 命令行版：`cargo build --release --target x86_64-pc-windows-gnu`（目标 + mingw + wine 10 都已装，本机可验）；图形界面版只能在 Windows 上编 | **用户在 Win 上自己编** | 平台目录已铺 `gui/windows/`（未提交）；待写 `BUILD-Windows.md` |
| 苹果（macOS + iOS） | 「专属层」：`ffi/` C ABI → Dart 侧 ffigen 绑定 + `FfiTransport` → `ios/` + `macos/` 工程 → `BUILD-iOS.md`（免费 Apple ID 自签，7 天有效期可续） | **用户最后用 GitHub 编** | 平台目录已铺 `gui/ios/`、`gui/macos/`（未提交）；不买 99 美元账号、不上架、不公证 |

> 苹果口径（用户 2026-10-28 定，别再问）：不上架、不公证、不买开发者账号，发源码 / 未签名包，谁要谁自己签名 —— 花钱 0。
> **上 GitHub 前两件必修（已探测，别重查）**：① `.git` 300 MB，历史里有 `target-full/` 编译产物（最大 65 MB），
> 推之前必须清历史；② 个人路径 `/home/user` 在 `HANDOFF.md`、`refs/analysis/*.md`、
> `assets/sfx/yshjy-sfx.exe`（**二进制内嵌**）里，要脱敏。**无任何密钥泄露**（sk-/ghp_/AKIA/私钥格式零命中）。

下一步：把 §优化建议 那节从上往下清，一次一行。
> 已知小账（不阻塞发版）：RAR 在「库因 CRC 失败提前 drop writer」时，已写字节仍会落地；
> 彻底解决要迁 `rars::extract_with_control_and_errors` 重写回调层（见 CHANGELOG 第 12 项）。
> RAR 的中断实测已补上（装 `rar` 后加 `tests/rar_atomic.sh`，SIGKILL 打断 → 只剩 part、重试零副本）。

## 阶段 0：体检（✅ 已完成，证据在 `docs/AUDIT-PHASE0.md`）

| # | 项 | 结论 |
|---|---|---|
| T1 | 依赖安全 | ✅ 1 漏洞 + 1 yanked + 1 许可证缺口，已修并验证 |
| T2 | 测试覆盖 | ✅ 66.3%；协议层是真空洞（serve.rs 26.2% / api.rs 8.1%） |
| T3 | 性能压测 | ✅ 2 GB 单文件内存只 7 MB；10 万碎文件 8.6 秒 |
| T4 | 模糊测试 | ✅ 2.38 亿次零崩溃；写盘路径无 target（并入梯队三第 7 项） |
| T7 | 多语言 | ✅ 引擎 7 语全齐；界面未接多语言（架构决定，非缺陷） |
| T8 | 异常场景 | ✅ 路径穿越守住；中断留半截（见梯队一第 1 项）。**剩**：真实 ENOSPC / 拔盘需 sudo |
| T9 | 数据安全 | ✅ 不弄坏原有文件；假开关与文案问题已入梯队一、二 |
| T6 | 新手走一遍 | ◐ 用户已实机体验 0.9.25-c「都正常」；缺「下载 → 装 → 出错」完整首用链路 |
| T5 | 装包与发布检查 | ✅ **0.9.26 完成**：对外号去 `-c` 并五处同步 / deb 逐项核对 33 项全绿 / AppImage 评估＝本轮不做（配方见 `docs/RELEASE-CHECK.md`） |

## 阶段 1：要做（按梯队排，梯队一优先）

### 梯队一：✅ 三项全部做完（原子写入 / 错误原因说人话 / 重名策略接线）
> 详细证据见 `CHANGELOG.md` 的 0.9.26 三节，回归探针见 `tests/errmsgs.sh`、`tests/conflict.sh`

### 梯队二：✅ 两项全部做完（自动抓密码收窄 / 设置页语言文案）
> 详细证据见 `CHANGELOG.md` 的 0.9.26 后两节

### 梯队三：工程护栏（防以后改坏）✅ 三项全部做完
> 协议契约测试 / fuzz 写盘路径 / caps 能力位 —— 证据见 `CHANGELOG.md` 对应三节

### 梯队四：体验功能（按真人会撞到的频率排）

| # | 项 | 用户能感觉到什么 | 验收标准 |
|---|---|---|---|

### 待查（偶发问题，别当新功能做）

- [ ] `cargo test --release` 在 `tests/all.sh` 里**失败过一次**（2026-10-27 23:59，重启后首跑）。
  紧接着单跑 `cargo test --release`、`--quiet`、以及整轮 `all.sh` 复跑**全绿（334+12+2 / 24 通过 0 失败）**。
  所以暂不定性，怀疑是机械盘冷启动 I/O 慢触发的超时类测试或 OOM 杀进程。
  下次复现时先 `bash tests/all.sh > ~/.hermes/cache/scratch/all.log 2>&1` 留完整输出，**别再只 tail**（这次就是因为 tail 掉了现场，无从判断是哪一条）。

- [ ] **`--pick` 被 Ctrl-C 中断后光标不恢复**（2026-10-28 实测，pty 探针）：发 `\x03` 后进程以
  SIGINT 结束（rc=-2），屏幕上只有 `\x1b[?25l`（隐藏光标）没有对应的 `\x1b[?25h`，
  真实终端里光标会一直不显示，得 `reset` 才回来。
  根因：中断走信号路径，`dialoguer` 的复原代码没有执行机会。
  候选修法（按代价排）：① 不修，已在 `docs/CLI-SPEC.md` §4.5 写明「正常退出请用 Esc / q」；
  ② 引 `signal-hook` / `ctrlc`（MIT/Apache，在 `deny.toml` 白名单内）装一个 SIGINT handler，
  只做 `Show` 光标 + `disable_raw_mode` 后 `exit(130)`；③ 自己在 `src/pick.rs` 用 `rustix` 装 handler
  （不破 `forbid(unsafe_code)`）。**优先 ②**，但要先跑 `cargo deny check` 四项。

## 优化建议（终端 ↔ 界面对账出来的缺口，2026-10-28）

> 由来：用户问「终端命令你更新了吗、匹配功能了吗」。逐项对账后发现**两边各缺一半**：
> 引擎其实都能做，缺的是「一边做了、另一边没接线 / 没写出来」。
> 排法按「用户能感觉到」优先，一次只做一行。
> **施工步骤见 `docs/CLI-GUI-PLAN.md`**（怎么下手 / 文件落点 / 红线 / 每步验收）—— 本文只记「做没做」，那边只记「怎么做」，不重复记状态。

### 终端（CLI）侧

- [x] **进度仪表盘**（0.9.26，第二十二批）：条 + 百分比 + 字节 + 正在处理第几项 + 速度 + 剩余时间 + 当前文件名。
  复用引擎里现成的 `EtaEstimator`（α=0.3 平滑、样本 <3 不给估算）；窄终端按「文件名 → 速度/剩余 → 项号」
  的顺序丢字段，40 列也读得懂；中文按显示宽度截断。实测 1 GB 包在 100 列下画出
  `[██████░░░░░░░░░░░░] 34.8% 356.6 MB/1.00 GB 1/1 项 big.bin`，样本够了会接上 `177.0 MB/s … 剩 1 分 12 秒`。
  验收：单测 6 条（时长分档 / 中文截断 / 全字段 / 窄终端不超宽 / 总量未知不硬凑分母 / 不足 1 秒不写剩 0 秒）。
- [x] **进度条改用 indicatif**（参考 `refs/upstream/indicatif`，用户明确要求「别纯手搓」）✅ **已完成（2026-10-28）**
  **约束**（已满足）：剩余时间仍用引擎的 `EtaEstimator` 算好再塞进 `{msg}` —— 不许让 CLI 和界面显示两个不同的「剩余时间」。
  **落地**：进度行的文字段抽成共用函数 `进度消息()`（两条渲染路径同一份算法）；
  单任务仍走原来那条手绘 `\r` 路径（**一个字没改**，7 条单测原样全绿），
  只有**真出现第二个任务**才切到 indicatif `MultiProgress`，每任务一条独立行。
  **实测（pty 真跑）**：① `-j 2` 包A 14 帧 / 包B 21 帧，不再互相覆盖 ✅；
  ② 单任务输出与改前等价 ✅；③ 管道模式一行都不画 ✅。
  **踩到的坑（重要）**：indicatif 在 `TERM=dumb` 下会**静默退化成 hidden**
  （`draw_target.rs` 的 `is_dumb()` 一票否决，不报错不提示）→「一点进度都没有」。
  已加判断：`TERM=dumb` 时退回单行手绘，宁可单行也不沉默。
  Hermes 自己的终端执行环境正是 `TERM=dumb`，本地测试特别容易撞上这个坑。
  新探针 `tests/progress_multi.sh`（4 项）已挂进 `tests/all.sh`。
- [x] **`--pick` 全屏勾选**（用户点名要的「档 3」，**用 `dialoguer` 不手搓**）✅ **已完成（2026-10-28）**
  键位直接是业界共识那套（`refs/upstream/dialoguer/src/prompts/multi_select.rs:245-305`）：
  `↓/Tab/j` 下移、`↑/BackTab/k` 上移、`←/h` `→/l` 翻页、`空格` 勾选、`a` 全选、`Esc/q` 取消、`Enter` 确认。
  **落地**：新增 `src/pick.rs`（预览用 `NullSink`，零进度噪音）；`main.rs` 里 `--pick` 优先于 `--select`；
  非 TTY 报 `cli.pick_no_tty` 并以 USAGE(2) 退出；Esc/q 取消算成功（退出码 0，不解任何文件）。
  3 个 i18n 键 × 7 语言 + help 里一行 `--pick`。新探针 `tests/pick.sh`（6 项）已挂进 `tests/all.sh`：
  **验收①** 勾第 1、3 项与 `--select "1,3"` 解出的文件**逐字节一致**；
  **②** 管道里退出码 2 且提示改用 `--select`；**③** 中文文件名不错行；**④** Esc 取消零产物。
- [x] **`--list` 改表格** ✅ **已完成（2026-10-28）**：引 `comfy-table 8`（MIT，只多带一个
  `unicode-segmentation`），四列「序号 / 大小 / 加密 / 路径」，中文按显示宽度对齐。
  **只在真终端且 ≥50 列时画表格** —— 管道/重定向保持原来一行一条（脚本解析不受影响），
  窄于 50 列也退回老格式（实测 40 列时表格会把路径折成「大文件.b / in」）。
  表头 4 个 i18n 键 × 7 语言。实测 80/100 列中文对齐正确、40 列自动退回、管道输出与改前一致。
- [x] **中文键位表** ✅ **已完成（2026-10-28）**：`docs/CLI-SPEC.md` 新增 §4.5，
  照 `refs/upstream/inquire/KEY_BINDINGS.md` 的 `<kbd>` 表格形式，列全 8 组按键 + 边界行为
  （非 TTY 拒绝 / 一个都不勾 / Ctrl-C 的真实后果）。键位不是猜的 —— 逐条对着
  `refs/upstream/dialoguer/src/prompts/multi_select.rs` 的按键分支抄，并用 pty 探针实测了 Ctrl-C 行为。
- [x] **帮助补三条**（✅ 9c749ee 已完成，2026-10-28 补勾）：`--level`（压缩级别 0-9，界面打包页已有滑块）、`--sfx` / `--sfx-linux`（自解压包，
  界面已有开关）、`--caps`（各格式能力表，界面设置页已有按钮）。三处引擎都实现了，就是 `--help` 没写。
  验收：`yshjy --help` 能搜到这三个词，且文案与界面上的说法一致。
- [x] **补全与 man 页都改成自动生成 ✅ 已完成（2026-10-28）** —— 三处手写副本全部退休
  （**不许再手写副本**，做法见 `docs/REFERENCE-INDEX.md` §9.1）：
  原状实测（2026-10-28 用 `target/release/yshjy --help` 数）：**`--help` 共 35 个长选项，手写补全只登记 18 个，
  缺 17 个** —— `--caps --config --cover --encoding --extract-cover --keep --layout --level --log
  --no-auto-password --no-checksum --no-color --no-readme --password-file --sfx --sfx-linux --split`。
  根因不是「漏抄」，是**手抄副本必然漂移**。
  **已落地（补全这一半）**：新增 `src/cli_spec.rs`（唯一真源，39 项）+ `--dump-cli-spec` 吐 JSON +
  `scripts/gen-cli-assets.py` 生成 bash/zsh/fish 三份（说明文字从 `help.text` 自动提取，不产生第二份文案）+
  `tests/cli_spec_match.sh` 五条对账探针并挂进 `tests/all.sh`。手写三件套已退休（文件头写明「别手改」）。
  实测：bash 里 `yshjy --imp<TAB>` → `--import-passwords`、`-j<TAB>` → `1 2 4 8`、`--lang<TAB>` → 7 种语言；
  三份补全都从 18 项变 36 项对外选项全在；`bash -n` / `zsh -n` 语法过。
  **man 页也已完成**：`docs/yshjy.1` 改由 `docs/yshjy.1.tmpl` + 生成器产出（说明优先取 help 文案；
  help 里只出现在「用法」段的 6 个选项 —— `--list/--rescue/--report/--no-paths/--help/--version` ——
  由 `MAN_DESC_OVERRIDES` 补），版本号从 `Cargo.toml` 读（**不再手写**），`build-deb.sh` 打包前自动重新生成。
  探针已扩到 7 条（补全/man 的选项集合 + man 版本号全部对账）。
  实测 man 页从 22 个选项变 **36 个**，`groff -man -Kutf8` 只剩 3 条「cannot adjust line」排版微告警（中文长行，无害）。

### 功能（新增，用户点名）

- [ ] **边解压边删源（`--reclaim`）**〔**以后更新做，本版不做** —— 2026-10-28 用户口径：
  「这一版核心已经完整，现在要做多平台适配」，此条排到后续版本，**不是否决**〕：
  不是「解完再删」，而是解的过程中就把源让出来 ——
  峰值从「包 + 解出」降到接近「解出」。三份现成项目已拉下来逐篇读过，结论写在
  **`docs/INPLACE-EXTRACT.md`**：`hatchzip`（Rust，从尾部倒着解 + tail 转存 + journal）、
  `delete_when_unzip`（Python，单 ZIP 搬移后截断 / 分卷读完删卷）、`ReclaimArc`（Windows，靠 NTFS 特性）。
  三档落地，互不阻塞：
  · **第一档（建议先做）分卷读完删卷**：我们的分卷归组、卷名推导都现成，风险最低。
    一卷里的条目全部 fsync 落盘才删这一卷；有失败/取消就停手，剩下的卷一个不删。
  · **第二档 打洞**：Linux `fallocate(PUNCH_HOLE)`（用 `rustix` 的安全封装，引擎不必破 `forbid(unsafe_code)`）。
    顺序解压时每批（约 256 MB）fsync 落盘后，释放已读区间；FAT/exFAT/网络盘自动退回「解完再删」并说明。
  · **第三档 倒着解 + tail 转存**：照 hatchzip 全套做，全格式通用但工程量最大，
    而且 7z 的 solid 块天生省不了 —— 除非实际包都是 ZIP，否则不建议现在做。
  安全铁律（不许破）：**默认关** / **先 fsync 落盘再释放**（顺序反了就是断电双丢）/ 有失败·跳过·取消则一字节不删 /
  分卷整组 / 删前把清单和将释放的空间打出来 / 不动 `.sha256`、`.说明.txt`、密码本、内层包。
  验收：3 卷共 3 GB 的包峰值占用 ≤ 2 GB；`kill -9` 后能续；产物 sha256 与常规解压逐字节一致。

### 界面（GUI）侧

- [x] **「只取封面」入口** ✅ **已完成（2026-10-28）** —— 界面上其实**早就接线了**
  （预览页「伪装封面 / 压缩包内容」两个勾选框，只勾封面时主按钮就写「只提取封面」，
  Dart 侧传 `cover: _wantCover, wantExtract: _wantExtract`）；文档落后于实现，已实测坐实。
  **但实测挖出一个真 bug，顺手修了**：`--extract-cover` / 协议 `cover` **不认 `-o` / `dest`**，
  封面一律落在源包旁边 → 只读目录直接失败、可写目录越跑越多（`tests/data` 里实测堆了 36 个
  `A1_steg_modeA_plain.封面 (N).mp4`，就是契约测试每次跑它留下的）。
  修法：`cover_path_in(input, dir)` + `extract_zip_cover_to(input, base, dest)`（旧的
  `cover_path` / `extract_zip_cover` 保留成薄包装，老调用点零改动）；落点目录不存在就建、
  失败把自建空目录收回；顺手改成 `.yshjy-part` → rename 的原子写（半截 mp4 用户会当成原片）。
  **验收**：新探针 `tests/protocol_cover_only.py` **25 项全绿**，已挂进 `tests/all.sh` ——
  ① 只取封面时 `entries_done=0`、dest 里只有封面一个文件、封面是合法 MP4（ftyp 头）；
  ② 源包目录里零副本；③ 不给 dest 时仍落包旁边（老行为不回退）；④ 同名自动改名不覆盖；
  ⑤ 解压 + 取封面同时要时两者共存同一 dest。
- [ ] **「救援模式」入口**：CLI 有 `--rescue`（索引坏了按文件头抢救），界面里搜 `rescue` 零命中。
  协议里还没有这个命令，要加 `rescue` 命令 + Dart 侧入口 + 契约测试。
  验收：拿一个索引被改坏的样本，界面点「救援」也能救出文件；`tests/contract/` 补一份 golden。

### 全平台（零成本路线，2026-10-28 定）

> 背景：**用户没有 Mac 设备，也不买 Apple 开发者账号（99 美元/年）**。所以不出官方签名/上架版本，
> 改为「公开源码 + 免费 CI 出**未签名**包 + iOS 由懂的人自编自装」。**花的钱 = 0。**
> **2026-10-28 用户改口径（三次，以最后一次为准）**：①先上 GitHub 再干活 → ②先做优化清单、GitHub 需要时再上
> → ③**（本次，最终）核心功能已完整，现在做多平台适配**；苹果不上架不公证，只发未签名包/源码、谁需要谁自己签名装。
> **下面 ①~⑥ 就是本轮主要工作**，其中 ①（CI 骨架）是 mac / iOS 出包的**唯一零成本路径**。
> 项目是开源的迟早要推，但推的时机由这轮需要决定。本机能力实测：
> · 能出：Linux deb/tar（现成）、Windows **命令行** exe（`x86_64-pc-windows-gnu` 目标 + mingw 已装，可用 wine 本机验）、
>   安卓 APK（需先装 Android SDK + NDK，约 8–10 G 下载，待用户点头）。
> · 出不了：Windows **图形界面**版、macOS、iOS —— Flutter 的这三个平台必须在对应系统上编。
> 参考依据（已查证）：GitHub Actions 对**公开仓库免费**（含 macOS runner）；macOS 未签名 app 用户右键「打开」即可，
> 不需要开发者账号；iOS 用免费 Apple ID 可自装到真机（**7 天有效期**，SideStore 之类可续签）。
> 参考项目（真身已在 `refs/upstream/`，台账见 `docs/REFERENCE-INDEX.md` §9）：
> `rustdesk/`（Rust+Flutter 全平台，CI 矩阵与打包脚本）、`localsend/`（Flutter 全平台，一平台一个 reusable workflow）、
> `hiddify-app/`（ffigen 接 Rust 核心）、`tauri-bundler/`（各平台安装包规范）；
> 工具链：`ffigen`（**选它，不选 flutter_rust_bridge**）、`cargo-ndk`、`fastforge`（原 flutter_distributor）、`cargo-dist`。

- [ ] **① 仓库上 GitHub（公开）+ CI 骨架** —— 这是后面所有平台的前置。
  push 触发 `tests/all.sh`；打 tag 触发构建矩阵。验收：push 后 CI 自动跑绿；`git tag v0.9.26` 能触发构建。
- [ ] **② Windows**：`flutter create --platforms=windows` 补平台目录；
  引擎 `cargo build --release --target x86_64-pc-windows-gnu`。产物 zip（exe + 界面目录）。
  **未签名** → 用户首次点「仍要运行」，README 写明怎么点。验收：Windows 10/11 上双击能开界面、能解压、能打包。
- [ ] **③ Android**：`cargo-ndk` 编 arm64-v8a / armeabi-v7a 的 `.so`，`flutter build apk --release`；
  keystore 用 `keytool` 自己生成（免费，**别丢**，丢了以后没法覆盖升级）。
  **重点风险 = 分区存储**：必须说清"能解到哪、不能解到哪"，别做成静默失败。
  验收：真机装 APK → 选一个伪装包 → 解压到指定目录成功，或给出明确的权限提示。
- [ ] **④ macOS（未签名，靠 CI 的免费 Mac 机器）**：`flutter build macos` 出 dmg，**不签名、不公证**。
  README 写明：首次要**右键 →「打开」**（或系统设置 → 隐私与安全性里允许）。
  验收：CI 产出 dmg；在真 Mac 上右键打开能跑（需要借一台或找朋友试）。
- [ ] **⑤ FFI 层**（`src/ffi.rs` 现在不存在，只有 `docs/FFI.md` 设计）—— iOS 的唯一道路，顺带给 Android 提速。
  用 `flutter_rust_bridge` 生成绑定，**别手写 `extern "C"`**。
  验收：Dart 侧 `FfiTransport` 在桌面上跑通一次 list + extract，结果与 `ProcessTransport` 逐字节一致。
- [ ] **⑥ iOS 工程 + `BUILD-iOS.md`**（给愿意自己编的人）：`flutter create --platforms=ios` + Xcode 工程 +
  一页说明（装 Xcode、免费 Apple ID、**7 天有效期**、怎么续）。
  验收：照文档在一台 Mac 上从零编到真机装上（需要借 Mac 或 CI 验证）。

**这个方向明确不做（写死，别反复讨论）**：App Store 上架 / TestFlight（99 美元/年）、macOS 公证、
Windows 代码签名证书（几百到几千/年，用户点「仍要运行」即可）、购买 Mac 设备。

### 已评估、本轮不做

- AppImage 打包 —— 理由与完整配方见 `docs/RELEASE-CHECK.md` §4（本机 glibc 2.41 当基线做出来没人能跑）。
- 终端全屏 TUI 的鼠标支持、终端里画封面预览 —— 各家终端协议不兼容，收益抵不上维护成本。

## 不做（**已经定过，不再讨论**）

- 7z 写包（D18/D22 已评估：只读够用，写包收益低）
- Windows / macOS 全套（自解压 exe 存根、dmg、MSI、winget、跨平台 CI）—— 本阶段冻结
- 云盘 / 同步 / 内置文件管理器 / 追 200 种格式
- 自动更新、崩溃上报（默认关，不急）
- GNOME Shell Introspect 授权（D3：不阻塞，不动）

## 做完的（留个记号，详细证据在 CHANGELOG.md / AUDIT-PHASE0.md）

- ✅ **RAR 接原子写入**（`CountingWriter` 改走 `PartFile`，Drop 时原子改名；调研确认 `rars` 回调**拿不到条目大小/CRC**，只能「Drop 即落地」——CRC 失败时落地与改前相同、不算退步；真实样本与 `7z` 内容 sha256 完全一致 + 分卷正常 + 2 条落盘语义单测 + **`tests/rar_atomic.sh` 真打断**：SIGKILL 后只剩 part、正式名一个没出现、重试零副本零残留、删文件能自愈）—— 0.9.26

- ✅ **打包进度**（伪装 / 自解压 / 普通三条路径都报；新增 `PackTick::TotalHint` 让拼装段换分母，进度条不再卡在压缩段百分比；拼装用 `拷贝带进度` 替掉 `io::copy`；**CLI 与协议两条代码路径都接**；节流 500→200 ms；实测 2 GB 伪装包 82 条事件、0→100% 无回退；新探针 `tests/pack_progress.sh` 7 项）—— 0.9.26

- ✅ **断点续传**（条目级 + JSONL 记录 + 三重校验：包没变 / 文件还在 / 大小对得上；SIGINT 与 SIGKILL 都能续，`tests/resume.sh` **15 项**探针证明续出来的内容与参照逐字节一致，含**随机 kill 点 12/12 轮**与「改名成功 / 记录没写」临界区定向；新事件 `RESUMED`、`caps.resume` + `resume_level: "entry"`、CLI 7 语言文案、界面备注一句）—— 0.9.26

- ✅ **托盘 / 后台**（GNOME 顶栏图标 + 显示窗口/退出菜单；解压中点关窗口 = 藏到托盘继续跑；托盘状态行显示真实百分比、涨 2% 才刷一次；托盘「退出」先让引擎在数据块边界收尾再退；`caps.background` 改 true；界面 13 条测试）—— 0.9.26
- ✅ **默认打开方式**（MimeType 7→14 种；postinst 给 $SUDO_USER 设默认；新增 set-default-mime.sh 可设/查/还原；`tests/mime_default.sh` 7 项断言隔离实测）—— 0.9.26
- ✅ **首屏单一入口**（本来就收口了，但只是注释里的规矩；新增 widget 测试钉住「FilledButton 只能有一个」，并验证过改坏会失败）—— 0.9.26
- ✅ **五态审计**（五页 × 五态逐页对照；修掉打包页/关于页失败态甩裸错误码；历史页失败降级是有意设计，已记档）—— 0.9.26
- ✅ **zip 符号链接**（看 external attributes 的 Unix mode；目标在条目内容里；安全规则与 tar 共用，绝对路径和 `..` 逃逸都拒；`tests/zip_symlink.sh` 9 项断言）—— 0.9.26
- ✅ **RAR「最后一卷没下完」判得出**（靠头里的 `split_after` 位；缺卷说「分卷不齐」而不是「这不是压缩包」；`tests/rar_volume.sh` 8 项断言）—— 0.9.26
- ✅ **RAR 还原修改时间**（回调拿 `file_time` 按版本换算，整包解完统一 `set_modified`；实测解出 2012-07-13；caps 的 `restores_mtime` 改 true）—— 0.9.26
- ✅ **`caps` 能力位如实**（引擎 `--caps` / `--caps --json` 输出 12 个组合 × 6 项能力；设置页点一下问引擎要，不写死）—— 0.9.26
- ✅ **fuzz 写盘路径**（`extract_target`：8 种组合真解压到沙箱；600 秒 71,661 次 0 崩溃；断言不留半成品、不超限额）—— 0.9.26
- ✅ **协议契约测试**（`tests/contract/` 12 份 golden：事件名/字段名/类型/退出码/错误码；故意改坏两种都能测出）—— 0.9.26
- ✅ **设置页「界面语言」文案改实话**（不再并列 7 个语言代码暗示能切，直说目前只有中文）—— 0.9.26
- ✅ **自动抓密码收窄**（同目录要「名字有说明性或与包同名」，上一级只认同名；父目录 README/无关 txt 不再被读）—— 0.9.26
- ✅ **重名策略接线**（rename/replace/skip 真生效，含单流；「全部按设置跳过」不再算失败；`tests/conflict.sh` 14 项全过）—— 0.9.26
- ✅ **错误原因说人话**（没权限/目标是个文件/路径太长/截断包各给专属原因；`tests/errmsgs.sh` 探针全过）—— 0.9.26
- ✅ **解压原子写入**（六条落盘路径：zip/7z/单流/xz/tar/救援；1 GB 包被杀后只剩 `.yshjy-part`，重试自愈、0 个 `(1)` 副本）—— 0.9.26
- ✅ 依赖漏洞与 yanked 清零、`deny.toml` 适配新版 cargo-deny —— T1
- ✅ 阶段 0 体检 T1/T2/T3/T4/T7/T8/T9（证据见 `docs/AUDIT-PHASE0.md`）
- ✅ 0.9.25-c 打包装机；清掉 0.9.21-c 手工残留（`~/.local/opt/yshjy` + `~/.local/bin/yshjy-gui`）
- ✅ 界面文案表（错误码 17 / 诊断码 10 / 条目结果 6，查不到也不露原始码）—— L3/N1
- ✅ 打包进度事件 `PACK_PROGRESS`（引擎 + 协议 + CLI + 打包页进度条）—— L3/N2
- ✅ `tests/all.sh` 一条命令回归（15 通过 / 0 失败）—— L3/N3
- ✅ 批量单行取消（D5）—— L3/N4
- ✅ Del 键删队列条目（D4）—— 早批次
- ✅ 拖入文件夹提示（D6）—— L3/N6
- ✅ 文件管理器右键菜单（Nautilus 脚本 + deb + 用户级安装）—— L3/N7
- ✅ RAR 分卷 / 去 256 MiB 上限 / tar 符号链接 / 跳过不再报数据损坏 / D8 失败带原因 —— L1+L2

## 用法（给自己看的三句话）

1. 开工：读 `HANDOFF.md` 顶部 → 回这个文件看梯队一第 1 行 → 只做那一项
2. 收工：`bash tests/all.sh` 全绿 → 证据写 `CHANGELOG.md` → 这里划掉一行 → 本地 git 提交
3. 半路发现新问题：**不要动手**，追加到表尾，编号往后排
