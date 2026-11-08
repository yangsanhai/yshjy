# yshjy 交接文档（重开对话先读这个）

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

> **2026-11-05 夜 · 0.9.57「自己的应用图标 + 窗口图标」—— 已装机，用户确认「就这样」**
>
> **这一批（0.9.56 + 0.9.57）做了什么**：
> 1. **主题切换立刻生效** —— 真 bug：`onChanged` 只改页面局部 `_theme`，`themeMode.value` 只在保存时写；
>    修在 `settings_page.dart`，回归 `gui/test/settings_theme_test.dart` 3 条。
> 2. **格式表 / 快捷键表搬到关于页**（`pages/reference_tables.dart`，铺开不折叠）；设置页 907 → 714 行。
> 3. **保存按钮改右下角常驻圆形 FAB**（`ValueKey('settings-save')`，tooltip「保存」兼无障碍名；
>    刻意不用 CircularProgressIndicator —— 那个无限动画会让 pumpAndSettle 卡到超时，本项目踩过）。
> 4. **开源许可**：`scripts/gen-third-party.py` 由 `cargo metadata` 生成 190 个依赖到
>    `gui/assets/third_party.json`；关于页两个入口（清单页 + Flutter 自带许可证页）；
>    仓库地址只做**可选中文本、不联网**（守 `tests/offline_probe.py` 那条承诺）；
>    7 语言各 +13 键（561 → **574**）；`tests/all.sh` 加了 `gen-third-party.py --check` 防清单过期。
> 5. **应用图标**（用户要求「压缩软件是看书脊不是看封面」）：一摞书脊朝前的书 + 最上层播放三角，
>    7 档铺进 `gui/linux/icons/hicolor/`，Windows 的 `app_icon.ico` 同步；
>    补 `gtk_window_set_icon_name(window, "yshjy")` —— 此前任务栏 / Alt-Tab 一直是系统默认齿轮。
>
> **验证**：界面测试 **435 条全过**、`flutter analyze` 干净；引擎 446 过/1 忽略；
> `scripts/test-packages.sh` **37/37**；装机后 dpkg 0.9.57、CLI `--version` 0.9.57、
> 装机图标 md5 与项目源一致、`nm -D yshjy_gui` 里确实有 `gtk_window_set_icon_name`。
>
> **用户验收**：0.9.57 装机后看过图标与设置页，原话「看到了就这样」。
>
> **还没做**：① 爱发电赞赏链接（等实名注册，位置已留）② 推 GitHub（T9 截图；`/home/user`
> 路径泄漏要先脱敏）③ `docs/TODO.md` 里「Windows CLI 缺分发包」等 Windows 侧三条修完一起改
> ④ Linux 仓库仍缺硬盘提交 `74512bf`（要提交得先拉回来，否则历史分叉）。

> **2026-11-04 深夜（前一版） · ✅ 0.9.54 已装机 + 全部验收通过 —— 当前没有欠账**
>
> **这一版装的是 0.9.54**（`dpkg -l yshjy` = 0.9.54），桌面菜单启动的也是它（右下角「引擎 0.9.54」）。
>
> **验收结果（全部真跑，详见 `docs/RELEASE-CHECK.md`）**：
> - `cargo test` **446 通过 / 1 忽略**、`tests/all.sh` **61 通过 / 0 失败 / 5 跳过**
> - `test-packages.sh` **37 通过 / 0 失败**
> - 装机六项：dpkg 版本 ✔ / 两个命令名 ✔ / 菜单 14 种格式 ✔ / `pick.sh` 11-0 ✔ /
>   真打包真解压 sha256 逐一致（中文名+三层嵌套+500KB 随机二进制）✔ / 界面显示 0.9.54 ✔
>
> **这一版修的东西**（两笔，都在 0.9.54）：
> 1. 真机撞出来的「解压失败在目标目录留空文件夹」—— zip 家族 + 截断的 `.tar.gz` 两条路都修了，
>    改用 `空目录守卫`（`Drop` 兜所有返回路径），只删空目录。
>    **★ 教训**：第一版把清理写在函数末尾是**假绿**（`open_tar_stream` 的 `?` 会绕过），
>    测试只查「目录里面空不空」也漏了「目录该不该在」；靠反向验证才发现，已写进 CHANGELOG。
> 2. 顺带确认：CLI 不带 `-p` 时的**交互式密码输入**是好的（pty 输对就解开、回车就放弃、管道不卡死）
>
> **下次开工**：没有阻塞项。想推 GitHub 再做 T9 截图收尾（`metainfo` 里那段还注释着）；
> 想加功能就从 `docs/TODO.md` 的「下一轮施工顺序」接。**别再复跑门禁当第一件事**。

> **2026-11-04 深夜 · ✅ 真机验收通过 + 0.9.53 打好了（本地，未对外发布、未装机）**
>
> **真机长跑（原「唯一欠的」）已销账** —— 用户亲自在界面上拖包验了三关，全过：
> 1. **分卷 ZIP**：`C12_split.zip` + `C12_split.z01` 一起拖 → 正常认出并解开（A-5 真机验收 ✅）
> 2. **坏包假尺寸**：`A6_declared_size_impossible.zip` → 预览页四处都写「尺寸异常」，**不再摆那个 1.17 GB**（A-6 真机验收 ✅）；
>    解压时按「包里有不安全的文件名（含控制字符之类）— 已拒绝写出来，这是安全保护，不是包坏了」拦下。
>    ★ 那个包的文件名列本来就是垃圾字节（真身 `00 23 00 00 … c3 b6 …`，NUL + 乱码），**显示成方块是对的**，
>    不是中文显示坏了 —— 已把这个认知记进 TODO「已裁定不做」（用户：不需要那么专业）。
> 3. **伪装视频 + 密码**：`A2_steg_modeA_ascii_pwd.mp4`（`P@ssw0rd!2026`）和 `A3_steg_modeA_cn_pwd.mp4`（`demo-password`）
>    都认出来、都能解开 ✅
>
> **这一轮做的**：
> - 门禁复跑 `bash tests/all.sh` → **61 通过 / 0 失败 / 5 跳过**（313 秒），基线没漂
> - 版本号 **0.9.52 → 0.9.53**（`bump-version.py`，11 处 / 8 个文件一致）；CHANGELOG 两段 `[未发布]` 已成版
> - 引擎重编（bundle 里那份引擎原比源码旧一笔，4d87708 的 RAR 候选收一份没跟上）→ 已重编
> - 产物：`dist/yshjy_0.9.53_amd64.deb`（11M）+ `yshjy-0.9.53-linux-x86_64.tar.gz` / `-full-`（各 4.3M）
> - `test-packages.sh` → **37 通过 / 0 失败**（第一次跑有 1 红：dist 里还躺着 0.9.52 的旧 tar.gz，跑 release.sh 补齐后转绿）
>
> **★ 装机仍是 0.9.52**：用户口径「先不发布，本地打包就行」。要装的话：
> `sudo apt install ./dist/yshjy_0.9.53_amd64.deb`，装完还有发版流程的**五项验收**（这一步没做）。
>
> **下次开工**：① 问用户要不要装机 + 五项验收 → ② 要不要推 GitHub（T9 截图依赖它，metainfo 里那段还注释着）。

> **2026-11-02 · 🎉 P8 审计全部收官（9 条 A 类 + 6 条 B 类注释 + 死代码清理）—— 仍未发版**
>
> 最后一轮做了三件事：
> 1. **B 类 6 条注释落在具体代码行上**（不是写在文档里）：`scan.rs::scan_whole_file`（垃圾前缀也认）、
>    `unpack.rs` tar 家族（剥壳后列真实文件，7z 只报 1 files）、`extract_rar.rs` 的候选密码与缺卷自查、
>    `单卷解压`（为什么不用 PBKDF2 passcheck）。
> 2. **★ 揪出 B-4 是假账**：账本引的 `rar5_cnpwd.rar` 库里不存在。用 `rar` 7.11 现造中文密码包实测 ——
>    我们能开，命中 **UTF-8**；GBK/UTF-16LE 对 RAR 永远命不中（RAR5 密码就是 UTF-8 文本，
>    `rars` 的 RAR4 更是强制 UTF-8）。已把 RAR 侧候选收成一份，账本按实测改。
>    RAR 中文密码现在是**机器验过的**：`tests/data/B4_rar5_cn_pwd.rar` + 两条回归测试。
> 3. **死代码清理**：`scripts/dead_pub.sh`（已挂 `all.sh`，现 0 个）清掉 14 个无人用的 `pub`，
>    含 `rescue_broken_index`(47 行)、`try_remux_with_ffmpeg`(35 行)。
>
> **验证**：`cargo test` **443 passed / 1 ignored**、`clippy -D warnings` 干净、
> `flutter test` 421、`bash tests/all.sh` 全绿。
>
> **★ 仍未发版**：装机还是 0.9.52，这些改动只在本地 git（A-5 / A-6 / B 类三笔）。
> **当前状态：⏸ 暂停（2026-11-02 凌晨，用户原话「先结束，更新相关的，有时间我在搞」）**
> —— 代码、文档、提交都已收尾，工作区干净，**没有任何半截活儿**。
>
> **下次开工第一件事**（按顺序，别跳）：
> 1. `cd ~/hermes/yshjy && bash tests/all.sh` 复跑一轮，确认基线还是 **61 通过 / 0 失败 / 5 跳过**
>    （`tests/data/` 不入库，但本机样本都在，不用重新生成）
> 2. **真机长跑验证**（用户说过「开发完了还没测试」，这是唯一欠的）：GUI 已重编在
>    `gui/build/linux/x64/release/bundle/`（引擎已拷进 bundle，是 A-6 后的新版），
>    装机版 `/usr/bin/yshjy` 仍是旧的 0.9.52 —— 要用新界面得从 bundle 启动
> 3. 真机验过之后再谈发版（发版流程见 `AGENTS.md` / 记忆：bump-version → release.sh →
>    build-deb.sh → test-packages.sh → sudo 装机 → 五项验收）

