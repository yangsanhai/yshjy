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
**当前该做的（2026-10-29 第二期收尾后）**：第二期 7 条**全部有结论** —— 4 条做完（窗口尺寸记忆、
CLI 原包处置、右键脚本、补全两处）、2 条核实后**不用做**（「解完移回收站」开关早就在
`settings_page.dart`；密码卡「这次不保存」与 `config.rs` 的密码本保险丝冲突、做了是假开关），
**只剩 1 条：引擎条目 packSize 校验**（要动解码主循环 + fuzz 十分钟，单开一轮）。
⚠️ 装机版仍是 **0.9.28**，这几轮的修复都不在里面，要打 0.9.29 并装机得用户点头。

**当前该做的（2026-10-28 夜·0.9.28）**：**§优化建议那节已经全部清空**
（终端 5 条 + 界面 2 条，最后两条「只取封面」与「救援模式」在 0.9.28 做完，见 CHANGELOG）。
**参考项目补拆已完成（2026-10-28 夜）**：原 §10.2「漏的」9 项全部拆完，产出 5 份文档
（`refs/tech/{clap-complete-mangen,hiddify-core-c-abi,libarchive,ark-fileroller,keka-nanazip-inquire-etc}.md`），
其中 **1 个真 bug**（bash 补全带空格文件名）与 **1 条根因确认**（`--pick` 的 Ctrl-C）已记到本文件表尾。
Keka 无法拆（上游闭源，仓里只有 Bin/ + 翻译）。
**2026-10-28 夜：对照审计完成** → `docs/ROADMAP.md`（台账 14 条失真 / 9 条真缺口 /
Windows 编译实锤阻塞）；可执行条目按三批记在下面「2026-10-28 对照审计」那一节。
所以现在盘面上是「下面那一节 + 这四件」，**都等用户发话，不自己找活**：
1. 表尾四条待查（`cargo test --release` 偶发失败 / `--pick` 被 Ctrl-C 后光标不恢复 /
   bash 补全空格文件名 bug / 缺 `tests/ffi-header-sync.sh` 探针）
2. 「多平台适配」整块（用户口径：**最后做**，探测结论已在下面那张表里，别重复调研）
3. ~~装机~~ ✅ **0.9.28 已装（2026-10-28 22:36）**：`dpkg -l yshjy` = 0.9.28；
   `~/.local/bin/yshjy` 是软链指向 `/usr/bin/yshjy`（0.9.28，干净 PATH 里跑的就是它）；
   无 `~/.local/opt/yshjy` 旧副本；desktop 的 `Exec=/usr/lib/yshjy/gui/yshjy_gui`；
   菜单认领 **14 种格式**；装机版实测「只取封面落 `-o`」生效、包旁边零副本、`--help` 里
   `--pick` / `--rescue` 都在

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

- [x] ✅ **已定性并修好（2026-10-29）**：整轮里真正会红的是 `rar_atomic.sh` ——
  它默认用 `target/debug/yshjy`，而 `all.sh` 第 1 关**只建 release**。所以「改过 `Cargo.toml` / `src`
  却没手动 `cargo build`」时**必然红**、没改过时绿，看着就像偶发 —— 实为**确定性**，不是 I/O / OOM
  （实测时间戳：`Cargo.toml` 比 debug 二进制新 5 分钟，正是改 0.9.28 号那一下）。
  修法：`all.sh` 第 1 关补一条 `cargo build --quiet`（debug，增量 8 秒、无改动近 0 秒），
  三个 debug 探针（rar_atomic / pack_progress / resume）保持 debug 并写明原因。
  **试过「三个探针全改 release」：不可行，别再重复** —— release 解 81 MB 只要 151 ms，
  `rar_atomic` 的中断点（耗时÷3）打不中、`pack_progress` 的进度事件只剩 1 条，两边当场假红。
  验收：`bash tests/all.sh` **30 通过 / 0 失败 / 4 跳过 / 188 秒**（`all10.log`）。
  证据见 `CHANGELOG.md`「未发布 · 2026-10-29」一节。
  原记录里「`cargo test --release` 那一关失败」今日两次全绿、现场已不可考，故并入本条；
  若再复现仍按老规矩先落完整日志（`bash tests/all.sh > ~/.hermes/cache/scratch/all.log 2>&1`），别再只 tail。

- [x] ✅ **已修（2026-10-29）：探针跑一次弹一次文件管理器** —— `pack_progress.sh` 执行自解压 `.sh`
  验证产物时漏了 `YSHJY_SFX_NO_OPEN=1`，而存根解完默认 `xdg-open` 打开目标目录
  （`src/pack/mod.rs:655`）→ 每轮回归在用户桌面弹一个窗口（用户当场抓到两次）。
  其它四个 sfx 探针早就设了，只有这一处漏。验收：跑前后数 journal 里
  `org.gnome.Nautilus` 激活次数，修后**增加 0 次**。**以后新写「会真执行 sfx 产物」的探针，
  一律带 `YSHJY_SFX_NO_OPEN=1`（`YSHJY_SFX_SILENT=1` 同理，别问确认）。**

- [ ] **`--pick` 被 Ctrl-C 中断后光标不恢复**（2026-10-28 实测，pty 探针）：发 `\x03` 后进程以
  SIGINT 结束（rc=-2），屏幕上只有 `\x1b[?25l`（隐藏光标）没有对应的 `\x1b[?25h`，
  真实终端里光标会一直不显示，得 `reset` 才回来。
  根因：中断走信号路径，`dialoguer` 的复原代码没有执行机会。
  **用户口径（2026-10-28）：这轮不做，就记在这里等以后。** 已在 `docs/CLI-SPEC.md` §4.5
  写明「正常退出请用 Esc / q」，所以不算哑坑。
  **动手时的执行顺序（照这个走，别即兴）**：
  ① 先引 `signal-hook`（或 `ctrlc`），MIT/Apache，在 `deny.toml` 白名单内 —— **加依赖前先跑
     `cargo deny check` 四项全绿**，红了就换实现方式；
  ② handler 只做三件事：`\x1b[?25h` 显光标 → `crossterm::terminal::disable_raw_mode()` →
     `exit(130)`。**别在 handler 里做别的**（信号上下文不能干重活）；
  ③ 只在 `--pick` 进屏期间装、退出时摘掉，别让整个程序的 Ctrl-C 行为变味；
  ④ 验收要能测出来：`tests/pick.sh` 加一项 —— pty 里发 `\x03` 后，输出流里必须出现
     `\x1b[?25h` 且退出码 130；顺手断言「不影响正常路径」（Esc / 勾选确认仍退出码 0）。
  备用方案（不行再加）：`rustix` 自己装 handler，不破 `forbid(unsafe_code)`。
  **2026-10-28 补拆 `inquire` 后确认了根因、并发现第二条解法**（详见
  `refs/tech/keka-nanazip-inquire-etc.md` §3）：
  - 根因有了上游背书 —— `inquire/src/error.rs:27-33` 注释原话（SIGINT 那句在 `:32`）：
    「This error will be returned only when using `crossterm` or `termion` as the terminal back-end.
    **If using `console`, pressing Ctrl+C will trigger SIGINT.**」而 **dialoguer 用的正是 `console`**。
    所以不是我们写错了，是后端选型决定的。
  - **解法 B（新）**：`--pick` 自己用 crossterm 读键（`Cargo.toml` 里已有 `crossterm = "0.29"`），
    Ctrl+C 就是 `KeyCode::Char('c') + CONTROL`，**不进信号路径**，收尾代码在同一函数里顺序执行；
    代价是要自己重写分页/过滤/非 TTY 降级（即 `REFERENCE-INDEX.md:136-140` 那条「别自己写 raw mode」的反面）。
  - **执行建议**：先试 A（改动最小）；若 handler 里 `disable_raw_mode` 出现不可重入问题，
    再退 B。**别两条一起上。**

### 待查 / 改进（2026-10-28 补拆参考项目时新发现，按铁律记表尾，不插队）

- [ ] 🔴 **bash 补全的真 bug：没装 bash-completion 包时，带空格的文件名被劈成两个候选**。
  `scripts/completions/yshjy.bash` 的 fallback 是 `COMPREPLY=($(compgen -f -- "$cur"))`，
  命令替换按默认 IFS 再分词 → `有 空格.zip` 变成 `有` + `空格.zip`。实测：无 bash-completion 时
  COMPREPLY 2 个元素（错），装了是 1 个（对），clap 的 `IFS=$'\n'` 写法也是 1 个（对）。
  修法见 `refs/tech/clap-complete-mangen.md` §6.1（生成器 `gen-cli-assets.py:295`、`:312` 两处）。
  验收：新增 `tests/completions.sh`，断言带空格文件名只出一条候选。

- [ ] 🟡 **新增探针 `tests/ffi-header-sync.sh`**：`ffi/include/yshjy.h` 是手写的第二份真源
  （头文件 43 行 vs `ffi/src/lib.rs` 9 个 `extern "C"`），改了实现忘改头文件就会漂移 ——
  和 man 页版本号、补全 18/35 项是**同一种病**（已经栽过两次）。做法照 `tests/cli_spec_match.sh`：
  两边抓函数名清单 `diff`，不一致就红。参考 `refs/tech/hiddify-core-c-abi.md` §7.1-B。

- [x] ✅ **scratch 已清（2026-10-29 收尾）**：删掉 `progtest_192424`（12 G 抓帧样本）+
  **805 个 `yshjy_*` 探针现场（7.2 G）**，scratch 从 **19 G → 968 K**，磁盘可用 71 G → 89 G。
  只留 `*.log`（116 K，CHANGELOG 引用的证据）。`/tmp` 里本轮 8 个临时文件也删了。
  ⚠️ **顺带发现的系统性问题（值得下一轮做）**：探针失败时会在 `$TMPDIR` 留「现场」（一个 RAR 现场
  400~500 MB），而且**从不自动清理** —— 攒了 **805 个目录 / 7.2 G** 才被发现。建议给探针加
  「跑完删现场，除非设了 `YSHJY_KEEP_SCENE=1`」或「启动时清掉 24 小时前的同类现场」，
  并把这条写进 `tests/all.sh` 的收尾。**别只记「记得手动清」** —— 那就是这次攒到 7 G 的原因。
  铁律不变：动手前先念清单给用户确认，`rm -rf` 单独一条命令。

- [ ] 🟢 `complete` 那行补 `-o nosort -o bashdefault -o default`（bash ≥4.4 才有 `nosort`），
  用户可感知的差别：选项候选不再被 bash 按字母序重排。位置 `gen-cli-assets.py:316`。

- [ ] 🟢 `--test`（测试包完整性）作为独立能力位 —— `Ark` 的插件元数据里 `SupportsTesting: true`
  是一个独立能力（`refs/tech/ark-fileroller.md` §2）。我们现在只有打包侧的 `--no-checksum`，
  解压侧没有「只测不写」的动作。另记：Ark 的能力是**按 MIME 声明**的，我们的 `caps` 是全局一份。

## 2026-10-28 对照审计：真缺口与平台前置（路线见 `docs/ROADMAP.md`）

> 由来：拿「5 份参考项目拆解 + `docs/INTERNALS.md` 内部全景」对 64 条台账逐条核实，
> 结论是 **14 条台账标 ❌ 但代码里早做了**（清单在 `ROADMAP.md` §2.2，`REFERENCE-INDEX.md`
> 的状态列要照它重写），**9 条是真缺口**，外加 Windows 编译的一个实锤阻塞。
> 这一节只记「要做的动作」，为什么这么排、平台怎么切全在 `ROADMAP.md`。

### 第一期：对齐与止血（半天）—— 方案 `docs/ROADMAP.md` §3.1

- [x] ✅ **重写 `docs/REFERENCE-INDEX.md` 的状态列（2026-10-29 完成）** —— 上一版 **14 条失真**
  （标 ❌/🟡 但代码里早就做完了；逐条证据原本在 `ROADMAP.md` §1.2，注意不是 §2.2）。
  这次**不是照抄上一份审计**：14 条逐条 `grep` 复核，每条都换成实测的 `文件:行`，落点列一并更正
  （例：`extract.rs:172,349`→`190 密码候选`、`src/scan.rs（无 ctx）`→`job.rs:311`、
  `--split 未实现`→`pack_post.rs:83`）。后 3 条在 §9.1 重复登记的也指回 §8 实现。
  **重写时又抓到 2 条同一类失真**（不在原 14 条里，ROADMAP 上一轮漏了）：
  ① 「输出目录 + 原包处理」标 ❌ 但 `dest: _outDir` 与设置页 keep/trash 都在 → 改 ✅；
  ② 「主题 + 窗口尺寸记忆」整条标 ❌ 但主题早能切 → 改 🟡（只剩尺寸/位置记忆没做）。
  回验：抽查 10 条证据行号逐行打印核对（`extract.rs:190` 是 `密码候选`、`zip.rs:149` 是
  「64 KiB + 20 B 重叠」、`job.rs:311` 发 `ScanProgress`、`extract7z.rs:239` 内存预检、
  `app_shell.dart:114` 三步引导、`main.rs:1246` MultiProgress、`main.rs:872` comfy-table、
  `pack_post.rs:84` split_file、`home_page.dart:1836` 复制解压方法、`history_page.dart:128` onTap），
  **10/10 都对得上**。仍在标 ❌ 的表格行是 **9 条**（`grep -c` + 逐块列数校验过，不是估的），
  已确认为真缺口不是失真：条目 packSize 校验 / 预览树+部分解压 / 干扰数据+假签名 / 移动端 seek 三档 /
  解压后删除源开关 / 移动端桥接（ffigen）/ Android 分区存储 / 发布工作流拆法 / 工作流单文件骨架。
  （另有 1 行标 ❌ 的是 Keka「无法拆：上游闭源」，不是缺口。）
- [x] ✅ **Windows 编译阻塞已解除（2026-10-29）** —— 全仓 48 处 unix-only 逐个看过，**真正没隔离的只有
  `src/extract.rs:499` 一处**（`std::os::unix::fs::symlink`，`E0433: cannot find unix in os`）。
  其余 47 处**早就 cfg 隔离好了**（`main.rs` 的 SIGPIPE/termios、`crash.rs` 的 `localtime_r`、
  `extract_stream.rs` 的 symlink、`tools.rs`/`resume.rs`/`api.rs`/`restore.rs` 的 `PermissionsExt`/`MetadataExt`
  全都有 `#[cfg(unix)]`；`hardware.rs` 的 `/proc`、`/sys` 走 `.ok()?` 降级；`pack/mod.rs` 的 `xdg-open`
  是 shell 存根里的字符串，不参与编译）。**别信「扫到 48 处就要改 48 处」**，先看有没有 cfg。
  修法：`#[cfg(unix)]` 直接建符号链接；`#[cfg(not(unix))]` 退回「目标是什么就复制什么」
  （目录建同名目录、文件 `fs::copy`），并记一条 `DiagnosticCode::SlowPath` 让界面看得到（不静默）。
  **实测三步全过**：`cargo check --all-targets --target x86_64-pc-windows-gnu` rc=0（7 秒）→
  `cargo build --release --target x86_64-pc-windows-gnu` 出 `yshjy.exe`（13 MB，PE32+ console，19.9 秒）→
  **wine 真跑**：`--version` 正常、解开一个真 zip（中文内容逐字节一致、时间戳有还原）。
  Linux 侧回归 `bash tests/all.sh` **30 通过 / 0 失败 / 181 秒**（`zip_symlink.sh` 探针照旧绿）。
  ⚠️ **wine 实测顺手抓到 Windows 侧一个真 bug**：`crash.rs` 的非 unix `本地时区偏移秒()` 还是
  `return 0` 占位 → zip 里的本地时间被当 UTC 解析，**文件名时间差 8 小时、跨日会显示成明天**
  （实测 10-29 的文件解出来显示 10-30）。这条已记进「平台」节的 Windows CLI 一项，跟时区一起修。
- [x] ✅ **进度条补「已用时」（2026-10-29 完成）** —— 就是 indicatif 里 `{elapsed}` 那件事。
  引擎的进度事件不带 elapsed（不为一句话升协议），所以**外壳自己记**：`终端出口` 新增
  `开始时刻: Mutex<HashMap<u64, Instant>>`，按 job 记第一帧时刻（`-j 2` 时两个任务各有各的起点）。
  渲染放在 `进度消息()`（单任务手绘与多任务 `{msg}` **两条路径共用同一份算法**，跟 ETA 一样），
  `画进度行()` 透传；单测能直接塞 `Duration`，不用等真实时间。
  **踩到一个真 UX 坑（加完才暴露）**：`已 xx` 一加，100 列终端上「当前文件名」被挤掉了 ——
  而文件名恰恰是用户最想看的（「现在在解哪个文件」）。修法：已用**最后才加**，且必须先给文件名留
  **8 列**（跟文件名准入线 `剩余宽 >= 8` 同一个数），放不下就牺牲已用、保住文件名。
  验收：单测 27→**29**（新增「已用不足一秒不显示已零秒」「窄终端先丢已用再丢剩余」，后者带
  **属性断言**：任何宽度下都「不超宽」且「已用出现 ⇒ 文件名也在」）；真实终端抓帧
  （2 GB 单包、24 秒、**102 帧**带已用、**0 帧超宽**，最后帧
  `98.4% 1.99 GB/2.00 GB 1/1 项 61.1 MB/s 已 23 秒 in/big2.bin`）；
  `bash tests/all.sh` **30 通过 / 0 失败 / 198 秒**。

> 第一批另外两件（bash 补全 IFS、`tests/ffi-header-sync.sh`）已记在上面「待查 / 改进」节里，不重复登记。

### 第二期：补用户能感觉到的缺口（1–2 天）—— 方案 §3.2

- [x] ✅ **界面：窗口尺寸记忆（2026-10-29 完成）· 位置记忆在 Wayland 下做不了** —— 不引
  `window_manager`，自己修 GTK 侧（`gui/linux/runner/my_application.cc`）。两个真 bug：
  ① 保存只挂在窗口 `destroy` 上，而现在的界面「忙时关窗口 = 藏到托盘」「托盘退出 =
  `g_application_quit`」两条路都不一定走到 destroy —— 用户用了一个月，`gui-window.txt`
  **一次都没生成**，功能等于没做。改成 configure 后 800ms 防抖写盘 + destroy 兜底 flush
  （定时器在 destroy 里先摘掉，防野指针）。
  ② 保存的是「含 CSD 装饰的 surface 尺寸」（本机实测宽 +112 / 高 +144），载入却当内容区喂给
  `set_default_size` → **越开越宽**：实测 880x700 → 存回 992x849 → 再开 1104x849。改用
  `gtk_window_get_size`（不含装饰，与 `set_default_size` 同基准）。
  **位置记忆：Wayland 协议不许客户端给自己定位**（`gtk_window_move` 无效），本机
  `loginctl` 显示会话 `Type=wayland`，所以不做；X11 才做得到，但本机没有 X11 会话可验收。
  **`window_manager` 评估过、不引**：MIT、1126 赞、30 天下载 89 万、0.5.2 活跃，许可证没问题，
  但它同样是 GTK 侧调 `gtk_window_move`/`set_default_size`，Wayland 下一样做不了位置 ——
  接进来只是给「已经有的实现」再加一份真源。
  证据：`tests/gui_window_state.sh` **8 项全绿**（`YSHJY_GUI_VISIBLE_TEST=1` 才跑，默认跳过，
  因为它会真开窗口）；关键回归是「再开一次尺寸不变（不滚雪球）」。真机分辨率 1366x768
  （逻辑工作区 1280x720），极端值场景（文件被手改成 5000x4000）clamp 后仍会被合成器最大化，
  已实测、不算失败（真实路径存的就是窗口真实尺寸，走不到那里）。
- [x] ⛔ **界面：密码卡「这次不保存」勾选 —— 与项目安全设计冲突，不做（2026-10-29 核实）**
  查代码发现前提不成立：`src/config.rs` 里有一条**保险丝测试** `读密码本绝不改动文件本身`，
  注释原话「将来谁加了『自动把抓到的密码 / 试成功的密码写进密码本』的逻辑，这条测试会红。
  密码本只允许**用户主动**写（自己编辑，或 `--import-passwords`）」。
  也就是说界面现在**根本不保存任何密码**（`_recentPw` 只是只读 chips），
  「这次不保存」这个布尔等于「永远不保存」，做了也是假开关（项目最恨的那种）。
  真要做「记住这个密码」是**新功能**（显式按钮 = 用户主动写），得先定：
  存明文还是加密、怎么让用户看见并删掉、要不要按包名关联 —— 等用户点头再排期。
- [x] ✅ **界面：「解完把原包移回收站」开关 —— 早就做完了（2026-10-29 核实，台账又一条失真）**
  证据：`gui/lib/pages/settings_page.dart:265-283` 有「解压完原包：保留 / 移入回收站」下拉
  （读 `on_done` 在 `:65`、写在 `:155`、副标题在 `:279`）；引擎侧 `src/config.rs:244-246` 解析、
  `src/job.rs:845-855` 按「成功 / 部分成功才动」执行 `trash::move_to_trash`。三处都在，不做。
- [x] ✅ **CLI：`--trash-source` / `--delete-source`（2026-10-29 完成）** —— 选项进 `cli_spec.rs`
  （长选项 40 → 42，护栏单测同步改了），`main.rs` 里 `源包处置` 三态：不动 / 回收站 / 永久删除。
  两条都只在**成功 / 部分成功**时动原包（失败时原包是用户重试的唯一材料，与 `job.rs` 同口径）。
  `--delete-source` 开工**前**二次确认（只认 y/yes），非交互（`--quiet` / `--json` / stdin 不是
  终端）**直接拒绝、退出码 2**，并提示改用 `--trash-source`；默认两个都不动、命令行优先于配置。
  7 语言文案全同步（4 条运行时 + 2 行 help）。探针 `tests/trash_source.sh` **10 项全绿**：
  回收站真有 `files/` + `info/`、非交互拒绝且原包不动、输 y 真删、输 n 取消。
- [ ] **引擎：条目 packSize 校验**（按头部声明大小给输出封顶，多写丢弃、少写报截断）。
  语义照 libarchive 的 `entry_bytes_declared`（`refs/tech/libarchive.md` §4）；这条在台账里挂了很久，是数据正确性问题。
  **2026-10-29 决定单独一轮做**：要动 `zip.rs` / `extract_stream.rs` 的解码主循环，
  属于「会写坏文件」的那一类改动，必须配 fuzz 写盘路径（`--slow` 十分钟）+ 全回归；
  和别的条目挤在同一轮里，出了问题分不清是谁的锅。
- [x] ✅ **nautilus 右键脚本按「目标目录可写」给提示（2026-10-29 完成，方案与原文不同）** ——
  原文要「抄 FileRoller 给两套菜单」，实测做不到：Nautilus script 的菜单文字就是脚本文件名，
  没法按权限动态变；真正的动态菜单要写 Nautilus 扩展（C/Python + 编译安装）。
  退一步：能写 → 原地解（原行为不变）；不能写（U 盘 / `/mnt` / 别人的目录）→ 解到
  「下载/解压-<包名>」，通知里写清「原文件夹不能写，已解到下载」，**绝不静默失败**。
  探针 `tests/nautilus_script.sh` **7 项全绿**（可写 / 只读两条路 + 假 `notify-send` 拦通知）。
- [x] ✅ **补全再修两处（抄 clap，2026-10-29 完成）** —— zsh 目录补全 `_directories` → `_files -/`；
  fish 目录补全 `-r -F` → `-r -a "(__fish_complete_directories)"`（⚠️ 必须双引号：单引号里 fish
  不执行命令替换）。落点 `scripts/gen-cli-assets.py`，已重新生成，`cli_spec_match.sh` **7 项全过**。

### 第三期：结构优化（2–3 天，要设计）—— 方案 §3.3

- [ ] 预览树 + 部分解压（界面）—— 引擎有 `select`，界面还是平铺
- [ ] 干扰数据 / 假签名（打包）—— 要定「打多少、怎么打、会不会影响播放」
- [ ] 移动端 seek 三档 —— 跟 Android 一起做（SAF 不支持随机定位）
- [ ] `caps` 按格式声明（Ark 的做法，协议面升版，影响 GUI + FFI 两侧）
- [ ] 7z 头加密专门提示（现在有 `encrypted` / `encrypted_count`，但「头加密时无密码连清单都列不出」没做提示）
- [ ] `build-deb.sh` 补 `prerm`（现在有 postinst + postrm，缺 prerm）
- [ ] GitHub 发布流水线（现在只有单平台 `ci.yml`，没有多平台矩阵与 release 汇总）

### 平台

> **唯一真源在下面「全平台（零成本路线）」那一节（①~⑥）** —— 这里原有一份 4 条的简版，跟那节重复
> 登记（Windows / Android / 苹果各写了两遍）。2026-10-29 把这份副本收掉：进度只往那节写，
> **别再起第二份**。已完成的部分已在 ②/⑤ 里标出（引擎交叉编译已通、`yshjy.exe` 已出、wine 验过）。

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
- [x] **「救援模式」入口** ✅ **已完成（2026-10-28，0.9.28）** —— 协议新增 `rescue` 命令
  （`api::救援`，移植 CLI 那条链），`serve.rs` 走「一次一个任务」调度（慢路径 + 可 cancel），
  `hello.features` 带上它；Dart `EngineClient.rescue()`（30 分钟超时）；失败页在**救得回来**的错误上
  才给「试着救出来（索引坏了也能抢救）」按钮（`DATA_LOST` / `PARTIAL_DATA` / `CHECKSUM_FAILED`，
  以及 `NOT_ARCHIVE` 但 `looks_like=zip`；密码错、缺分卷不给）。
  `list_only:true` 只重建清单不落盘；不给 dest 落 `<包名>.rescue`（与 CLI 一致）。
  **不用 `tests/contract/` 的 CLI golden 兜（那份已有 `rescue.json`），改用真实进程协议探针**：
  `tests/protocol_rescue.py` **22 项全绿**（样本＝现造「中央目录签名被打乱的 zip」），已挂 `tests/all.sh`。

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
- [ ] **② Windows**：`flutter create --platforms=windows` 补平台目录；产物 zip（exe + 界面目录）。
  **引擎部分已完成（2026-10-29）**：`cargo build --release --target x86_64-pc-windows-gnu` 通，
  出 **13 MB `yshjy.exe`**（PE32+ console），wine 里能跑 `--version`、能解含中文的真 zip。
  还剩：① **补时区**（`crash.rs` 非 unix 的 `本地时区偏移秒()` 仍是 `return 0` 占位 → 改
  `GetTimeZoneInformation`；不补则 zip 里的本地时间被当 UTC，**文件名时间差 8 小时、跨日显示成明天**，
  wine 实测已踩到）② wine 跑 4 样本 + 1 坏包 ③ 出 zip 包 ④ 写 `docs/BUILD-Windows.md`。
  **未签名** → 用户首次点「仍要运行」，README 写明怎么点。验收：Windows 10/11 上双击能开界面、能解压、能打包。
- [ ] **③ Android**：`cargo-ndk` 编 arm64-v8a / armeabi-v7a 的 `.so`，`flutter build apk --release`；
  keystore 用 `keytool` 自己生成（免费，**别丢**，丢了以后没法覆盖升级）。
  **重点风险 = 分区存储**：必须说清"能解到哪、不能解到哪"，别做成静默失败。
  验收：真机装 APK → 选一个伪装包 → 解压到指定目录成功，或给出明确的权限提示。
- [ ] **④ macOS（未签名，靠 CI 的免费 Mac 机器）**：`flutter build macos` 出 dmg，**不签名、不公证**。
  README 写明：首次要**右键 →「打开」**（或系统设置 → 隐私与安全性里允许）。
  验收：CI 产出 dmg；在真 Mac 上右键打开能跑（需要借一台或找朋友试）。
- [ ] **⑤ FFI 层** —— iOS 的唯一道路，顺带给 Android 提速。
  **现状修正（2026-10-29，上一版文档是错的）**：`ffi/` **早就存在**（`ffi/src/lib.rs` 9 个
  `extern "C"` + `ffi/include/yshjy.h` 手写头文件），不是「`src/ffi.rs` 不存在」；缺的是
  **Dart 侧的 `FfiTransport`** 和绑定生成。
  **绑定用 `ffigen`，不用 `flutter_rust_bridge`**（结论在 `REFERENCE-INDEX.md` §9.2：我们的 C ABI
  已经是 JSON 进 / 事件出，ffigen 只在 Dart 侧生成绑定、引擎一行不用改；frb 要从 Rust 侧改导出层）。
  另：手写头文件与 `ffi/src/lib.rs` 会漂移 → 要做 `tests/ffi-header-sync.sh`（已记在表尾）。
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

- ✅ **「解完自动打开目录」的口径定了**（2026-10-29，用户问「自动打开好还是不打开好」后拍板）：
  **自解压 `.sh`** = 真人在终端跑或双击才开，脚本调用/重定向不开；**图形界面** = 默认不开，
  设置页给开关、批量只开第一个目录；**命令行** = 永不开。证据与四场景验证见 CHANGELOG「未发布」三、四节。
- ✅ **探针二进制档位**（2026-10-29）：`rar_atomic`/`pack_progress`/`resume` 必须用 debug，
  `all.sh` 第 1 关补 `cargo build --quiet`（增量 8 秒）。**「全改 release」实测不可行**（release 太快，
  中断点与进度事件数假设全崩），别再试。
- ✅ **探针不再弹用户文件管理器**（2026-10-29）：`pack_progress.sh` 执行自解压 `.sh` 补
  `YSHJY_SFX_NO_OPEN=1`；验收方式 = journal 里 `org.gnome.Nautilus` 激活次数 +0。

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
