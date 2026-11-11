# yshjy 交接文档（重开对话先读这个）

> **2026-11-11 · 移动硬盘同步完成 —— 盘上 6 个安卓提交已并入本机，Linux 侧只剩「重出包 + 重装」**
>
> 本机 HEAD 现在是「盘上 `886f913` + 一笔 `fix(signature)` 死代码修复」。盘上工作区当时零改动，两边没有文件级覆盖。
>
> **盘上带回来的产物（5 份 sha256 逐个校验通过）**：
> `dist-win/`（setup.exe 15.1M、绿色版 zip 19.6M、CLI zip 7.3M）、
> `dist-android/`（arm64 23.1M、armv7 14.6M）；已拷回本机同名目录，`.gitignore` 已挡，不入库。
>
> **当前唯一欠账 = Linux 1.0.0 重出包 + 重装**：
> - 装机的 deb 是 11-08 22:59 那份，**不含** `fix(signature)`，也**不含**批 0 的 `gui/lib` 目录重构 → 界面必须重编
> - 顺序：把 `dist/` 里 1.0.0 旧产物挪走（`release.sh` 拒绝覆盖同版本）→ `flutter build linux --release`
>   → `release.sh` → `build-deb.sh` → `test-packages.sh`（期望 37/37）→ `tests/all.sh`
>   （2026-11-11 实测：**62 通过 / 1 失败 / 5 跳过**，唯一的红是安卓批带来的 `mobile_ui_test.dart` 两条，见本卡末「同步验收」）
>   → `sudo apt install ./dist/yshjy_1.0.0_amd64.deb` + 六项验收
>
> **Windows 侧**：1.0.0 三件套已出、老板已装机验收通过。但盘上 `dist-win/` 是 21:42 编的，
> 批 0 目录重构（22:06）在它之后 —— 那份界面对应重构前的代码（行为等价，只是目录结构不同）。
> 要不要用当前源码把 Windows 包重编一遍，等老板拍板。
>
> **安卓侧**：可装 APK + 真机（Redmi K70）全链路验收通过；剩两笔都不影响用：
> 长按「打开方式」进 App 的 intent-filter、Android 10 以下存储分支未真机验。
>
> **GitHub 上架前**：脱敏只剩 13 处 `/home/user`（8 个文件，`grep -ro` 实测）；
> `refs/analysis` 要单独确认不进库。
>
> **本机同步验收（2026-11-11 实测，接收侧重跑）**：
> - `cargo test`：448 过 / 1 忽略 + 33 + 2 doctests，rc=0 ✓
> - `tests/all.sh`：**62 通过 / 1 失败 / 5 跳过**（468 秒），唯一挂的是 `flutter test`
> - `flutter test` 里的两条红都不是产品 bug，是**测试自身**的两处问题：
>   ① `手机外壳：三个页签都在…` —— 断言找 `跟随系统`，而语言包实际值是 `跟随系统（推荐）`（`settings.theme.auto`），
>      精确匹配落空。盘上那张「437 过 / 0 红」是**批 0 重构时（22:06）**的数字，批 5 加了这条测试之后没再重跑全量，属过期绿。
>   ② `金标：手机首页空态` —— `goldens/mobile_home_empty.png` 是 Windows 上录的 1179×2556 基准，
>      本机字体渲染差 9.26%（`gui/test/failures/` 里有 diff 图），属平台差异。
> - 修法待老板拍板：① 断言改成从语言包取文案（别再手写中文字面量）；② 金标按平台存双基准，或只在该平台跑。

> **2026-11-10 夜 · 📱 安卓版从零到可安装 APK（在 Windows 机上做，真机 Redmi K70 冠军版验收）**
>
> **完成度**：方案里 6 个批次全部落地，真机跑通「选包 → 预览 → 解压 → 导出」全链路。
>
> **已做完**：
> 1. **批 0 目录重构**：`gui/lib/` 拆成 `common/`(13 个共享) + `desktop/`(31 个桌面) + `mobile/`(新)；
>    import 全改 `package:` 绝对路径，44 个文件 `git mv` 保历史；**桌面零回归**（analyze 干净、test 437 过/0 红/3 跳过）。
> 2. **批 1 环境**：cmdline-tools + NDK 28.2.13676358 + rust target `aarch64-linux-android` + 环境变量。
>    **没装 cargo-ndk** —— 它只是帮忙设 CC/AR/linker 的封装，脚本里直接设这几个变量等价（省掉一次网络安装）。
> 3. **批 2 第一块 .so（止损点）**：`libyshjy.so` 6.01 MB、**9/9 个 ABI 符号**、**16 KB 页对齐**（`readelf` 核过），进 APK 真机打开。
> 4. **批 3 FFI 桥接**：`FfiTransport` 实现 `EngineTransport`（`job_submit` + 回调 + 另起 isolate 里 `job_wait`）；
>    **先在电脑上用同一份 C ABI 编 Windows dll 跑通测试**（hello + list 真包），再去真机 —— 这条路提前抓掉一个真 bug：
>    `Isolate.run` 闭包捕获了整个实例（含 Completer）→ `object is unsendable`，改成只传库名 + 地址。
> 5. **批 4 存储**：A 私有目录（默认，零权限）/ B 设置开关「解到系统下载」（MANAGE_EXTERNAL_STORAGE，跳系统页申请）/
>    C 完成页「导出到系统下载」（MediaStore 一条条插，零权限）。**三条都在真机上验过**。
> 6. **批 5 手机界面 + 前台服务**：底部三页签（解压/历史/设置）、六态流程（空/识别/预览/解压中/完成/出错）、
>    7 语新文案；解压期间挂 `dataSync` 前台服务（logcat 核实 `Background started FGS: Allowed`）。
> 7. **批 6 出包**：自签 keystore（RSA2048/10000 天，**不入库**，备份在 `D:\Work\yshjy-win\签名备份`）、
>    `--split-per-abi` → arm64 23.1 MB / armv7 14.6 MB + `SHA256SUMS.txt`；应用图标也是这次画的（青色拆箱箭头）。
>
> **真机证据**（Redmi K70 冠军版 `2407FRK8EC`，Android 16 / arm64，包名 `com.yangsanhai.yshjy`）：
> - 冷启动 ~380–510 ms；预览显示「ZIP · 2 个文件 · 8.0 MB」并由引擎真读出两个 4 MB 样本；
> - 解压落盘：`/sdcard/Download/yshjy/test/打包输入/样本{0,1}.bin` 各 4,194,304 字节，中文目录名与 mtime 都对；
> - 私有目录模式与「导出到下载」模式都验过；截图在 `D:\Work\yshjy-win\截图\安卓-*.png`。
>
> **卡在哪·为什么停下**：没有阻塞，功能齐了。留给以后的两件（都不影响用）：
> ① 从文件管理器长按「打开方式」进 App（`VIEW`/`SEND` intent-filter）还没加；
> ② 极老 ROM（Android 10 以下）的存储分支只写了没真机验。
>
> **下一步第一步**：老板装上 `D:\Work\yshjy-win\安卓\yshjy-1.0.0-arm64.apk` 用几天，提意见再改。
>
> **关键文件**：`gui/lib/mobile/`（手机界面）、`gui/lib/common/transport/ffi_transport.dart`（桥接）、
> `gui/android/app/src/main/kotlin/com/yangsanhai/yshjy/{MainActivity,ExtractService}.kt`、
> `scripts/build-android.ps1`（一条命令出包）、`docs/ANDROID-整体方案-2026-11-10.md`、`docs/ANDROID-使用说明.md`。
>
> **版本锚**：`Synced through 8421607`（批2 提交；安卓相关后续提交见 git log）。**自评**：90 / 100
> （6 批全落 + 真机全链路；扣分点是 intent-filter 没做、老 ROM 分支没真机验）。


> **2026-11-10 · Windows 1.0.0 三件套出包 · ✅ 老板已装、已看过，验收通过**
>
> **完成度**：Windows 交付链 100%（引擎常用版+全能版 / Flutter 界面 / 绿色版 zip / 命令行 zip / setup.exe / SHA256SUMS）。
>
> **已做完**：
> 1. 从移动硬盘 `E:\yshjy-1.0.0-20261108`（Linux 侧 1.0.0 快照）原样重编，**源码一行没改**——
>    `pwsh -File scripts\build-win.ps1 -TargetDir <本机盘>` 一条命令全量复现（引擎 44s + 23s，界面 89s）。
> 2. **修掉一处出包坑（改的是脚本，不是源码）**：仓库在移动硬盘（exFAT）上时，Inno 最后一步
>    「更新 Setup.exe 资源」报 `Resource update error: EndUpdateResource failed (110)` 并中止（重试同样失败）。
>    已给 `build-win.ps1` 加兜底：直接输出失败时先编到本机临时目录，成功再拷回 `dist-win`。此后一条命令走到底。
> 3. **补回这份 `.git` 丢的 `refs/`**：`refs/` 没随盘拷过来（旧快照里 1.85 GB），git 报「not a git repository」；
>    已按 `.git\logs\HEAD` 最后一个 sha `8b6cc20` 重建 `refs\heads\main`，`git log / status` 恢复正常。
>    工作区里 32 个文件显示改动，但 **0 增 0 删**——是 exFAT 丢了可执行位的模式位差异，内容没漂移。
> 4. **冒烟验过**：`yshjy --version` → `yshjy 1.0.0（常用版）—— 杨三海解压` + 署名行
>    `yshjy · MIT License · Copyright (c) 2026 yangsanhai`；exe 属性 `杨三海解压 / 杨三海 / 1.0.0 / MIT`；
>    界面能开（1100×760 居中、中文常规字重）；拿 `E:\yshjy-探针-打包-230401.zip` 真解，2 个 4 MB 文件全出。
>
> **验收**：2026-11-10 晚，老板本人装好并看过界面，回复「安装和看了一下可以了」——Windows 1.0.0 **验收通过**。
> 还剩一件可做可不做的：**干净 Windows 机器复验**（不装 VC++ 运行库也能开）——本包已把三个运行库 dll 随包，未在裸机实测。
>
> **卡在哪·为什么停下**：没有阻塞，这一版收工，项目转存档。
>
> **下次开工第一步**：①要改功能 → 先在 Linux 侧改，再拷新快照到移动硬盘重跑 `pwsh -File scripts\build-win.ps1`；
> ②只重打 Windows 包（源码没动）→ 加 `-SkipBuild`；③产物目录 `D:\Work\yshjy-win` 只放交付件，不存源码。
>
> **关键文件**：`dist-win\yshjy-1.0.0-setup.exe`(15.1M) / `yshjy-1.0.0-win-x64.zip`(18.7M，绿色版) /
> `yshjy-1.0.0-win-x64-cli.zip`(6.9M) / `SHA256SUMS.txt`；交付副本在 `D:\Work\yshjy-win`；
> 界面截图 `D:\Work\yshjy-win\截图\1.0.0-界面-*.png`。
>
> **证据**：`cargo build --release` 与 `--release --features full` 各一次（exit 0）；`flutter build windows --release`
> 89.2s 出 `yshjy_gui.exe`；7z 打两个 zip；ISCC 6.7.3 编译安装包成功；GUI 冒烟启动 + 截图 + 真解 2 文件。
> **没跑**：完整 `cargo test` / `flutter test`——源码与 Linux 侧 1.0.0 逐字节相同（只多 SPDX 头），测试已在 Linux 侧全绿。
>
> **版本锚**：`Synced through 8b6cc20`（release: 1.0.0）
>
> **自评**：92 / 100（老板已装机验收；扣分点是没在本机跑完整 Rust/Flutter 测试套件，也没在裸机试过）。


> **2026-11-08 · 0.9.58 已出包并装机，用户验收通过 —— 当前没有欠账**
>
> **这一版装的是 0.9.58**（`dpkg -l yshjy` = 0.9.58，`yshjy` / `yshys` 两条命令都是 0.9.58）。
> **用户可感知的变化**：打包页能拖文件了（★本轮重点，用户亲手验过「可以拖文件进去打包了」）、
> ZIP 解出的时间不再 +8 小时、`--split` 的 `.001/.002` 裸分卷能打开、含 `* ? < > " |` 的条目被拒、
> 启动不再卡「正在识别…」。
>
> **发版链（全跑过）**：`flutter build linux --release` → `bump-version.py 0.9.58`（11 处/8 文件）
> → `release.sh`（两档 tar.gz 各 4.2M）→ `build-deb.sh`（`dist/yshjy_0.9.58_amd64.deb` 11M）
> → `test-packages.sh` 37/37 → `sudo apt install` → 六项验收。
>
> **门禁**：`cargo test` 448 过/1 忽略 + 33 + 2 doctests、`clippy -D warnings` 干净、
> `flutter test` 440 过/0 红、`tests/all.sh` 62 过/0 失败/5 跳过。详见 `CHANGELOG.md` 0.9.58 段。
> AppStream 也补了 0.9.58 的 `<release>`。
>
> **剩下的只有 Windows 侧**：三个包（setup.exe / 绿色版 / CLI）sha256 全过、在 `dist-win/`，
> 等老板在一台干净 Windows 上肉眼验收。Linux 侧这一轮没有欠账。

> **2026-11-08 · Windows 那轮改动已同步回 Linux 并全绿（已提交，未发版）**
>
> 从移动硬盘 `yshjy-20261106` 搬回 50 个文件（10 个 Windows 机器相关生成物刻意不同步），
> 见 `docs/LINUX-同步说明-2026-11-07.md` 与 `CHANGELOG.md` 2026-11-08 段。
> **本机实测**：`cargo test` 448 过/1 忽略 + 33 + 2 doctests、`clippy -D warnings` 干净、
> `flutter analyze` 无问题、`flutter test` **440 过/0 红**、`tests/all.sh` **62 过/0 失败/5 跳过**。
> 重点两条也真验了：ZIP mtime 与 7z 逐秒相同（不再 +8 小时）；`--split` 的 `.001` 裸切分
> 能 `--list` 也能真解（cmp 字节一致）。Windows 三个包 `sha256sum -c` 全过，存在 `dist-win/`。
> **两张卡（0.9.56/0.9.57）曾从归档里漏掉，已补回 `HANDOFF-archive.md` 顶部。**
> 下一步只剩：老板肉眼验收 Windows 界面 + 干净 Windows 机器复验。

> **2026-11-07 · Windows 版 0.9.57 重新出包（「文字发虚」已修）· 待老板验收**
>
> **完成度**：Windows 交付链 100%（引擎 + 界面 + 两个 zip + 安装包 + 校验清单）。
> **Linux 专属分支一行没动**（`/proc`、XDG、unix symlink、权限位、落盘检验）；
> 共享逻辑只修了 ZIP 时间换算——那处在 Linux 上同样错 8 小时，属于两边一起修对。
> 另有共享的安全校验补了 Windows 非法字符（Linux 上同样拒，与既有的保留名/冒号策略一致）。
>
> **已做完**：
> 1. **界面发虚的根因查实并修掉** —— 不是透明度，是 Flutter 在 Windows 上把中文回退到
>    `Microsoft YaHei Light`；`main.dart` 钉成 `Microsoft YaHei UI` 常规字重，小字笔画变实。
> 2. **Windows 适配**：标题「杨三海解压」、初始窗口按工作区 92% 居中（最多 1100×760）、
>    最小 760×560、深色主题时标题栏同步变深。
> 3. **出包脚本**：`pwsh -File scripts\build-win.ps1` 一条命令全量复现；
>    VC++ 三个运行库 dll 随包（干净机器不用先装运行库）；`SHA256SUMS.txt` 已生成。
> 4. **中文安装包**：Inno Setup 6 + `scripts/ChineseSimplified.isl`；按用户装不弹 UAC；
>    22 个扩展名 + 文件夹的右键菜单；装完可立即启动。设置页「通知」那句按平台分叉，
>    Windows 不再显示「需要系统有 notify-send」（7 语言新增 `settings.notify.hintWindows`，574→575 键）。
> 5. **按「Windows 分发包待修清单」自查时又揪出 4 个真问题，都已修**：
>    ① **启动竞态**（Linux 同样有）：`hello` 和 `list` 同时写引擎 stdin，`flush()` 把 sink
>    绑住，另一条写抛 `StreamSink is bound to a stream` → 界面永远停在「正在识别…」
>    （Win 上约一半概率复现）。已把 `ProcessTransport.writeLine` 串行化 + 首页兜底 catch。
>    ② exe 文件属性还是模板默认的 `com.example` / `yshjy_gui` → 改成「杨三海 / 杨三海解压 / MIT」。
>    ③ Windows 上 `cargo test` 编不过（`flush.rs` 测试用了 unix symlink）→ Linux 专属测试加 cfg。
>    ④ `--no-paths` 在 Windows 失效（只看 `HOME`，Windows 用 USERPROFILE）+ 进度条字符判断 →
>    改用 `paths::home()`；Windows 进度条直接用方块。
>    **结果**：`cargo test` 439 过/1 ignored + 41 + 2 doctests、`clippy -D warnings` 干净、
>    `flutter test` **432 全过/0 红/3 跳过**、启动竞态连开 **10 次 10 次**都进预览。
> 6. **设置页两项按 Windows 实际情况收口**：①「并行任务数」下面那句磁盘类型以前永远
>    「没探到」—— `hello.disk` 只认 `/proc` + `/sys`；已加外壳探针（`DeviceIoControl` +
>    `StorageDeviceSeekPenaltyProperty`，与内存探针同一注册模式），本机实测 `solid`，
>    界面显示「固态硬盘 —— 可以试试 2 或 4」。②「落盘检验」在 Windows 上本来就不生效
>    （`is_removable` / 等待逻辑都是 Linux 专属），开关改为禁用 + 说清「拔 U 盘前先安全弹出」
>    （`settings.flush.hintWindows`，575→576 键）；Linux 不动。
> 7. **「全面支持 Windows」专项审计（允许改核心）** —— 又修 5 处，全部实测：
>    ① **ZIP 时间 +8 小时**（DOS 本地墙钟转 Unix 秒少减时区偏移；源 18:50 → 解出次日 02:50，
>    与 7z 也对不上）→ 修完往返一致、与 7z 逐秒相同；**Linux 同一处一起修好**。
>    ② **符号链接回退**：zip 路径把目标拼到解压根（嵌套链接必失败）、tar 路径直接跳过 →
>    两处都改成按 POSIX 语义相对链接所在目录「复制目标内容」，并记 SlowPath。
>    ③ CLI 打印路径的 `\\?\` 前缀去掉（显示专用）。④ `--report --no-paths` 不再劝你加
>    `--no-paths`（7 语言新键）。⑤ 外部语言包目录改用 `paths::home()`。
>    **实测**：打包→解回字节+mtime 一致、中文密码、回收站、自解压 exe、诊断包脱敏、
>    4000 文件中断续跑、tar.gz、308 字符长路径、大小写同名、通知气泡、选择文件→预览。
> 8. **按参考项目补齐最后三笔**：① `--split` 的 `.001` 裸切分**现在能 `--list` 也能解**
>    （新增 `as_raw_slices` 按序拼成逻辑流；顺手给 `as_7z_volumes` 补魔数校验，
>    否则裸切分会被误判成 7z）——参考 `refs/tech/对比-同名技术择优.md` §8 的 unar 规则。
>    ② 包内非法文件名字符 `* ? < > " |` 入库前就拒（`windows_invalid_char`），
>    参考 `refs/tech/对比-安全与健壮性.md` §A3 的 7zip/PeaZip/XAD 三家一致做法。
>    ③ RAR4 的 DOS 时间查过 `rars/timestamp.rs`：它本来就按**本地时区**换算，
>    没有 +8 小时问题（`rar_mtime_sample` 回归照过）。
> 9. **拖放补齐**：首页拖放原来就有（单文件→预览、多文件→队列）；**打包页原来不能拖**，
>    这次补上（整页 DropTarget → 输入列表，去重 + 自动推输出名），两页都用 `acceptDrop`
>    按前台页开关（IndexedStack 两页都铺满窗口，否则在打包页拖文件会被首页抢走）；
>    顺带修输入列表标题只按 `/` 取文件名（Windows `\` 路径会显示整条路径）。
>    新增 **5 条**拖放 widget 测试（真实 MethodChannel 事件）。`flutter test` 432 → **437**。
>
> **给 Linux 侧**：这一轮有 7 处**共享代码**改动（ZIP 时间、非法字符、`.001` 分卷、
> HOME→paths::home、7 语言新键、传输层竞态、打包页拖放），Linux 拉过去行为会变；
> 同步清单与验证命令见 `docs/LINUX-同步说明-2026-11-07.md`。
>
> **卡在哪**：不卡。**只剩老板本人验收**（他反复强调：界面必须他本人点、本人截图）。
>
> **下一步第一步**：老板双击 `E:\yshjy-20261106\dist-win\yshjy-0.9.57-setup.exe`
> （或解压 `yshjy-0.9.57-win-x64.zip` 双击 `yshjy_gui.exe`），看中文向导 + 新界面。
>
> **关键文件**：`scripts/build-win.ps1` · `scripts/yshjy-setup.iss` ·
> `docs/BUILD-Windows.md` · `gui/lib/main.dart`（字体）· `gui/windows/runner/main.cpp`（窗口）
>
> **证据**：四屏截图（浅色+深色，`D:\Work\.tmp\win-0957-*.png`）；GUI 实解 5 个文件全出；
> 静默装→启动→卸载全链路干净；连开 10 次都进预览（竞态修复）；`flutter analyze` 干净；
> `flutter test` 432 全过；`cargo test` 439+38+2 全过、`clippy` 干净。
>
> **版本锚**：`Synced through c60506d`（工作区另有 Linux 0.9.56/0.9.57 未提交改动 + 本次 Windows 改动，
> 按硬规矩 3 未 commit）
>
> **自评**：90/100（界面观感与干净机器复验只能老板给）

