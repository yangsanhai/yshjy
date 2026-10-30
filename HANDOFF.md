# yshjy 交接文档（重开对话先读这个）

> **2026-10-30 深夜 · 清 `docs/AUDIT-GAP.md` 对账欠账 · 6 个提交 · 未发版**
>
> 用户的原话是「写个进度清单，做一个验证一个再写下一个清单，全搞完」。清完的样子见
> `docs/TODO.md` 顶部那张 T1~T15 表（**唯一真源**，别另建清单）。
>
> **已完成（8 项）**
> - `11e1851` T2 引擎：`SpaceShortage` 事件以前**全库只有定义、零发射点**（界面那段是死代码）；
>   现在预检失败真发事件，`JobOutcome.error_detail` 把 `need`/`available` 带到外壳层，
>   CLI 输出「需要 X / 目标盘只有 Y / 还差 Z」（7 语言）；顺带去掉了「0 B，用时 0.0 秒」。
>   T3：`default_mode` 是死配置（真机制是程序名 `yshjy`/`yshys`），**按删除处理**。
> - `ab84533` T9/T10 门面：AppStream 元数据（`appstreamcli validate` 通过）+ hicolor 7 尺寸图标
>   （已接 deb 与用户级安装）+ README 版本与如实说明；SECURITY.md / CODE_OF_CONDUCT.md / 三份 issue 表单。
> - `fde75ce` T13 台账：`REFERENCE-INDEX` 再清 5 处失真 + 完成度表全部换成本轮实测值。
> - `1b372f4` T4/T6 界面 S5：**真机截图验过** —— 界面显示「需要 3.00 GB / 目标盘只有 1.94 GB /
>   还差 1.06 GB」+ 三个按钮（换个位置 / 只解压部分 / 继续试试），`extract.force` 以前界面根本没传。
> - `bfee65f` T8 引擎侧：自动找密码拆成两档（`auto_from_book` + `--no-auto-password-notes` /
>   `-book`，总开关语义不变），探针 `tests/auto_pw_switch.sh` 8/8。
> - `ef9cb9c` T12 CI：`.github/workflows/ci.yml` 加 `gui` job（flutter analyze + test）。
>
> **★ 本轮最值钱的东西：真机点那一下撞出两个只读代码看不出来的 bug**
> ① `api.rs` 失败路径**无条件重新 identify** 再兜底 —— 而空间不足时 identify 是**成功**的，
> 于是走兜底分支、`detail` 全丢，界面只剩一句空话。② 错误页误报「这个文件看起来是：认不出的文件」
> （包是好的，用户会以为坏了跑去重下）。两个都修了，并补了探针 `tests/t2_space_detail.py`
> （默认跳过，设 `YSHJY_SPACE_PROBE_PKG=<3GB包>` 才跑）和 widget 测试 10 条。
>
> **没做（都在 TODO 表里，别以为漏了）**
> - **T5 多候选**：引擎只给**单个** `looks_like`，没有"N 个候选带可信度"。要做得改探测层；
>   且对新手反直觉（让他从 6 个候选里挑）。**替代方案（说清最可能的那个 + 一键强试）待用户点头。**
> - **T7 八项入口**（decoy / 布局 / `--test` / smart-dir / report 目录 / jobs…）：八次真机验收，慢。
> - **T8 界面半**：设置页那几行「自动抓解压码 = true」是**硬编码字符串**，页面自己标了
>   「其它（改 config.toml）」。要变真勾选，得先加「读配置 / 写配置」的协议命令（现在只有 `--show-config` 打终端）。
> - **T9 差主界面截图**（已有 `screenshots/space-shortage.jpg`）、**T11 无障碍**（`Semantics` 零处）、
>   **T14 界面多语言**（474 条硬编码，两天级）、**T15 覆盖率**（要装 `cargo-llvm-cov`）。
>
> **状态**：`cargo test` 401 过 / `tests/all.sh` **44 过 0 挂 5 跳** / `flutter test` 99 过；
> **未发版**，装机版仍是 0.9.34（不含本轮改动），发版前要跑 `scripts/build-deb.sh` + 装机四项验证。
>
> **别重踩的三个坑**
> 1. **widget 测试里的 detail 形状是我自己假设的** —— 假设错，测试照样全绿。界面改完必须真机点一次。
> 2. **协议两条通道形状不同**：`--json` 是 `{"事件名":{...}}`，`--serve` 是 `{"kind":..,"data":..}`，
>    而 `SPACE_SHORTAGE` 的具体数字还嵌在 `event.SpaceShortage` 里 —— 写探针别照另一条通道抄。
> 3. **bash 变量名不认中文**：`local 期望=ok` 会报「不是有效的标识符」，标识符全用英文，中文只放字符串里。

> **2026-10-30 晚 · 移动设备落盘检验已落地（用户拍板的方案，真机验过）** ——
> 设置里新增**默认关**的开关「解压到U盘/移动硬盘时，等数据真正写完再报成功」：
> 打开后只对移动设备生效，给真进度条 + 「请勿拔出设备」；本机硬盘一秒不多等；
> **开关关着也提示**「拔盘前请先安全弹出」。铁证：关检验时程序报「完成」那一刻
> U 盘实际只写了 **0.0 MB**（12 秒后才 20 MB），开检验时那一刻已写 **20 MB**；
> 界面亲手点过：`用时 0.1 秒` + `落盘校验通过（13.2 秒）`。
> 代码：`src/flush.rs` + `[safety] flush_movable`；事件 `FlushProgress` / `FlushFinished`；
> 诊断码 `MovableNotVerified`；探针 `tests/flush_movable.sh`（已挂 `tests/all.sh`）4/4。
> 回归：`cargo test` 367 过 / clippy 干净 / `flutter test` 89 过。
> **已发版：0.9.34**（2026-10-30，已装机验过）。全文见 `CHANGELOG.md` 的 `[0.9.34]` 节。
> ⚠️ 验证时踩的两个坑：`--json` 是 `{"事件名":{...}}` 而 `--serve` 是 `{"kind":..,"data":..}`；
> 界面输出目录默认「包旁边」且**不读** config 的 `[output] dir`（要验U盘就把包放U盘上）。

> **2026-10-30 · GUI「加密包是死路」已修 + Linux 侧把 W1~W9 拉回来了（装机版仍是 0.9.31）** ——
> **用户真机报的 bug**：带解压码的包拖进图形界面，只有一句「这个包需要解压码」，
> **永远没有输入框**。先怀疑是 Windows 编译问题，实测是界面自己写错码名，两个平台一起错。
> **根因**：`home_page.dart::_needsPassword` 判的是 `PASSWORD_REQUIRED` / `PASSWORD_WRONG`，
> 而 `yshjy --serve` 真实返回 `NEED_PASSWORD` / `WRONG_PASSWORD` → 条件恒假。
> 文案表（humanize.dart）当年改对了，这个开关漏改。**连带 4 处同款错位都已修**：
> 漏 `HEAD_ENCRYPTED`（7z `-mhe=on` 也没入口）、`camelToCode` 把 `TRUNCATED` 拆成
> `T_R_U_N_C_A_T_E_D`、救援按钮用 `PARTIAL_DATA`（诊断码）+ `CHECKSUM_FAILED`（不存在）、
> `errorText` 缺 `VIDEO_UNPLAYABLE`；文档 `PROTOCOL.md` / `GUI-PLAN.md` / `TODO.md` 的错码同步改正。
> **防回归**：判断收进 `humanize.dart` 的 `codeNeedsPassword` / `codeCanRescue`，
> 新增界面测试 **21 条**（humanize 10 + password_box_widget 11），**反向验证过**
> （改回老写法当场红 4 条）。`flutter test` 67 过 / `cargo test` **390 过** /
> `bash tests/all.sh` **41 过 / 0 挂**。全文见 `CHANGELOG.md` 的 `[0.9.34]` 顶部小节。
> **同步方式记一笔**：`git fetch` 直接读移动硬盘仓库会报"所有权可疑"（NTFS ro 挂载 owner=root，
> 且 `-c safe.directory` 传不进 upload-pack 子进程）→ 在盘上 `git bundle create 增量.bundle e494259..92b71f7`
> 再本机 `git fetch bundle`，64K 就够，别为此改全局 gitconfig。
> **已发版**：**0.9.33 已装机**（2026-10-30）。这一轮连出三版：
> 0.9.31（Ctrl-C）→ 0.9.32（GUI 加密包死路 + 预览页直接给解压码 + 从硬盘拉回的 W1~W9）
> → **0.9.33（打包页：输出名跟着输入变 / 完成后显示真实路径 + 打开所在文件夹）**。
> 装机后每轮都验四项：`dpkg`/`--version`、菜单 14 种格式、`tests/pick.sh` 11 项、
> 装机版真打包解压 sha256。发版记录见 `docs/RELEASE-CHECK.md` 顶部。
> **✅ 已随 0.9.34 发出去**：提交 `82d3088`（打包页命名/路径抽纯函数 + 修 Windows 写死 `/` 的 bug
> + 20 条单测）现在**在安装包里**了。Windows 侧拉完仍需重编 `yshjy.exe`（那只在用户自己的 Windows 机上做）。
> **📋 验收方案已定**：见 `docs/AI-验收方案.md`（2026-10-30 起草）。
> 发版必须过「验收四层」，其中第三层是**AI 用 computer-use 真开界面点 + 截图**——
> 今天的四个 bug 全在这一层，而它此前完全空白。晚上按该文档第 7 节 S1~S5 干。
> **下一步**：① **fsync 那笔账**（`docs/TODO.md` 表尾 🔴 那条）仍未动 —— Linux 侧最后一个
> 真会让用户丢数据的坑（报 ✅ 时数据还在内存里，立刻拔盘会全灭）；
> ② Android / macOS / iOS 三平台未开始；③ 界面只有中文（架构决定，非缺陷）。

> **2026-10-30 深夜 · Windows 审计清单 W1~W10 全部清账（ZCode 自主夜跑，老板授权直接提交）** ——
> 审计表（`docs/TODO.md`「2026-10-30 Windows 实测审计」）**十项全 ✅**，每项一个提交、
> 证据全文在 `CHANGELOG.md` 的「[0.9.34] 2026-10-30」：
> **W1 时区**（6c430f5）→ **W2 续传两层根因**（94dd695，append 权限 + canonicalize 指纹漂移）→
> **W4 密码回显**（2b15deb，dialoguer::Password）→ **W5 内存闸**（b14d42e，外壳注册探针）→
> **W6 波浪号**（641a8ca）→ **W8 单测清零**（8474087，挖出 WSL bash 假绿）→
> **W3 回收站**（dcc28eb，trash crate 走系统回收站）→ **W7 回归套件全绿**（e9a8de1，
> `tests/all.sh --no-gui` = **35 过 / 0 挂 / 9 明说跳过**，四个深坑见 CHANGELOG）→
> **W9 图形界面**（bfb2781 + 本次提交）：`flutter build windows` 出包实测、
> 设置页「关窗口时」tray/exit（默认分平台）、Windows 托盘 C++（同一份 yshjy/tray 协议）+ 气球通知。
> **全库现状**：`cargo test` **355 过 / 0 挂**（基线 341/14）、clippy 干净、flutter test 43 过 / 3 跳。
> **下一个会话先看**：①老板的 GUI 肉眼验收清单（CHANGELOG W9 节末尾）；②W2 的真 Ctrl-C 交互路径
> 只能真终端验；③Linux 侧把 main 拉过去重跑 `bash tests/all.sh`（本分支改动在移动硬盘同一份 .git）。
> **别重踩**：MSYS `[ -x ]` 对无后缀名兜底 .exe；MSYS grep 星平面 emoji 要 LC_ALL=C 或 ASCII 标记；
> Windows 7-Zip 输出文件名是 GBK；MSVC 无 BOM 按 GBK 读源码（runner CMake 已加 /utf-8）；
> MSYS kill -INT 送不进原生子进程（无控制台）；heredoc 传 python 的 \n 会被吃一层转义。

> **2026-10-30 收工 · 环境已配齐 + 参考项目查完（代码仍然一行未改）** ——
> ① **Windows 环境备齐**：Inno Setup 6.7.3（`INNO_SETUP_PATH` 已设）、Flutter 3.47.8（`D:\flutter`，
> 进了 PATH，`PUB_CACHE` 挪到 `D:\pub-cache` 免得吃 C 盘）。**C 盘只剩 ~10 G，装什么一律 D 盘。**
> 操作手册见新建的 **`docs/BUILD-Windows.md`**（编译 / 测 / 出两个包 / 五个 Windows 坑）。
> ② **参考项目结论**（都已拉进 `refs/upstream/`，台账在 `docs/REFERENCE-INDEX.md` §9）：
> **W1 时区**改成 `GetLocalTime - GetSystemTime`（**不要自己算 Bias**，夏令时让 Windows 算），
> 出处 `refs/upstream/time/…/windows.rs`；**W3 回收站自己写不进去** —— COM 要 unsafe，
> 而 `trash.rs` 在 `lib.rs` 的 `#![forbid(unsafe_code)]` 里，**只能引 `trash` crate**，
> 出处 `refs/upstream/trash-rs/src/windows.rs:92`（含「`PerformOperations` 成功≠真删了」那个坑）；
> **W9 关窗口**照 `refs/upstream/localsend/app/lib/widget/watcher/window_watcher.dart`（它的默认值
> 正好是「直接退出」，与用户拍板一致）；**出两个包**照 `refs/upstream/hatchzip/packaging/`。
> ③ 意外收获：`hatchzip` 与 `delete_when_unzip` 就是延后项 `--reclaim` 的现成实现。
> **下一轮开工：W1 → W2 → W4**（都在 `docs/TODO.md` 末尾，编号不用重新排）。
>
> **2026-10-30 · 第一次在真 Windows 机器上编 / 跑（代码一行未改，装机版仍是 Linux 的 0.9.31）** ——
> 仓库副本在 `E:\yshjy-20261030`，机器上 Rust 1.95 + MSVC 14.50 + Win SDK 10.0.26100 齐备，
> **`cargo build --release` 57 秒出 7.8 MB `yshjy.exe`**，`--version` / `--caps` 正常，
> 真解了一个伪装成 `.mp4` 的中文名 zip，内容正确。**引擎在 Windows 上是通的，不是"编不动"。**
> `cargo test` 实测 **341 过 / 14 挂 / 1 ignored**。
> **待办与证据全部落在 `docs/TODO.md` 末尾「2026-10-30 Windows 实测审计」一节（W1~W10），
> 这里不另记。** 一句话：W1 时区（+8 小时，实测复现）、W2 断点续传在 Windows 上静默失效
> （`append` 模式下 `set_len` 被拒，10 条单测抓到）、W3 回收站写错地方、W4 密码明文显示、
> W5 内存预检失效、W9 图形界面（用户点名一定要上，脚手架已在、Flutter 待装）。
> **编译两个坑（W10）**：`CARGO_HTTP_PROXY=` 置空（注册表里残留 7890 代理，直连其实是通的）；
> cargo 要执行自己生成的程序，受限环境里会 `os error 5`。
>
> **2026-10-29 夜 · 真机实测：磁盘满预检 + 报成功后拔盘（代码一行未改，装机仍是 0.9.31）** ——
> 拿用户那块 14.6G U盘（切 512MiB exFAT）做的，**两条硬结论**：
> ① 磁盘满预检 3 次全命中（剩 110M 想解 300M → 直接拒、0 字节落地），「写到一半才满」本机造不出
>    （预检准 + 页缓存秒回，要写超过脏页上限 ≈3.9G）；
> ② **屏幕报「✅ 解开（300.0 MB，用时 0.3 秒）」后立刻拔盘 → 300MB 一个文件不剩**（重插 `fsck`:
>    `clean. directories 1, files 0`，解压目录都不存在）；真落盘约 200 秒，差 **660 倍**。
> 修复方向（解压前 `fsync`）+ 验收方式已按铁律记进 `docs/TODO.md` 表尾，**本轮没动手**。
> 证据全文见 `CHANGELOG.md` 的 `[未发布]` 一节。**U盘上的坑**：廉价盘持续写速 1.5 MB/s（突发 6.7）。
>
> **📖 想一次看懂代码结构** → `docs/INTERNALS.md`（49 个模块的职责/依赖/机制/落点/探针，
> 由 `scripts/scan-src.py` 从源码扫出来；数字是扫的，不是手写计划）。
> **待办与完成度只看 `docs/TODO.md`**（顶部有一张「📊 完成度总览」，别处写完成度一律指回它）；
> **参考项目台账看 `docs/REFERENCE-INDEX.md`**；
> **往哪走 / 全平台怎么切看 `docs/ROADMAP.md`**（2026-10-28 对照审计 + 三批路线）。
> **2026-10-29 晚 · 文档对齐（纯文档，代码一行没改、没出包、装机版仍是 0.9.31）** ——
> ① `docs/TODO.md` 顶部新增「📊 完成度总览」：**完成度的唯一真源**（14 块状态 + 复现命令 + 一句话总评
> 「Linux 单平台发布级 / 全平台 **1 / 5 个平台完成**：Linux ✅、Windows 半、Android·macOS·iOS 未开始」），
> 并补了一张 **「Linux 还剩什么」** —— 4 条小账（RAR CRC 落地 / 真实 ENOSPC·拔盘未实测 / 首用链路未走完 / 界面只有中文）
> + 1 个延后功能（`--reclaim`）+ 2 条「物理做不到·已决定不做」。**别把 ✅ 读成零遗留**。
> ② 表里第三期 B（干扰数据/假签名）、C（seek 三档探测）两条**做完却没打勾**的项已划掉并补证据。
> ③ `README` 版本从 0.7.0 / 0.8.0 对齐到 **0.9.31**；GUI「🚧 规划中」、ZIP `.z01` 分卷「规划中」、
> 封面「规划中」三处其实**早就做完了**；单测 98 → **388**，fuzz 只写 2787 万 → 补上只读 2.38 亿 + 写盘 7.2 万。
> ④ `docs/INTERNALS.md` 模块表补 `pack/decoy.rs` / `seek.rs` / `io/blockcache.rs` 三个模块，
> 52 文件 / 29,584 行、错误码 16→**17**、协议命令 10→**11**、CLI 37→**43**、探针 41 通过；
> **架构图改成只画层级、把写死的行数删掉** —— 图上写数字必然漂（这病栽过两次）。
> ⑤ `docs/ROADMAP.md` §0 一页看懂对齐 0.9.31，完成度一律指回 TODO，不再第二份。
> **证据**：`cargo test` 388 通过 / 0 失败（本轮实跑）；`scan-src.py --check` 与文档数字逐项一致。
> **下一轮**：仍是「多平台适配」整块，最便宜的一步 = 补 Windows 时区占位（`crash.rs` 非 unix 分支）。
>
> **2026-10-29 22:4x · 0.9.31 已出包并装机** —— 这一版只有一处改动：`--pick` 被 Ctrl-C
> 打断后光标不恢复（顺手收掉「`ctrlc` 只能装一次 + `let _ =` 吞错误」那个隐患）。
> 装机验过四项：`--version` = 0.9.31、菜单 14 种格式、`tests/pick.sh /usr/bin/yshjy` **11 项全绿**、
> 真打包解压**逐文件 sha256 一致**。`dist/` 只留 0.9.31 全套 7 项（36M，旧 0.9.30 已按用户要求清掉）。
> 证据见 `CHANGELOG.md` 的 `[0.9.31]` 一节与 `docs/RELEASE-CHECK.md` 顶部记录。
> **待办现在只剩「多平台适配」整块。**
> **2026-10-29 · `--pick` Ctrl-C 光标复原（已进 0.9.31）** —— TODO 表尾最后一条待查项清掉。
> **三条坑记档，别重踩**：
> ① `ctrlc` 全进程**只允许装一次**（第二次返回 `MultipleHandlers`）→ 现在统一在
>    `main::装中断接管()` 一处安装、按阶段分派（`--pick` 在屏 / 解压中 / 其它）。
>    **别再把 `set_handler` 挪回解压流程里，更别用 `let _ =` 吞错误** —— 那会让解压时的
>    取消接管静默失效（以前就这么埋着）。
> ② 接管 SIGINT 之后 `dialoguer` 的阻塞读被 `EINTR` 打断，`interact_opt()` 返回
>    `IO error: read interrupted` → 必须判成「用户取消」（复原光标 + 退 130），
>    不能当内部错误报给用户（否则屏幕上冒「交互勾选出错」）。
> ③ `console` 的 raw mode 是 RAII 的、被信号打断时自己复原 —— **只有光标要我们补**，
>    不用存 termios 快照。
> 探针 `tests/pick.sh` 新增第 ⑤ 组 5 项，反向验证过（注释掉 `装中断接管()` → 当场红 2 条）；
> `tests/all.sh` 41 通过 / 0 失败 / 5 跳过 / 203 秒；证据见 `CHANGELOG.md`「未发布」一节。
> **解压中 Ctrl-C 另测过没变味**：2 万碎文件包解到一半 SIGINT → 退 130、留已解 6890 个、
> 残留 part 0 个。
> **2026-10-29 · 第三期 B + C（干扰数据 / 假签名 · 移动端 seek 三档）** —— 第三期到此清完。
> **① B `--decoy`**：新模块 `src/pack/decoy.rs`（8 单测）+ `pack::build_ctx_decoy` + CLI + 协议。
>    噪声只写在「压缩数据之外」（L1 封面后 / L2 free 原子 pad 后），绝不进盒子本体与压缩流；
>    随机量由 `种子(输入路径, 盐)` 派生，**同一份输入两次打包 sha256 完全相同**。
>    **方案被实测推翻两处**（都记进 CHANGELOG 一节）：㈠「噪声 ≤ 外壳 10%」不可行（内置封面
>    1688 B，10% = 168 B，连最小段 4 KiB 都放不下；实测噪声长度对 file / 7z / unzip 无影响）
>    → 改绝对上限 1 MiB；㈡「假签名后跟随机字节」会**随机性地骗坏 7-Zip**（20 种子 × 2 长度
>    实测 17/40 失败）→ 改「签名 + 30 字节零 + 其余随机」（0/40）。`pack::decoy` 模块头有实测表，
>    **别改回纯随机**。
>    探针 `tests/decoy.sh` **14 项全绿**（已挂 all.sh）；反向验证：把写噪声挪到 ZIP 之后 → 当场红 2 条。
> **② C seek 三档**：`src/seek.rs`（`SeekAbility` + 探测 + `复制到本地` 带空间预检，6 单测）+
>    `src/io/blockcache.rs`（32 KiB 块 / 8 MiB LRU，回退到已淘汰块**明确报错不装 EOF**，7 单测）+
>    协议 `hello.caps.seek_probe` 与 `scan` 的 `seek` / `seek_label`。探针 `tests/seek_probe.py` **7 项**。
>    **没做也不冒充**：本机无 Android，「SAF 的 content:// 算哪一档」未验证 —— 落地清单在 `docs/TODO.md`。
>    **坑**：探针一开始把 FIFO 塞进 `scan`，引擎 `job::identify` 会 open FIFO → 永久阻塞（120 秒超时）；
>    FIFO 改由单测覆盖（只探测不打开）。**以后别把管道/FIFO 放进 scan 的 paths**。
> **收尾（22:14）**：五处版本号同步（`Cargo.toml` / `gui/pubspec.yaml` / `ffi/Cargo.toml` /
> PROTOCOL 两处示例 / man 页由生成器带出）→ `CHANGELOG` 三段「未发布」改成 `[0.9.30]` →
> `build-deb.sh` 自查通过 → `release.sh` 出两档 tar.gz → `test-packages.sh` **37 项全绿**。
> `dist/` 只留 0.9.30 全套（旧包 55 项已按用户要求删除）。
> **装机已验（22:17，用户输的 sudo 密码）**：`dpkg -l yshjy` = **0.9.30**、`--version` = 0.9.30、
> 菜单认领 **14 种格式**、三种补全装到系统目录、`~/.local/bin/yshjy` 是指向 `/usr/bin/yshjy` 的软链（非覆盖）。
> **真机实测（1200 条目 · 带干扰数据）**：打包 1200 条目 + 🎭 4 段 64 KB + 自检通过；
> `--list` 与第三方 `7z l` 都列出 1200 files；`--test` 通过；真解压 1200 个文件，
> **与源目录逐文件 sha256 完全一致**；`--help` 里 `--decoy*` 三项都在。
> 测试临时产物（`scratch/{dt,decoy_probe,ship_verify}`，共 32 MB）已清，日志留档。
> 下一轮只剩「多平台适配」整块与 `--pick` 光标那条（等用户点头）。

> **2026-10-29 · 第三期 A：预览树 + 部分解压（**纯界面轮，引擎一行没改**）**
> ① 新增 `gui/lib/entry_tree.dart`：平铺清单 → 目录树（纯 Dart、不 import Flutter，可直接 `flutter test`）。
>    节点带子树合计的 `size`/`packed`/`fileCount`/`encryptedCount`；同名中间目录合并；空目录
>    显示但不可勾；`packed` 任一子项未知就整树记 `null`（不拿 0 冒充）。
> ② 预览页改树形（默认树，可切平铺）：三态勾选（勾目录 = 整棵子树，半选时点一下 = 全选）、
>    状态栏 `已选 N / M 个文件 · 合计 X`（按文件条数算）、面包屑**真过滤**、
>    **目录行右侧「只看这一层」下钻入口**（方案没写，截图验收才发现「只能回不能进」）、
>    列标题「大小 / 压缩后」。
> ③ `EntryItem.packed`：引擎**一直**在发（`src/event.rs:112` + `--list --json` 实测有 `"packed":6`），
>    只是 Dart 侧没解析 → 补解析，不动协议、不升版本。
> ④ 测试：`entry_tree_test.dart` **19 条** + `preview_tree_widget_test.dart` **7 条**（真点界面，
>    为此给 `HomePage` 加 `debugPreview` 注入口，生产永传 null）+ `tests/tree_select.sh` **7 项**
>    （挂进 all.sh；序号串 vs 通配产物逐字节一致；**反向验证过会红**）。
> ⑤ 回归：`flutter test` **46 条全绿**（须给隔离 `XDG_CONFIG_HOME`）、analyze 仅 2 条既有 info、
>    `bash tests/all.sh` 日志 `~/.hermes/cache/scratch/all_tree.log`。真机实测 **1026 条目**包渲染正常。
> ⑥ 坑记档：widget 测试默认视口 800×600，ListView **看不见的行不 build** → `find` 报「找不到」，
>    会被误判成功能坏；测试里设 1200×2000。**真机绝对指针手点不可靠**（一次点击把「已选 1001」
>    点成「已选 1」），状态机类交互一律走 widget 测试。
> ⑦ **已出包并装机（21:46）**：`dist/yshjy_0.9.29_amd64.deb`（9.9M）+ 两档 tar.gz 全部重出
>    （旧包改名成 `*.before-tree` 保留，没删），`test-packages.sh` 37 项全绿；
>    `sudo apt install` 覆盖安装 → `dpkg -l` = 0.9.29；装机三验全过；**装完真启动界面截图核对**
>    「大小/压缩后」列标题 + 目录行下钻箭头都在，1026 条目树形正常。
>    旧包 `yshjy_0.9.29_amd64.before-tree.deb` 等还在 `dist/` 里，等用户发话再清。
> **下一轮**：第三期 ② B 干扰数据 / 假签名（方案 §B 已写，引擎 + 打包侧，要单独一轮）。
>
> **2026-10-29 · 第三轮：表尾待查清完 + 两个新能力 + 打包补 prerm + 第三期方案**
> ① 表尾 4 条全清：**bash 补全 IFS 劈词（真 bug，改 `mapfile`；实测否决 `IFS=$'\n'` 前缀写法——
>    会泄漏全局 IFS）**、FFI 头文件对账探针（顺带抓到 `ffi/Cargo.toml` 版本停在 0.9.20）、
>    `complete` 补 `-o nosort -o bashdefault -o default`（并给「故意不给候选」的位置
>    `compopt +o default` 关回落）、**新能力 `--test` 只测不写**（开关收在 IO 层空设备，
>    七条落盘路径解码逻辑不动；不建目录/不设属性/跳过空间预检/停用续传；与源包处置互斥）。
> ② 7z 头加密单独说人话（新码 `HEAD_ENCRYPTED`，错密码单独报 `WRONG_PASSWORD`），
>    顺带修掉 **`do_list` 把密码写死 `None`** 的真 bug（头加密包带正确密码也列不出内容）。
> ③ `caps` 按格式声明：`unpack::能力组合` 升成唯一真源，协议 `hello.formats` 给 12 条
>    （只加不删 → 协议版本不升）；界面不再 spawn `--caps` 子进程（iOS 跑不了），改走 `EngineClient.hello()`。
> ④ `build-deb.sh` 补 `prerm`：只在 `remove|purge` 动手，**升级早退**（不许杀掉用户正在跑的解压）；
>    `test-packages.sh` 37 项全绿。
> ⑤ **第三期方案已写**：`docs/THIRD-PHASE-PLAN.md`（A 预览树 / B 干扰数据+假签名 / C 移动端 seek，
>    每条带参考出处、设计决策、边界、验收）。**这三条本轮只写方案、代码一行没动。**
> 回归：`bash tests/all.sh` **38 通过 / 0 失败 / 5 跳过 / 180 秒**；`cargo test` 336+29+2；clippy 干净。
> 五个新探针：`completions.sh`(11) / `ffi-header-sync.sh`(4) / `verify_only.sh`(8) /
> `caps_protocol.py`(5) / `head_encrypted.sh`(5)。**下一轮：按方案 ①A（预览树）开工。**
>
> **2026-10-29 · 0.9.29 已出包并装机（收尾轮）** —— 五处版本号同步（`Cargo.toml` /
> `gui/pubspec.yaml` / `docs/yshjy.1` 的 `.TH` 由生成器带出 / `docs/PROTOCOL.md` 两份示例 /
> `CHANGELOG.md` 四个「未发布」段改成 `[0.9.29]`）。`dist/` 里：`yshjy_0.9.29_amd64.deb`（9.8M）
> + 两档 `tar.gz`（各 3.8M，`yshjy-` / `yshjy-full-`）。
> 验收：`scripts/build-deb.sh` 自查通过（14 种格式认领）、`scripts/test-packages.sh` **33 项全绿**
> （先红过一次：`dist/` 里最新 tar.gz 还是 0.9.28 —— 跑 `scripts/release.sh` 重出两档后转绿，
> 那条「tar.gz 与 deb 必须同一次构建」的断言就是为此存在的）、`bash tests/all.sh` 33 通过。
> 装机：`sudo apt install ./dist/yshjy_0.9.29_amd64.deb` 已覆盖 0.9.28 → **`dpkg -l yshjy` = 0.9.29**；
> 实测 `--version` 0.9.29、桌面项认领 **14 种格式**、`--help` 有 `--trash-source`、
> GUI 二进制里带窗口尺寸记忆的打点（`gui-window.txt`）。
> **下一轮候选**：`docs/TODO.md` 第三期（结构优化，要设计）或「多平台适配」整块（用户口径：最后做）。
>
> **2026-10-29 · packSize 校验（第二期最后一条，引擎改动）** —— 按头部声明的解压后大小
> 给输出**提前封顶**：多出来的字节一个都不落盘、这一条判数据错误；少读判截断
> （libarchive `entry_bytes_declared` 语义）。
> ① **zip**（`extract.rs`）：原来只有「写完再比对」，声明 1 KB 实际吐 10 GB 的包会把磁盘
>    写满再报错；现在边写边按声明封顶，声明为 0 的老式数据描述符包不封顶。
> ② **7z**（`extract7z.rs`）：`entry.size()` 本来就是声明值，补上同款封顶 + 少读判错。
> ③ **RAR / 裸流做不到，如实记**：`rars 0.11` 回调结构 `ExtractedEntryMeta` 没有
>    unpacked_size（`lib.rs:328-344` 逐个核过）；绕道预扫描建名字表会因 RAR 允许同名条目
>    而误判。裸流（gzip/xz/zstd/bzip2）头里没有解压后大小，gzip 的 ISIZE 还会取模。
> 验收：`tests/pack_size_cap.sh` **10 项全绿**、fuzz 写盘路径 60 秒零崩溃、
> `bash tests/all.sh` 见 `scratch/all14.log`。证据见 `CHANGELOG.md`「未发布 · packSize」四节。
> **下一步**：第二期已全清 —— 剩下的都在「第三期：结构优化」（要设计：预览树 + 部分解压、
> 干扰数据/假签名、caps 按格式声明…）。✅ 装机版已是 **0.9.29**（2026-10-29 装机）。
>
> **2026-10-29 · 第二期收尾（4 条真活 + 2 条「查出来不用做」+ 1 条留单独一轮）**：
> ① **CLI `--trash-source` / `--delete-source`** —— `cli_spec.rs` 加两项（长选项 40→42），
>    `main.rs` 加 `源包处置` 三态；只在成功/部分成功时动原包；`--delete-source` **开工前**
>    二次确认，非交互（quiet/json/stdin 非终端）**拒绝且退出码 2**；7 语言文案同步；
>    探针 `tests/trash_source.sh` **10 项**（回收站真有条目 / 拒绝 / 输 y 删 / 输 n 取消）。
> ② **右键脚本**：只读目录不再静默失败 —— 改投「下载/解压-<包名>」并在通知里说清。
>    原计划「照 FileRoller 给两套菜单」实测做不到（script 菜单文字不能动态变），已在 TODO 记原委。
>    探针 `tests/nautilus_script.sh` **7 项**。
> ③ **补全两处**：zsh `_files -/`、fish `(__fish_complete_directories)`（必须双引号，
>    单引号里 fish 不执行替换）。`cli_spec_match.sh` 7 项全过。
> ④ **两条不用做**（台账又失真）：界面「解完移回收站」开关早在 `settings_page.dart:265-283`；
>    密码卡「这次不保存」前提不成立 —— `config.rs` 有保险丝测试禁止自动写密码本，
>    界面本来就不保存密码，做了是假开关。真做「记住密码」是新功能，等用户点头。
> ⑤ **packSize 校验留单独一轮**：要动 `zip.rs`/`extract_stream.rs` 解码主循环 + fuzz 十分钟，
>    不能和别的条目挤（出问题分不清谁的锅）。这是第二期**唯一剩下的**一条。
> 证据：`cargo test` 336+29+2、clippy 干净、`CHANGELOG.md`「未发布 · 第二期收尾」五节。
> ✅ **装机版已是 0.9.29**（含本轮全部修复，见下方发版记录）。
>
> **2026-10-29 · 界面轮：窗口尺寸记忆（引擎一行没改，只动界面原生层）** —— 用户口径是
> 「每次开都要手动拉窗口」。查下来不是没做，是**做了但从来没生效**，而且生效路径上还埋着
> 一个「越开越大」的基准错位：
> ① 保存只挂在窗口 `destroy` 上，而现在「忙时关窗口 = 藏到托盘」「托盘退出 =
>    `g_application_quit`」两条主路都不一定走到 destroy —— 用户用一个月，
>    `~/.local/state/yshjy/gui-window.txt` **一次都没生成**。改成 configure 后 800ms
>    防抖写盘 + destroy 兜底 flush（定时器在 destroy 里先摘，防野指针）。
> ② 存的是「含 CSD 装饰的 surface 尺寸」（实测宽 +112 / 高 +144），载入却当内容区喂给
>    `set_default_size` → 实测 880x700 → 992x849 → 1104x849 越开越宽。改用
>    `gtk_window_get_size`（不含装饰，同基准）。
> **位置记忆：Wayland 协议不许客户端给自己定位**，本机会话 `Type=wayland` → 不做；
> X11 才做得到，但没有 X11 会话可验收。**`window_manager` 评估过不引**（MIT 没问题，但它在
> Wayland 下同样做不到位置，只是给已有实现再加一份真源）。
> 新探针 `tests/gui_window_state.sh` **8 项全绿**，关键回归是「再开一次尺寸不变（不滚雪球）」；
> 探针默认 skip（会真开窗口），`YSHJY_GUI_VISIBLE_TEST=1` 才跑。`flutter test` 20 条、
> `flutter analyze` 只剩 2 条既有 info。证据见 `CHANGELOG.md`「未发布 · 界面」四节。
> **下一步**：`docs/TODO.md` 第二期第二条（界面：密码卡「这次不保存」勾选）。
> ✅ **装机版 0.9.29 已含本轮修复**（2026-10-29 20:22 装）。
>
> **2026-10-29 · 测试基建轮（引擎一行没改，只动 `tests/` + 文档）**：
> ① TODO 表尾那条「`cargo test --release` 偶发失败」**定性为确定性**：`rar_atomic` /
>    `pack_progress` / `resume` 默认用 `target/debug/yshjy`，而 `all.sh` 第 1 关**只建 release**
>    → 只要改过 `Cargo.toml` / `src` 却没手动 `cargo build` 就必红（实测差 5 分钟，不是 I/O / OOM）。
>    修法：`all.sh` 第 1 关补 `cargo build --quiet`（debug，增量 8 秒、无改动近 0 秒）。
>    **试过「三个探针全改 release」：不可行、别再走** —— release 解 81 MB 只要 151 ms，
>    `rar_atomic` 的中断点（耗时÷3）打不中、`pack_progress` 只剩 1 条进度事件，当场假红。
> ② `pack_progress.sh` 执行自解压 `.sh` 验证产物时漏了 `YSHJY_SFX_NO_OPEN=1` → 每轮回归在
>    用户桌面弹一个文件管理器（被用户当场抓到两次）。已修，验收方式是「journal 里
>    `org.gnome.Nautilus` 激活次数 +0」。**以后会真执行 sfx 产物的探针一律带这个开关。**
> ③ **「解完要不要自动打开文件夹」定了口径**（用户问的）：自解压 `.sh` = 真人在终端跑或双击
>    才开、脚本调用/重定向不开（存根四条判定，假 `xdg-open` 四场景验过）；**界面默认不开**，
>    设置页新增「解压完自动打开输出目录」开关（存 `[general] open_after`，批量只开第一个目录）；
>    命令行永不开。业界依据：WinRAR/Bandizip 都有这个开关、7-Zip 不做、NanaZip 做成选项。
> 回归：`bash tests/all.sh` **30 通过 / 0 失败 / 4 跳过 / 179 秒**（`scratch/all11.log`），
> 整轮跑下来文件管理器激活次数 **+0**；`flutter analyze` 仅一条既有 info、`flutter test` 20 条绿。
> 证据见 `CHANGELOG.md`「未发布 · 2026-10-29」（四节）。
> **还没动的下一步**：`docs/TODO.md` **第二期第一条**（界面：窗口尺寸/位置记忆，要引 `window_manager`，
> 加依赖前先看许可证）。
> **第一期（对齐与止血）三条同日全部完成**：① 重写 `REFERENCE-INDEX.md` 状态列（14 条失真逐条 `grep`
> 复核、另抓到 2 条、10 条证据行号回验 **10/10**）；② **Windows 编译阻塞解除** —— 48 处 unix-only 里
> 只有 `extract.rs:499` 真没隔离，修完出 **13 MB `yshjy.exe`**，wine 能跑、能解真 zip（中文逐字节一致）；
> ③ 进度条补「已用时」（单任务手绘与 `-j 2` 的 `{msg}` 共用一份算法，真实终端抓帧 **102 帧**带已用、
> **0 帧超宽**）。顺手记下 Windows 侧时区占位 bug（时间差 8 小时、跨日错天）→ TODO 平台节。
>
> **本轮（2026-10-28 夜 · 纯文档轮，代码与产物一行没动）**：
> ① 新增 `docs/INTERNALS.md`（代码全景）+ `scripts/scan-src.py`（数字由源码扫出，可重跑）；
> ② 新增 `docs/ROADMAP.md`（体检五类 + 参考项目 16 条 + 四期方案 + 全平台路线）；
> ③ 台账 32 条 ❌/🟡 **逐条核实** → **14 条标未做其实早做完了**、9 条真缺口、1 个真 bug；
> ④ Windows 交叉编译**实测失败**（5 处 unix-only 没 cfg 隔离，`E0433`）；
> ⑤ 参考项目补拆 9 项 → `refs/tech/` 5 份文档（1123 行）。
> **下一步**：等用户点头，按 `docs/ROADMAP.md` 一期开工（对齐止血）。
>
> **★ 最新状态（2026-10-28 夜）· 对外版本 0.9.28「终端/界面收尾」**
> 三件事，全是实测撞出来的（不加新功能）：
> ① **只取封面不认 `-o`/`dest`**（真 bug：`tests/data` 里堆了 36 个副本才暴露；修法是
> `cover_path_in` + `extract_zip_cover_to` 薄包装，顺手改原子写；探针 `protocol_cover_only.py` 25 项）；
> ② **救援模式接进协议与界面**（新命令 `rescue` + `serve` 调度 + Dart `rescue()` + 失败页按钮；
> 探针 `protocol_rescue.py` 22 项）；
> ③ **man 页版本号永远停在旧版**的根因（生成器比对时把整行 `.TH` 抹掉，版本号跟着被抹 → 版本升号后
> 它认为没改、跳过写盘；改成只抹日期）。
> 回归：`bash tests/all.sh` **29 通过 / 0 失败 / 4 跳过（慢测试）/ 179 秒**；`flutter test` 13 条；
> `flutter analyze` 只剩一条既有 info；clippy `-D warnings` 干净。
> **已出包并装机**：`dist/yshjy_0.9.28_amd64.deb`（9.8M）+ 两档 `tar.gz`（各 3.7M，0.9.27 缺的那档补上）；
> `scripts/test-packages.sh` **33 项全绿**；`sudo apt install` 已覆盖 0.9.27 → **`dpkg -l yshjy` = 0.9.28**；
> 装机版实测：菜单认领 **14 种格式**、`Exec=/usr/lib/yshjy/gui/yshjy_gui`、无旧手工副本、
> 「只取封面落 `-o`」生效、`--help` 里 `--pick` / `--rescue` 都在。
> **下一轮**：`docs/TODO.md` §优化建议 已全部清空 —— 剩下的就是「多平台适配」整块（用户口径：最后做）
> 与表尾两条待查（`cargo test --release` 偶发失败、`--pick` 被 Ctrl-C 后光标不恢复）。
>
> **历史：0.9.27「终端体验收口」**
> 一次性做完四件事：① CLI 选项清单升成唯一真源 `src/cli_spec.rs`（37 项对外）；
> ② 补全三件套 + man 页改成从选项表**生成**（补全 18→36，man 22→36），打包前自动重生成；
> ③ 新 `--pick` 全屏勾选（dialoguer）、`--list` 宽终端画表格（comfy-table）；
> ④ `-j 2` 多行进度（indicatif），单任务路径不动。
> 顺带校准 **10 处过时文案**（7 种语言全同步）。探针 **27 项全绿**（新增 3 条）。
> 踩过的坑都在 `CHANGELOG.md` 第十～十一节（README 级重要，别再犯）。
>
> **已出包并装机**：`dist/yshjy_0.9.27_amd64.deb`（9.8M）；`dpkg -l yshjy` = **0.9.27**。
> 装完实测：菜单项认领 **14 种格式**、`/usr/bin/yshjy` 版本正确、**双击 zip / 7z 都指向 yshjy**
> （用户配置里 14 条，不是 root 的 —— 那个 su 事故点没复发）、三种补全 **37 个长选项**
> 且与源码树逐字节一致、man 页 **36 项**含 `--pick`。

> **待办只看一处：`docs/TODO.md`** —— 阶段 0（体检 T1–T9）**已完成**，八项证据在 `docs/AUDIT-PHASE0.md`；
> 阶段 1 已按体检证据重排成四个梯队（T5 装包检查按用户要求放最后）。
> **2026-10-28 状态：阶段 0 的 T5「装包与发布检查」已完成 —— 全项目待办清零** ——
> 对外版本号定为 **0.9.26**（就在批次号上去掉 `-c`，五处同步：Cargo / pubspec / man 的 `.TH` /
> PROTOCOL 两份示例 / CHANGELOG+HANDOFF）。**0.9.26 已出包**：`dist/yshjy_0.9.26_amd64.deb`（9.7M）
> + 两档 `tar.gz`，`scripts/test-packages.sh` 重写后 **33 项全绿**（元数据/文件表/属性/依赖闭包/
> 功能实测/tar.gz 同源），`build-deb.sh` 自检从 5 项扩到 11 项 + 三条事故断言。
> **修掉两个真事故**（都固化成断言，别回退）：
> ① `postinst` 里 `su -s /bin/sh $SUDO_USER -c "xdg-mime default …"` —— 不带横杠的 su **继承 root 的
>    HOME**，默认项写进 `/root/.config/mimeapps.list`，用户账号里一条没写成 → 装完双击压缩包还是
>    Nautilus。修法：`getent passwd` 取真家目录 + 显式 `HOME=` / `XDG_CONFIG_HOME=`。
> ② 0.9.25-c 的桌面项只认领 **7 种**格式（源码早 14 种）→ 包是拿旧 desktop 打的，7 种格式用户摸不到。
>    修法：自查死断言「包内 `MimeType` 计数 = 14」，数不对拒绝出包。
> 顺带：man 页打进包 664 → 显式 644；`test-packages.sh` 原来写死 0.9.21-c（验的是三个月前的老包）
> → 自动取最新产物。
> **AppImage：本轮不做**（不是做不出来，是本机 glibc 2.41 为基线做出来的没人能跑；要用 bookworm
> 容器基线 + 排除图形栈 + 只捆 GTK3，体积 60–80 MB vs deb 9.7 MB，收益对不上）。完整配方留在
> `docs/RELEASE-CHECK.md` §4，将来要做照抄，不用重新调研。
> **2026-10-28 第二批（参考项目 + 方向定调）**：用户拍板「核心已完成，剩下是**终端界面 + 各平台图形界面**」，
> 并要求先看业界怎么做。九份参考真身已拉到 **`refs/upstream/`**（`rustdesk` / `localsend` / `hiddify-app` /
> `ouch` / `tauri-bundler` / `cargo-dist` / `fastforge` / `flutter_rust_bridge` / `clap`，约 260 MB），
> 台账写在 **`docs/REFERENCE-INDEX.md` §9**（含源码行号，下次照抄不用重查）。**三条结论**：
> ① 补全与 man 页改成 **clap 构建期自动生成** —— 实测 `--help` 有 35 个长选项、手写补全只登记 18 个，手抄必然漂移；
> ② 移动端桥接走 **ffigen**（我们 `ffi/` 的 C ABI 已就绪，引擎不用改），**不走 flutter_rust_bridge**；
> ③ 出包 CI **必须钉 ubuntu-22.04 基线**（同 AppImage 那次翻车的坑）。
> **用户改口径（2026-10-28，以最后一次为准）：先做优化清单，GitHub 需要时再上** —— 项目是开源的迟早要推，
> 但不作为硬前置。先做「本机能出的包 + 界面功能」：Linux deb/tar 现成；Windows **命令行** exe 可交叉编译
> （目标与 mingw 已装，wine 可本机验）；安卓 APK 可出但需装 Android SDK + NDK（8–10 G，待用户点头）。
> **出不了**：Windows 图形界面版、macOS、iOS —— 这三个平台必须在对应系统上编，别承诺。
> **下一步**：按 `docs/CLI-GUI-PLAN.md` 第 1 行开工（补全/man 自动生成）。

> **2026-10-27 状态：梯队四第 12 项（RAR 接原子写入）已完成 —— 梯队四全清** —— 伪装 / 自解压 / 普通三条路径都报
> `PACK_PROGRESS`；新增 `pack::PackTick`（`Wrote` + `TotalHint`，拼装段换分母）、
> `build*_ctx` 三兄弟、`拷贝带进度`（替掉零进度的 `io::copy`）、节流 500→200 ms。
> **CLI 与协议是两条代码路径**：`api.rs` 的 `pack` 命令（界面走）和 `main.rs` 的 `do_pack`（CLI 走）
> 都要接 —— 只改前者时拿真二进制实测仍是 0 条事件。实测 2 GB 伪装包：82 条事件、
> 0→95%（2048 MiB）→ 分母放大到 4096 MiB → 100%，无回退。新探针 `tests/pack_progress.sh`。
> 验收：单测 332 + 协议 12 + doctest 2；clippy 干净；`tests/all.sh` **23 通过 / 0 失败 / 143 秒**。
> **下一轮等用户定**：要么进 T5（装包检查，用户要求放最后），要么打包发版。
>
> **RAR 原子写入的调研结论（别再重查）**：`rars` 的 `ExtractedEntryMeta` 只有
> `name / file_time / file_attr / attr_source / mtime_refinement / is_directory` ——
> **没有条目大小、没有 CRC**，所以回调里无法判断「库写完了还是中途出错」，`Drop` 只能当作
> 「写入结束」。因此做法是「Drop 时 `finish()` 原子改名」，取消则丢 part。
> 有意接受的局限：库因 CRC 失败提前 drop writer 时，已写字节仍会落地（改前同样会，非退步）。
> 彻底解决要迁 `rars::extract_with_control_and_errors`（`on_error` 能拿逐条目错误），
> 但要重写整个回调层（`decide` 收 `ArchiveMember`），风险大，未做。
> **中断实测已补上**：装了 `rar`（仅造样本用）后新增 `tests/rar_atomic.sh` 并挂进 `all.sh` ——
> SIGKILL 打断 → 只剩 `.yshjy-part`、正式名一个都没出现；重试全齐零副本零残留；删文件能自愈。
> 踩到的坑：**`cargo test` 不会重建 `target/debug/yshjy`**，第一次拿旧二进制跑出「半截 + (1) 副本」
> 的旧症状，差点误判成修复无效 —— 探针已加「源码比二进制新就拒跑」自检。
>
> **历史：梯队四第 10 项（断点续传）已完成** —— 新增 `src/resume.rs`（条目级续传 +
> JSONL 记录 + 三重校验：包没变 / 文件还在 / 大小对得上），四个解压实现共 14 处接线，
> 新事件 `RESUMED`、`caps.resume` 改 true、CLI 七语言文案、界面备注一句。
> 验收：`tests/resume.sh` **15 项全过**（SIGINT 解到 83/120 打断 → 重开报已完成 86 项 →
> `diff -r` + sha256 清单与参照**逐字节一致**；SIGKILL 硬杀同样能续；删文件反证能补回；
> **随机 kill 点 12/12 轮**字节一致；临界区定向覆盖重解不留副本）；`caps` 另加 `resume_level: "entry"`；
> 单测 330 通过 / 0 失败；clippy 干净；协议契约 12 项；`flutter test` 13 条；`flutter build linux --debug` 成功。
> **四个踩出来的坑**（别再犯）：
> ① 续传记录**一条一次 write、不攒批不 fsync** —— 攒批时被 `kill -9` 会一条都没落盘，
>    而点取消却能续，最糟的错配；
> ② 跳过检查**必须在同名策略之前**，否则续传要跳的文件会被当成「已存在冲突」记进跳过列表；
> ③ **「改名成功 → 记录没写」有个极窄临界区**：磁盘上文件是好的、记录里没有它，
>    按默认 `Rename` 会多出一个 `文件 (1).bin`。固定断点测不出来，**随机 kill 压测**第一轮
>    就抓到 —— 修法是「续传会话里没记录却已存在 → 强制覆盖重解」（`ctx.同名策略()`）；
> ④ 诊断时别把被测进程放进会提前退出的管道（`| grep -m1` 会 SIGPIPE 把它杀掉，
>    看起来像引擎 bug，其实是我自己杀的）。
> **方法论教训（比上面四条更重要）**：做「中断 / 并发 / 时序」这类功能，先花二十分钟看别人怎么踩坑
> —— itchio/savior 的三级续传能力（none/entry/block）、peel 的随机 kill 点压测，都是现成的答案。
> 这次是用户追问「参考别人项目了吗」才去查的，之前一直在自己推演。
> **下一轮做梯队四第 11 项：伪装 / 自解压打包也报进度**（`src/pack/` 的 `PACK_PROGRESS` 只覆盖普通打包；
> 验收标准是「打 2 GB 伪装包，进度条在动」），一次一行不插队。
>
> **历史记录：梯队四第 9 项（托盘 / 后台）已完成** —— 四批提交：
> `ba64554`（GNOME 顶栏图标 + 显示窗口/退出菜单）/ `0b052e4`（解压中点关窗口 = 藏到托盘继续跑）
> / `e11c16f`（托盘状态行显示真实百分比，涨 2% 才刷一次）/ `2e9b048`（托盘「退出」先让引擎在
> 数据块边界收尾再退，`caps.background` 改 true）。
> 验收：界面测试 13 条全过；`flutter build linux --debug` 成功；`cargo test` 333 通过 / 0 失败
> （319 单测 + 12 协议 + 2 doctest）；`cargo clippy -- -D warnings` 干净。
> **下一轮做梯队四第 10 项：断点续传**（动 `src/job.rs` + 协议加字段 + 状态持久化；
> 验收标准是「中断后重开能接着解，且已验证字节一致」），一次一行不插队。
>
> **2026-10-24 状态**：源码 **0.9.26**（解压原子写入，六条落盘路径）；**装机仍是 0.9.25-c**
> （`dist/yshjy_0.9.26_amd64.deb` 尚未打，用户下次要体验再打）。
> 旧手工残留已清（`~/.local/opt/yshjy` = 0.9.21-c、`~/.local/bin/yshjy-gui` 断链）。
> 若应用菜单点开仍报「找不到 /home/user/.local/opt/yshjy/yshjy_gui」，是 GNOME 会话缓存，
> **注销重新登录**即好（Wayland 下无法单独重启 shell）。
>
> **L3 轮（N1–N7）已全部交付：0.9.25-c（2026-10-24）—— 本轮到此结束**
> 计划、实测证据与批次记录见 **`docs/LINUX-FULL-PLAN.md`**；**待办清单见 `docs/TODO.md`**。
> L3 轮做完的事（提交 352b135 / c6f729c / 64e4c4d）：
> - **N1 界面文案表**：`gui/lib/humanize.dart` —— 错误码 17 / 诊断码 10 / 条目结果 6，全中文；
>   查不到的码**不再显示原始码**；修掉「界面查 `PASSWORD_REQUIRED` 而引擎发 `NEED_PASSWORD`」的真 bug
> - **N2 打包页进度条**：订阅 `PACK_PROGRESS`，进度条 + 百分比 + 当前文件
> - **N3 `tests/all.sh`**：一条命令跑 编译 → clippy(-D warnings) → 单元测试 → 7 个协议探针 →
>   冒烟 → 界面测试 → 右键脚本语法；`--slow` 跑大 RAR/分卷；**15 通过 / 0 失败**
> - **N4 批量单行取消（D5）**：引擎 `CancelToken` 按任务号取消 + 协议 `cancel.job` +
>   界面行内「取消这一项」；只有正在跑的就是它才真打断；显示「已取消」不显示「失败」
> - **N5 核实**：D4（Del 键删队列条目）早批次已实现
> - **N6 拖入文件夹提示**：队列顶部明说「会自己翻开找压缩包和分卷（只读文件头，不动文件）」
> - **N7 右键菜单**：`gui/linux/nautilus-script-yshjy` + `scripts/install-nautilus.sh`（用户级，
>   已装到本机）+ deb 装到 `/usr/share/nautilus/scripts/`；用法：选中压缩包 → 右键 →「脚本」→
>   「用杨三海解压」
> - 三个坑记在代码/脚本注释里：**bash 的 local 不认中文变量名**、**Dart 也不认中文变量名**、
>   `cargo fmt --check` 只作提示（手写紧凑风格，rustfmt 会重排上千行）
>
> **下一轮做什么**：只看 `docs/TODO.md` 第 1 行（协议契约测试），做完划掉一行；
> 另外**系统里装的还是 0.9.24-c，要重新 `scripts/build-deb.sh` 打包装上**
> 才能看到这轮的界面改动。
>
> **L 批前两刀（0.9.25-c 首次交付，2026-10-24）**
> 计划与实测盘点见 **`docs/LINUX-FULL-PLAN.md`**（X1–X6 + 老账 + L1–L7 分批）。
> 本批做完（提交后）：
> - **X1 RAR 分卷**：`as_rar_volumes` 认三种命名 + `rars::extract_volumes_to` 多卷流式；
>   RAR5 四卷样本 sha256 与 unrar 一致
> - **X2 RAR 256 MiB 上限删除**：`read_path` 流式；300 MiB 实测 6.32 秒解完；
>   只有管道流回退整读（上限仍 256 MiB，文案改为「给路径就没有限制」）
> - **X3 tar 符号链接/硬链接真的还原**（安全链接才建，逃逸的一律拒绝并说明）
> - **X4/X5 文案**：`stats.failed` 不再把「跳过」标成 `DATA_ERROR`；新增 `ItemResult::人话()`，CLI 全换中文
> - **D8**：失败条目带 `(名字, 码, 原因)`，协议多 `human` / `reason`
> - 回归：`cargo test` 303 passed / smoke 25/25 / 6 探针全过 / clippy 干净
> 下一批（按清单往下）：**打包进度事件** → 协议契约测试 → D5 单行取消 / D4 Del 键 / D6 文案 →
> 右键菜单 → tests/all.sh；断点续传与托盘排在后面（工程量大，先记账）
>
> **第 23 批已完成（v0.9.24-c，2026-10-24）——「能力补齐」6/7 项 + 1 项评估结案**
> - P23-1 文件级挑选：协议 `select`（表达式 / 序号数组）+ 解压页逐条勾选（实测只解勾中的）
> - P23-2 单任务输出目录：解压页「高级选项」里选（默认仍是包旁边）
> - P23-3 文件名编码：界面下拉 GBK/Big5/Shift-JIS/UTF-8（接引擎 `--encoding`）
> - P23-4 压缩级别：协议 `level` + CLI `--level` + 界面下拉（顺带修 level=0 打包失败）
> - P23-5 自解压：支持解压码 + 进度显示 + Linux `.sh`（`--sfx-linux`）；macOS 排最后
> - P23-7 `--serve` 不丢结果（D16 结案）
> - P23-6 7z 写包：**评估后本批不做**（理由见 CHANGELOG 与 GUI-PLAN §7 D22）
> 提交链：3f88e4e（组1 引擎/协议）→ abcd2a6（组2 界面）→ c21ec75（组3a 存根密码/进度）
> → ff5c03c（组3b Linux .sh）→ 收尾提交（版本 0.9.24-c / 文档 / 打包）
> → b26e3e7（补丁：小窗口下列表被挤成 0 高、滚轮翻不动 —— 已选行压成一行、
>   高级选项改弹窗、窗口最小高度 400→560、列表常显滚动条、预览页整页接滚轮）
>
> **第 23.5 批已完成（v0.9.23-c，2026-10-18，工作室接手交付）——「成熟感最小集」**：
> ① **deb 现在带图形界面**：包含 `yshjy_gui` + `libapp.so`、应用菜单项「杨三海解压」、
>    256×256 图标、`postinst/postrm` 刷新菜单缓存。装完直接点开，**不再需要手动跑
>    `install-gui.sh`**。包体积 1.9MB → 9.5MB。
> ② **桌面通知**：解压完成 / 部分完成 / 解压失败 / 打包完成各发一条（`notify-send`，8 秒）；
>    命令缺失或超时静默跳过；设置页新增「处理完成后发桌面通知」开关。
> ③ **发布一致性**：`build-deb.sh` / `release.sh` 打包前比对 `Cargo.toml` 与
>    `gui/pubspec.yaml` 版本；出包后自查 CLI / GUI / libapp.so / desktop / 图标 / 版本号。
> **引擎 `src/` 一行未改**。回归：`flutter test test/notify_test.dart` 2/2；
> `build-deb.sh` 自查 5 项通过；`dpkg -l` 实装状态正常。
> ⚠️ **版本号已递进到 0.9.23-c**（23.5 批占用）；原第 23 批功能顺延为 **0.9.24-c**
> （清单在本文档下方与 `docs/GUI-PLAN.md` §8）。
> 工作室档案：`~/hermes/studio/projects/yshjy/`（PRD / ARCH / PLAN / ACCEPTANCE / RELEASE /
> REVIEW / MATURITY / EXISTING）。
>
> **第 23 批（0.9.23-c，功能 7 项）尚未开始**，清单仍在本文档下方与 `docs/GUI-PLAN.md` §8。
>
> **第 22 批已完成（v0.9.22-c，2026-10-18）**：
> 伪装文件自选（任意类型外壳）+ 自解压 exe（自研 Rust 存根）+ 同类「限制选择」修复
> （解压页白名单 / 分卷大小静默 / 输出后缀名不符实 / 输出覆盖源文件防呆）。
> 三个回归脚本：`tests/protocol_pack_cover.py`、`tests/sfx_stub_test.py`、`tests/protocol_sfx.py`。
>
> ⚠️ **发布前必做**：`bash scripts/build-sfx-stub.sh`（需要 `gcc-mingw-w64-x86-64`）
> 生成真正的 Windows PE 存根。开发机上临时放的是 Linux 版存根（要靠
> `YSHJY_ALLOW_NON_PE_STUB=1` 才编得过），**绝不能发布**；`build.rs` 会拦非 PE。
>
> 第 21 批（v0.9.21-c）：组 D 平台地基 / 组 C 自动下钻 / 组 B 双向协议 /
> 组 A-界面 / 组 E CLI 扫尾 全部落地（每组一个提交，可单独回滚）。
> 下一批 0.9.23-c 清单见 `docs/GUI-PLAN.md` §8（文件级挑选 / 单任务输出目录 /
> 编码切换 / 压缩级别 / 自解压增强 / 7z 写包 / 管道多命令）。
> 第 24 批**定位已调整（2026-10-24 用户拍板）**：第一优先级不是补功能，是
> 「先让真人用起来 + 把敢改的底子铺好」——
>   24.1 真人试跑（3～5 人，产出 docs/USER-FEEDBACK.md；拿到反馈前不动代码）
>   24.2 tests/all.sh：一条命令跑完全部验证（探针不再手动一个个跑）
>   24.3 用户每批亲手改一件小事（文案/间距/一条探针）
>   24.4 D21 文案审计 + pack 进度事件 + 五态审计（先补失败/部分成功）
> 原「成熟度基线」清单（格式注册表 / 契约测试 / 桌面集成 / CI / 安装包矩阵）
> 降为参考，等有用户诉求再取用；参考项目在 `refs/外部-归档软件/`。

## 第 21 批进度（0.9.21-c 已完成）

    ✅ 组D-1 引擎路径跨平台：src/paths.rs（Windows LOCALAPPDATA / macOS Application Support /
       Unix XDG），crash.rs + report.rs 共用；3 个单测
    ✅ 组D-2 命令实现抽成 src/api.rs（serve 与 C ABI 共用一份命令语义）
    ✅ 组D-3 稳定 C ABI：ffi/ 独立 crate（cdylib+staticlib）+ include/yshjy.h + docs/FFI.md
       实测：ctypes 直调 .so，hello/list/extract/事件回调/取消 全 PASS
    ✅ 组D-4 Dart EngineTransport + ProcessTransport（gui/lib/transport/engine_transport.dart）
    ✅ 组D-5 gui/lib/platform_paths.dart：5 处写死 state 路径 + 4 处 xdg-open 全部跨平台
    ✅ 组A-引擎部分 list 结果带 cover{size,kind,ext}；extract 支持 extract:false（只提封面）
    ✅ 组C 嵌套包自动下钻一层：src/drill.rs + extract.drill（默认关；最多 8 个；
       跳过输入自身；失败只记 nested[].outcome）—— 协议实测 mp4→zip→文件 全通
    ✅ 组B 双向协议：src/ask.rs（ASK_USER / answer / skip）
       · extract/batch 的 ask:true 才启用询问；60 秒不答=跳过；一次任务只问一次
       · 取消/跳过改走 Cancelled 语义（ZIP/7z/单流三个循环都修了）
       · 实测：continue→Success / skip→Cancelled / batch 只跳一项下一项照跑 /
         不带 ask:true 仍 0.03s 硬失败
    ✅ 组A-界面 封面勾选行 / 打包模式二选一 / ASK_USER 弹窗 + 跳过按钮
       · home_page 两行勾选（want-cover / want-extract）、只提封面文案、置灰规则
       · pack_page 模式二选一，默认普通压缩（.zip），伪装是特殊功能
       · 引擎 pack 加 mode:"plain"（build_plain），协议实测 17 项全过
       · 顺带修 D17：小包压缩比误报（64 MiB 起检）
    ✅ 组E CLI 扫尾（--config / --layout / --cover / --password-file / --no-auto-password /
       -v / -vv / --log / --no-color / --keep）
       · 实测 16 项断言全过（tests/cli-e-probe.py）；7 种语言包 --help 同步
       · --layout L5/L6 明确拒绝、L3/L4 报未实现；--keep 打印中间 ZIP 路径

## 第 21 批验收补丁（2026-10-18 凌晨，用户实际试用后）

    ✅ D19 默认输出目录：CLI 从「进程当前目录」改成「包所在目录」，
       完成行显示 `canonicalize` 后的绝对路径（542746b）
    ✅ D20 拼错选项必须报错：`-extract-cover`（少一个横线）曾被静默忽略 →
       默认解压照跑（实测把 5.2 GB 包完整解了一遍）；现在报错 + 拼写提示
    ✅ 交互式输解压码：终端里遇到带锁的包先问（不回显、直接回车=放弃）；
       `-q` / `--json` / 管道输入保持快速失败（脚本不会挂死）
    ✅ 安装包：dist/ 下 `0.9.21-c` 常用版 + 全能版 tar.gz + deb，
       解包实测可运行（见本轮总结）

## 第 20 批进度（0.9.20-c 目标）

    ✅ 组0 文档与版本：PROTOCOL 补 scan/batch/report；版本 0.9.19-c
    ✅ 组1 密码链路：多编码 UTF-8→GBK→UTF-16LE（zip/rar）、7z 加密标记如实上报、
       enc_kind 入协议 + 界面强度提示、密码框眼睛图标（D14/D15 已记录）
    ✅ 组2 封面分离：`--extract-cover` + 协议 extract 带 cover + 完成卡一行
       （真包实测切出 361.3 MB / 1080p H.264+AAC，ffmpeg 60 秒零错误，原包未动）
    ✅ 组3 密码界面：PASSWORD_TRY「正在试第 N/M 个」+ 常用解压码 chips + 速度/ETA
       （D14 已修：错密码说「解压码不对」；预览勾选/只解一部分待 select 协议，记下一批）
    ✅ 组4 输出与设置：设置页可写（输出目录 / 重名策略 / 原包进回收站）、
       XDG 回收站（永不 rm）、历史页与批量失败项可点开目录、失败原因走人话
    ✅ 组5 打包端（引擎+CLI）：--split 分卷（.001…，sha256sum -c 通过）、
       .sha256 校验和、.说明.txt 说明书（默认开，--no-checksum / --no-readme 可关）
    ✅ 组5b GUI 打包页三开关（分卷 / SHA-256 / 说明.txt）
    ✅ 组6a D13 优雅关闭（解压中关窗口先问一句 + 先 cancel 再退）
    ✅ 组6b-1 EOCD 分块倒扫（64 KiB 块 + 20 B 重叠；5 MiB 垃圾尾 9 ms 命中）
       + 条目读满校验（声明大小 ≠ 实际写出 → DataError，不再假成功）
    ✅ 组6b-2 0x7075 Unicode Path（先验 CRC）+ 预览统一走自研 CD 解析
       （D15 顺带解决：ZIP AES 强度能给到 aes128/192/256）
    ✅ 组6b-3 全文件扫描双维度进度（SCAN_PROGRESS，500 ms 节流，GUI 显示已扫/总量）
    ✅ 组6b-4 --encoding 手动代码页（big5 / sjis / gbk / utf-8；实测 Big5 文件名从乱码变正确）
    ✅ 组6b-5 嵌套包提示（解压后扫产物，完成卡「里面还有 N 个压缩包 [继续解压]」；不自动下钻）
    ✅ 组6b-6 7z 内存预检（LZMA2 字典估算 + MemAvailable 比较，大字典不再 OOM 静默消失）
    ✅ 组7 完成卡分享 / 第一屏收口 / 空状态 / 解压预告 / Del 键 / 主题切换 /
       新手引导 / 窗口尺寸记忆
    ⬜ 组6b 剩余：限额改询问（嵌套自动下钻留 0.9.21-c）
    ⬜ 组7 剩余：单行取消（要引擎支持逐条跳过）
       → 组8 平台地基 → 组9 CLI

## GUI 完整化计划（2026-10-17）

**完整计划表见 `docs/GUI-PLAN.md`**（P1–P12，每项都带参考项目出处 / 实现落点 / 验收标准）。
已交付到 **第 19 批**（提交 768aa5a）；当前 HEAD 为版本 `0.9.19-c` 的文档/版本收口提交：

    第16批 拖放 + 一键诊断包 + 解压后提示启动入口
    第17批 应用外壳：左侧导航 + 解压/打包/历史/设置/关于五页 + 状态栏
    第18批 P1 批量队列（协议 scan/batch + 队列 UI + Sunpack 三段式汇总卡）D1–D6 已记录
    第19批 P3 缺分卷检测 + P2 智能解压上屏（修掉死错误码 MissingVolume）D7–D9

下一步 **第 20 批 = 0.9.20-c：AUDIT-2026-10-17b 的全部问题并入这一轮**（用户 2026-10-17 晚要求）——
密码链路 / 封面分离 / 密码界面 / 输出与设置 / 打包开关 / 引擎健壮性 / 界面打磨 /
平台地基 / CLI，共 9 组，清单见 docs/GUI-PLAN.md §5.1。**按组提交，做不完的组明确顺延**。

🔎 **动手前先查参考索引**：`docs/REFERENCE-INDEX.md` —— 每个设计项都对应到 `refs/` 的
   具体「文件:行号」（含参考源码位置）和我们的代码落点；补功能前必看，别凭印象写。

📋 **设计与实现对照审查（2026-10-17，用户要求）**：`docs/AUDIT-2026-10-17.md`
📋 **第二次对照（2026-10-17 晚，设计增补 §17/§18 后）**：`docs/AUDIT-2026-10-17b.md`
  · 15 项用户可感知缺口 + 14 项内部/平台 + 4 处文档落后于实现，全部带 文件:行号
  · **已全部并入第 20 批（0.9.20-c）清单，见 GUI-PLAN §5.1 的 9 组**
  · 设计缺口 6 处（中文密码编码 / 封面分离 / 输出目录+原包处理 / 打包 10 开关 /
    预览半成品 / CLI 11 参数），全部带 文件:行号
  · 实现优于设计 9 处（别改回去）
  · 参考台账 383 条 🔄 里筛出 12 条值得做的（含成本与优先级）
  · 竞品界面 7 项遗漏 + 4 项抄错方向
  · **建议第 20 批顺序：P4 前置（中文密码/7z 加密标记/眼睛图标）→ P2b 封面分离
    → P4 本体 → P5（含输出目录+回收站）→ 打包开关**

✅ **GUI 已更新并实机验证通过（2026-10-17 20:26）**：
  · `lib/libapp.so` 20:26 —— 含第 18/19 批界面改动。**看时间要以 `lib/libapp.so` 为准**，
    `yshjy_gui` 是启动器（只在 C++ 层变时才更新，现仍是 19:30），拿它当整体时间戳会误判"旧版"
  · 引擎已拷进 bundle：`bundle/yshjy` = `target/release/yshjy`（0.9.0）
  · 实机状态栏：`● 就绪  状态目录 ~/.local/state/yshjy  引擎 0.9.0`，无 ProcessException
  · 进程：GUI 1 个 + 引擎 1 个（双引擎竞态已修，见 GUI-PLAN D10）

⚠️ **每次 `flutter build` 都会清掉 bundle 里手工放的引擎**（D11）——顺序不能反：

    cd gui && flutter build linux --release
    cp ~/hermes/yshjy/target/release/yshjy gui/build/linux/x64/release/bundle/

  反了就是「引擎未就绪 (ProcessException)」。引擎查找顺序：`YSHJY_BIN` →
  同目录 `yshjy` → PATH（本机 PATH 里没装 yshjy，所以只能靠前两条）。

> 最后更新：2026-10-17（**v0.9.21-c**，第 21 批完成；重开对话先看顶部进度块和「GUI 完整化计划」）
>
> **版本号规则（用户定，2026-10-17 晚修订）**：批次号 = 版本号，**内测期加 `-c`**
> —— 第 19 批 `0.9.19-c`，第 20 批 `0.9.20-c`，依次类推。
> 写法必须是连字符：Cargo / pubspec 都按 semver 解析，`0.9.19.c` 会被拒绝。
> **主版本已从 0.0 抬到 0.9**（用户 2026-10-17 晚拍板）：`0.9.19-c > 0.9.0`，对外不再显旧。
> `-c` 只是内测标记，不影响版本排序（semver 里 0.9.19-c < 0.9.19，但 > 0.9.0）。
>
> **平台目标（用户定）**：**全平台** —— Linux（当前开发/测试机）/ Windows / macOS /
> Android / iOS。现在只在 Linux 上跑是开发机限制，不是平台定位。iOS 需要 FFI
> （禁止子进程），架构约束见 DESIGN §十八；引擎已能产 `.so/.a/.rlib`。
> 项目路径：`/home/user/hermes/yshjy/`

---

## 一句话说明

**yshjy（杨三海解压）** —— 把「伪装成 MP4/MKV/图片的压缩包」一键还原的工具，
也能反向打包成伪装包。Rust 写的，单文件、零外部程序依赖、全开源许可。

用户最早的问题是：网盘下的 `.mp4.zip` 在 Linux 上**所有工具都打不开**。

---

## 当前进度：v0.9.0 已定稿，发布包已打在本地（**还没上传发布**）

```
git 提交       782817d（v0.9 共 10 批 + 3 次文档收口；每批一个提交、都可回滚）
               6955a3a  第1批 统一解压器接口地基（新增 src/unpack.rs）
               2b4f7c5  第2批 安全护栏 + 密码抓取词边界修复
               875ee0a  第3批 统一接口接进编排层（job.rs）
               e38c299  第4批 trait 加 list()，ZIP 先落地
               294e647  第5批 预览完全归位（preview.rs 也走统一接口）
               b9af71e  第6批 性能：别让 zip 库自己检测偏移（--list 快约 1200 倍）
               e1610fe  第7批 fuzz 三个目标入库（用法与记录见 fuzz/README.md）
               85e710b  第8批 诊断包 --report（报障用；含隐私红线）
               b1f32c4  第9批 诊断包实测逮到并修掉三个问题
               782817d  第10批 tar 列清单 + 救援接事件 + 打包/救援文案归语言包
               fa9be88  第11批 崩溃兜底 + 单任务隔离 + 本地时区 + 图像封面 + 诊断包带崩溃记录
               83e098f  分发层：shell 补全 / man / deb 脚本
               （第12批）契约服务 --serve + docs/PROTOCOL.md（提交见 git log 顶部）
源码           17915 行 / 35 个文件（另有 fuzz/ 三个目标）
测试           277 单元 + 25 冒烟 + 7 组协议冒烟（全过）
               fuzz 首次体检：2787 万次畸形输入，**零 crash**
代码规范       clippy -D warnings 零警告
语言包         7 种（zh-CN / zh-TW / en-US / ja-JP / ko-KR / ru-RU / es-ES），中文包 116 条键
发布包         dist/yshjy-0.9.0-linux-x86_64.tar.gz（2.4 MB）
               dist/yshjy-full-0.9.0-linux-x86_64.tar.gz（2.4 MB）
               两个 .sha256 都校验过；包内二进制跑 25 个冒烟场景全过
```

### 本轮（v0.9 第 1~3 批）做完的

| 项 | 状态 | 说明 |
|---|---|---|
| 统一解压器接口 | ✅ 已落地 | `src/unpack.rs`：`trait InArchive` + 静态有序注册表 `HANDLERS` + `Caps` 能力表 + 统一参数 `UnpackOptions`（四份合并成一份）+ `Source`（只认一条流）。**加格式三步**：写 impl → HANDLERS 加一行 → 补能力表测试 |
| 接口接线进 job.rs | ✅ 已落地 | `extract_target` 从「五段平行代码」变成「准备源 → 查注册表 → 交给解压器」；7z / RAR 也改走一条流（新增 `extract_7z_stream_ctx` / `extract_rar_stream_ctx`）；`Source.path` 删掉（没人读了）。全项目唯一按格式取实现的地方是 `unpack::find_or_err` |
| 预览（`list()`）归位 | ✅ 已落地 | `trait InArchive` 新增 `list()`（默认实现**明确报未实现**，绝不给空清单）；`preview.rs` 的三个预览函数改成按「一条流」，`preview_target` 也走注册表；5 个格式全部实现（tar 如实答「信息不完整」）。**加格式不用改 `preview.rs`** |
| 安全护栏 | ✅ | 候选数夹硬上限 64；删掉 `try_common` 空壳开关；「只试不存」写进代码 + 测试保险丝；README 新增「本工具不做的事」 |
| 密码抓取词边界（真 bug） | ✅ | `251 passed` 曾被抓出垃圾候选 `ed`；现按词边界判定 |
| 两处松绑 | ✅ | `extract_tar_ctx` 的 `'static` → `'a`；`ExtractReport` 新增 `repaired` 字段 |
| 性能：`zip` 库别自己检测偏移 | ✅ | 三处打开点全改成 `打开已知偏移()`：`--list` 8.7GB **109 秒 → 0.09 秒**；解压 5.9GB **129 秒 → 55 秒**（产物 SHA 逐字节一致） |
| fuzz 模糊测试 | ✅ 首次体检通过 | 三个目标（列清单 / ZIP 定位 / 单流识别）；60 秒 × 3 = **2787 万次**畸形输入、**零 crash**。⚠️ 60 秒 ≠ 证明安全（`locate_zip` 覆盖率只 138），怎么跑久见 `fuzz/README.md` |
| 诊断包 `--report` | ✅ 已落地 **并实测通过** | 生成能直接贴出去的 zip：README + meta.json + findings.json + config 快照；`--no-paths` 脱敏路径。**修正了设计的一处漏洞**（原本要记「完整命令行」，会把 `-p 密码` 写进去）。实测还逮到三个真问题并修掉：`-p` 的值被当成目标文件、磁盘余量恒为 null、zip 条目时间戳 1980 —— 见第 9 批。隐私实测：假密码 0 次出现、`--no-paths` 下真路径 0 次出现 |
| tar 列清单（第 10 批） | ✅ | 顺序格式没有索引，只能把整条流的**头**扫一遍（不读数据体）。序号 1-based、目录也占号、中文名宽松解码、mtime / 可执行标记照实给；中途取消或坏条目就 `complete: false`，不把半份清单说成全部。剥壳逻辑抽成 `open_tar_stream()`，**解压与预览共用一份**。实测裸 tar / tar.gz / tar.xz 三种全通 |
| 救援路径接事件（第 10 批） | ✅ | 新增 `extract_by_locals_ctx`（旧入口保留为 quiet 包装）。「解出了可执行文件」从内部 `warnings` 改走 `Diagnostic::RiskyEntry` —— 以前外壳根本看不见；顺带补了 EntryFinished（Ok / Encrypted / CrcError / DataError）。端到端实测：坏包里的 `run.exe` 解出来后终端真的打出了警告 |
| 打包/救援文案归位（第 10 批） | ✅ | 删掉 `PackReport::summary()`（引擎拼中文违反分层），文案搬回外壳查语言包；两处重复的救援清单打印合并成 `打印救援清单()`，体量统一 `人类可读()`（不再「字节」+ 一律 GB）。7 个语言包补 25 条键，中/英/日实测正常 |

**真实包复验（第 3 批，改了核心解压路径后必做）**：

```
5.9 GB 包 PxoPiyCQKv9gsqFX_1.mp4.zip → 示例包.7z.002
   SHA-256 = 7dda988370e9403eb07eb00c0a0949477b1e8c0df5d3f0026aecd2491203fb59  ✓ 与基准一致（129 秒）
8.7 GB 包 lvIesSotSDRhnZfL_1.mp4.zip → 示例包.7z.001
   8,589,934,592 字节，SHA-256 = e6269244dd96fb3740f359c6a9aa7b12465d365ffa6b4cb009899d22316962dd  ✓ 与基准一致（481 秒）
```

（复验产物落在 `~/.hermes/cache/scratch/verify_*`，属过程文件，可随时删。）

**用户问过的那个问题（重要，别再走回头路）**：「自动输密码会不会给别人入侵别人电脑的机会？」
答：不会（不联网、只对已经拿到的那份包试、不给登录/远程能力），但**双用性是真的**
（让「猜一个加密压缩包」变便宜了）。所以立了硬边界：不做字典/爆破/内置密码表、
候选数夹上限、抓到的码只试不存。这些边界写在 README「本工具不做的事」+
`config::候选数硬上限` + 三条测试里 —— **动这块之前先读那三处**。

### ⚠️ 用户定的节奏（发布相关，下手前先看这段）

- 用户明确说：**先把东西做到"完全完整能用"再发布**，现阶段只做本地 git
- 发布时：仓库名 `yshjy`、描述写"杨三海解压"（用户已确认，不用再问）
- **发布前必须先做安全自查**：git 历史里的密钥 / 代码里的个人路径 /
  `refs/` 里的第三方素材（`refs/SteganographierGUI/` 是别人的源码副本，
  公开前应改成链接引用而不是拷贝）
- 发布动作：建仓库 → 推 23+ 个提交 → 建 Release 传两个 tar.gz

### 本轮（v0.8）做完的

| 项 | 状态 | 说明 |
|---|---|---|
| 多语言 | ✅ | 7 种；**加语言不用改代码**（build.rs 扫 locales/ 嵌二进制）；用户可放外部语言包只覆盖几句 |
| 文案归属 | ✅ | 错误码 / 预览清单 / 参数报错 / 中断 / 完成短语 / 诊断 / 配置项 —— 全走语言包 |
| 磁盘空间预检 | ✅ | 解压前拦"空间不够"，留 5% 余量，`--force` 可跳过；拿不准就放行（不假拒绝） |
| 时间戳/权限还原 | ✅ | ZIP + TAR 已接；**剥掉 setuid**；7z / RAR 受库限制做不了（见下） |
| 配置 + 密码本 | ✅ | XDG 路径、逐字段宽容、密码本自动试（用户自己记的优先） |
| 统一解压接口 | ⬜ | 未做（各格式仍是平行函数） |
| fuzz 模糊测试 | ⬜ | 未做 |
| 诊断包 | ⬜ | 未做（BACKEND-SPEC §4 已设计） |

### 本轮抓到的真 bug（都是新功能顺带暴露的老问题）

1. **`--lang en 包.mp4` 把 `en` 当成要解压的文件** —— "带值选项"清单漏登记。
   已抽成常量 `带值的选项`，以后加选项不会再漏。
2. **打包从不写时间戳** —— 解出来全是 1980-01-01 08:00（ZIP 格式起点）。
   根因：所有条目共用一份 FileOptions，而一份只能装一个时间。
3. **`yshjy --list 包.mp4 | head -3` 会 panic** —— Rust 默认忽略 SIGPIPE，
   管道关闭变成写失败、`println!` 直接崩。已恢复 SIG_DFL（Unix），现在安静退出 141。
4. **"可执行文件提醒"传不到用户眼前** —— 警告塞在引擎内部列表里，外壳看不见，
   所以配置开关"看起来无效"。已改成走诊断事件（外壳本来就会渲染）。
5. **配置项"只读不用"** —— `warn_executables` 传进 Limits 了但没人读。
   已加 `safety::should_warn_executable` 真正接线。
6. **`--show-config` 中英混搭** —— 条目名和冒号都是中文。已改成"引擎给键名、外壳渲染"，
   连冒号都分全角/半角。
7. **`do_extract` 参数涨到 9 个**（clippy 拦下）→ 重构为参数结构体。
   一堆 `bool` 按顺序传错是**不会报错**的，这是真隐患。
8. **`zh` / `zh-Hans` 随机落到简体或繁体** —— 遍历 `HashMap` 的迭代顺序是随机的，
   同一条命令今天明天不一样。已改成常量默认表 + 20 次重复断言防复发。


### 版本演进

| 版本 | 内容 |
|---|---|
| v0.1.0 | 核心五环节：定位 → 解压 → 打包 → 修补 → CLI |
| v0.2.0 | 7z + RAR + 分卷自动拼接 |
| v0.3.0 | 救援式提取（中央目录全坏也能救）+ 自动抓解压码 |
| v0.4.0 | 格式补全：单流四件套（gz/xz/zst/bz2）+ TAR 家族 |
| v0.5.0 | 引擎地基：事件流 / 进度 / 取消 / 挑选 / 批量 + 编排搬进引擎（GUI 的地基） |
| v0.6.0 | 欠债清理：多格式预览（PreviewReady 接线）/ 智能解压三规则 / Ctrl-C 接管 / 序号统一 1-based |
| v0.7.0 | 常用版补全（Brotli / LZ4 / ZIP 分卷）+ 双版本骨架（常用版 / 全能版） |
| v0.8.0（未发布） | 多语言（7 种）/ 磁盘空间预检 / 时间戳与权限还原 / 配置 + 密码本 |
| v0.9.0（未发布） | 统一解压器接口 + 安全护栏 / fuzz / 诊断包 --report / 性能修复（zip 偏移）/ tar 列清单 / 救援事件 |

### 支持的格式矩阵

```
                解压   加密   分卷   怎么认出来的
ZIP / ZIP64      ✅    ✅     —      魔数
ZIP 分卷(.z01)   ✅    ✅     ✅     扩展名 + 收尾块（必须在 7z 分卷之前判）
7z               ✅    ✅     ✅     魔数
RAR              ✅    ✅     ✅     魔数 + 扩展名（1.3 → 7，只解不压）
TAR              ✅     —      —      偏移 257 的 ustar + 文件名（.tar.gz 这类看不出来）
GZIP             ✅     —      —      魔数
XZ / LZMA        ✅     —      —      魔数
ZSTD             ✅     —      —      魔数
BZIP2            ✅     —      —      魔数
LZ4              ✅     —      —      魔数（帧格式）
Brotli           ✅     —      —     ⚠ 没魔数，**只能按扩展名 .br**
```

加密全支持中文密码（ZipCrypto / AES-256 / 7z AES-256）。

---

## 怎么跑

```bash
# 装到系统
bash ~/hermes/yshjy/install.sh

# 用法
yshjy 游戏.mp4                  # 一句话解压（自动定位 + 自动找密码）
yshjy --list 电影.mp4           # 只看里面有什么
yshjy -p "解压码" 包.mp4        # 手动给密码
yshjy ys 我的游戏/              # 反向：打包成伪装视频
yshjy --rescue 坏掉的包.zip     # 索引全坏也按文件头逐个救
```

开发时的完整检查：
```bash
cd ~/hermes/yshjy
cargo test                     # 206 单元测试
cargo clippy --all-targets -- -D warnings
bash tests/smoke.sh            # 25 个冒烟场景
```

v0.5 新增的用法：
```bash
yshjy 下载/                     # 整个目录排队解（大的先跑，一个坏了不影响其余）
yshjy --select "*.exe" 包.mp4   # 只解一部分（序号 / 通配 / required）
yshjy --jobs 2 下载/            # 并行 2 个
yshjy --json 包.mp4             # 事件流按行输出（脚本/前端消费）
```

---

## 下一步（按用户"做到完全完整能用"的目标排）

```
0. 界面 → **已推进到第 19 批**（拖放/密码框/诊断包/批量队列/缺卷卡片/智能解压都上了）。
                     **下一步第 20 批**：P5 设置页能真改 + P4 密码管理（密码本打码 /
                     试到第几个 / 不保存本次密码）+ D1 大文件识别进度。
                     ⚠️ 动手前先重新编译 GUI（现有 bundle 是 19:30 的，不含 18/19 批改动）：
                     `cd gui && flutter build linux --release`
                     GUI 技术：Process.start + NDJSON（不是 FFI），见 gui/README.md
                     协议层 `--serve` 已完成 + 7 组冒烟全过（docs/PROTOCOL.md）
1. 打发布包           bash scripts/release.sh（一次出常用版 + 全能版）+ scripts/build-deb.sh
                     —— v0.8 / v0.9 的包都还没打，攒一起发更省事
2. fuzz 跑久一点（可选）60 秒只算"初步体检"：认真跑用 -max_total_time=600 以上，
                     corpus 会累积（越跑越深）。nightly + cargo-fuzz 都已就位
3. 零碎               剩下的是「同类问题还没扫干净」的几处：
                     `ExtractReport::summary()` 仍是引擎里拼的中文（救援/解压完成那行）
                     —— 与第 10 批删掉的 `PackReport::summary()` 是同一类问题；
                     其余格式（ZIP/7z/RAR）没有救援式列清单（tar 第 10 批已做）；
                     `do_report` 之外仍有少量外壳句子写死中文
```

---

## 已修复的性能问题（留档：诊断过程本身也值得记）

### 大伪装包慢（`--list` 109 秒、解压多花 ~70 秒）—— v0.9 第 6 批修好

- **最初的误判**：以为是**识别阶段**（`scan`）在"包藏在文件中间"时走了全文件扫描。
  实测打脸：`--json` 事件流显示 **Locate 只要 40 毫秒**，慢的 115 秒全在 List 阶段
- **真根因**：`zip` 库默认 `ArchiveOffset::Detect` —— 它会自己去"搜索中央目录的真实
  位置"来反推归档偏移；而我们的偏移**早就自己算好了**（`io::Window` 把 base 归零 →
  对库来说就是 0）。HANDOFF 里那条「库自己扫 8.7 GB 要 113 秒，喂偏移只要 69 微秒」
  的实测，**没有被贯彻到所有调用点**
- **修法**：`layout::zip::打开已知偏移()`（`Config { archive_offset: Known(0) }`），
  三处打开点全部改用它：preview（列清单）/ extract（解压）/ verify_password_at（试密码）
- **效果**（同一个真实包，产物 SHA 逐字节一致）：
  · `--list` 8.7 GB：**109 秒 → 0.09 秒**（约 1200 倍）
  · `--list` 5.9 GB：22 秒 → 0.39 秒
  · 解压 5.9 GB：**129 秒 → 55.2 秒**（2.3 倍）
- **纪律**：打开 ZIP 一律走 `打开已知偏移()`，不要直接 `ZipArchive::new`
  ——「我们定位、库解码」这条分工漏一处就是百倍性能差

### 诊断方法论（下次遇到"慢"照这个来）

1. 先看**阶段耗时**（`--json` 的事件流里有 `StageFinished.millis`），别凭直觉猜是哪一步
2. 再看 `user` 与 `real` 的比例：`user` 远小于 `real` = 卡在 I/O（不是算法）
3. 然后去**依赖库的源码**里找那条"自动检测"的默认分支（本地就有
   `~/.cargo/registry/src/*/zip-9.0.4/`），比反复跑实验快得多

### 已完成（v0.6 / v0.7 两轮，供参考）

```
✅ 预览接线       ZIP / 7z / RAR / 单流都能「不解压就列内容」+ PreviewReady 事件
✅ 智能解压三规则 单一顶层目录/单文件 → 直接解；散文件 → 建同名子目录
✅ Ctrl-C 接管    → 引擎取消令牌（已解出的保留、退出码 130）
✅ 序号统一       条目号一律 1-based（界面显示的号 = --select 用的号）
✅ 双版本         常用版 / 全能版（cargo features + release.sh）
✅ Brotli/LZ4/ZIP分卷  常用版格式补齐
```


## 关键文件

```
~/hermes/yshjy/
├── DEVLOG.md            ← 完整开发历程（踩的坑全在里面，值得读）
├── CHANGELOG.md         ← 每个版本改了什么
├── README.md            ← 给用户看的说明
├── DESIGN.md            ← 设计文档（542 行）
├── ARCHITECTURE.md      ← 架构（352 行）
├── BACKEND-SPEC.md      ← 后端规格（411 行）
├── docs/CLI-SPEC.md     ← 命令行规格（196 行）
├── docs/UI-SPEC.md      ← 界面规格（586 行，含 6 张线框图）
├── src/                 ← 9011 行，23 个文件
├── tests/               ← 48 个测试样本 + 冒烟脚本
├── refs/tech/           ← 681 条技术点台账（从 6 个开源项目拆出来的）
├── refs/                ← 参考源码库 355MB（7zip/PeaZip/XADMaster/zip2/zip2/...）
└── dist/                ← 发布包
```

---

## 血的教训（务必记住）

### 1. 删文件之前必须确认清楚

用户明确说过两次：
- **"刚刚让你删过程文件你把我解压好的和源文件都删了"**
- 之前的对话里也发生过同样的事

**规则**：
- **"过程文件"只指测试/临时产物**（如 `~/.hermes/cache/scratch/` 里的东西）
- **源文件绝不删**（用户下载的原始文件，如 `~/下载/` 里的）
- **解压好的成品绝不删**（用户花时间解出来的成果）
- 删之前**列清单给用户看**，确认后再动手
- 批量删除、递归删除必须单独问一次

### 2. 同一个坑别踩两次

**压缩比防护的分母随手给了个 1** —— 这个错误犯了两次：
```
第一次（7z）   → 20MB 正常文件被判"压缩比 5025 倍"
第二次（单流） → 1750 字节文件被判炸弹，解出来 0 字节
```
统一修法：**分母一律从文件元数据取真实压缩大小**。

### 3. 输出说明要短

用户说过：**"说明太长了注意力不集中"**。
- 汇报要精炼，重点加粗，别堆大段文字
- 长内容写进文件，不要全塞在对话里

### 4. cargo 输出别写进 TMPDIR 根（测试会因此莫名变红）

- `password_hint` 找「说明文件」时会连包所在目录的**上一级**一起扫，而测试习惯把包造在
  `$TMPDIR/<名字>/` 里 —— 于是一份含 `251 passed` 的 cargo 日志被当成说明文件、把 `pass`
  抓成密码候选，测试就红了（本次真踩到，还顺带挖出「短标签没做词边界」这个真 bug）
- 规矩：日志一律写 `$TMPDIR/logs/`；造测试样本放自己造的深层目录，**别让测试结果依赖 TMPDIR 里有什么**

---

## 真实文件的关键数值（回归基准）

用户那两个伪装包（都在 `~/下载/baiduwangpan/示例包/`）：

```
lvIesSotSDRhnZfL_1.mp4.zip    8,711,817,691 字节（完整）
   ZIP 真实起点      119,248,031
   中央目录偏移      8,711,802,959
   条目数            1（ZIP64）
   解出的 示例包.7z.001  8,589,934,592 字节
   SHA-256           e6269244dd96fb3740f359c6a9aa7b12465d365ffa6b4cb009899d22316962dd
   示例包.7z.002 起点  378,881,716

PxoPiyCQKv9gsqFX_1.mp4.zip    5,931,359,517 字节
   示例包.7z.002 SHA-256  7dda988370e9403eb07eb00c0a0949477b1e8c0df5d3f0026aecd2491203fb59

zig4pQCY                      3,387,179,008 字节 ← 只有 39.4%，下载不全
   声明的压缩大小     8,592,554,867
   缺口              5,205,375,920
   （这个包不用管，用户已有完整版）
```

**已验证**：我们的工具解 8.7GB 那个包，SHA-256 与会话早前的手工 Python 脚本**逐字节一致**。

**v0.5.0 复验（2026-10-17，两个真实包各解一次）**：

```
示例包.7z.001   8,589,934,592 字节   SHA-256 = e6269244dd96...62dd   ✓ 与基准一致
示例包.7z.002                        SHA-256 = 7dda988370e9...3b59   ✓ 与基准一致
```
解压耗时：8.7 GB 包 382.9 秒 / 5.9 GB 包 201.1 秒（i5-8250U + 机械盘，约 47 MB/s）。

**试密码修复的效果（实测对比）**：

```
修前（每个候选把整条 8.6 GB 条目解一遍）   8.7 GB 包 382.9 秒
修后（只读第一个加密条目的开头 4 KiB）     8.7 GB 包 309.4 秒   ← 省 74 秒 / -19%
事件流：Locate 阶段 72 毫秒（尾部窗口定位，没走全文件扫描）
```

---

## 技术选择（为什么这么做）

1. **自己定位、库来解压** —— 我们算出 ZIP 的基准偏移，喂给成熟的解码库。
   实测：库自己扫 8.7GB 要 113 秒，喂偏移只要 69 微秒（快约 1600 倍）。
2. **不盲抄参考项目** —— 拆了 681 条技术点，逐条实测决定取舍。
   例：memchr SIMD 实测比标量**慢 15 倍**，已回滚并写进注释。
3. **坏包多数是「数据完好、索引写错」** —— 修索引（只改内存）比重新实现解压划算。
4. **RAR 必须主动验密码** —— 底层库解密失败会**静默不写数据**，不主动
   `archive.test(pw)` 会产出 0 字节的"假成功"。
5. **引擎不出中文文案** —— 只出错误码 + 参数，渲染归外壳（为多语言/多外壳准备）。
6. **`#![forbid(unsafe_code)]`** —— 全程零 unsafe。

---

## 用户偏好（做事方式）

- 不要"中途而废"，但也**不要急**，先讨论清楚
- 参考项目要真的读源码、逐条拆解，不是泛泛而谈
- **能结合的都要结合上**（优中选优）
- 跨平台兼容性要好，路径用 `PathBuf`，不用平台特有 API
- 密码永远打码，日志不打印明文
- 报告要实在，虚构数据比报错更糟
