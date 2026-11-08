# Windows 构建与出包

> 这份是 **Windows 这台机器** 的操作手册。Linux 那边看 `AGENTS.md`。
> 2026-11-07 按 0.9.57 实跑重写过；命令都能直接抄。

## 一、一条命令出全套

```powershell
# 在仓库根目录
pwsh -File scripts\build-win.ps1
```

它会依次做：编引擎（常用版 + 全能版）→ 编界面（Flutter Windows release）→ 摊目录 →
打两个 zip → 用 Inno Setup 打安装包 → 生成 `SHA256SUMS.txt`。

只重新打包、不重编（产物已在）：

```powershell
pwsh -File scripts\build-win.ps1 -SkipBuild
```

嫌仓库所在盘（移动硬盘）慢，可以把引擎 target 挪到别的盘：

```powershell
pwsh -File scripts\build-win.ps1 -TargetDir 'D:\yshjy-target'
```

产物全在 `dist-win\`：

| 文件 | 大小（0.9.57 实测） | 是什么 |
|---|---|---|
| `yshjy-<版本>-win-x64\` | — | 绿色版目录（双击 `yshjy_gui.exe` 就能用） |
| `yshjy-<版本>-win-x64-cli\` | — | 命令行版目录 |
| `yshjy-<版本>-win-x64.zip` | 18.7 MB | 绿色版压缩包 |
| `yshjy-<版本>-win-x64-cli.zip` | 6.9 MB | 命令行压缩包 |
| `yshjy-<版本>-setup.exe` | 15.0 MB | 安装包（中文向导） |
| `SHA256SUMS.txt` | — | 三个包的 sha256 |

## 二、这台机器上装了什么

| 东西 | 位置 | 用途 |
|---|---|---|
| Rust（MSVC） | `C:\Users\18484\.cargo`，目标 `x86_64-pc-windows-msvc` | 编引擎 |
| VS 2026 Community + Windows SDK | `Program Files\Microsoft Visual Studio\18` | 链接 |
| Flutter | `D:\flutter`（3.47.8 stable / Dart 3.13.7） | 编界面 |
| Inno Setup 6 | `%LOCALAPPDATA%\Programs\Inno Setup 6` | 打安装包 |
| 7z | `C:\Users\18484\scoop\shims\7z.exe` | 打 zip（没有就用 Compress-Archive） |

> `PUB_CACHE=D:\pub-cache`，Flutter 依赖不落在 C 盘。
> 新拉下来的仓库第一次要先在 `gui\` 里跑 `flutter pub get --offline`（`package_config.json`
> 里存的是 Linux 路径，直接编会找不到包）。

## 三、Windows 适配（和 Linux 不一样的四处）

1. **中文字体**：Flutter 默认字体是 Segoe UI，中文靠系统回退；本机实测回退会挑到
   **Microsoft YaHei Light**，11~12px 的小字笔画发细，看着像半透明。
   `gui/lib/main.dart` 里显式写了 `fontFamily: 'Segoe UI'` + `fontFamilyFallback:
   ['Microsoft YaHei UI', 'Microsoft YaHei']`（只在 Windows 生效）。**别再删掉这段。**
2. **窗口**：标题「杨三海解压」（不是 `yshjy_gui`）；初始尺寸取工作区 92%、最多 1100×760
   逻辑像素、居中；最小 760×560（`gui/windows/runner/main.cpp` + `win32_window.cpp`）。
   150% 缩放下如果还写死 1280×720，一开就铺满整块屏。
3. **深色主题**：标题栏是系统画的，界面切深色时它不会自己变。Dart 侧监听 `themeMode`
   调 `yshjy/tray` 的 `set_titlebar_dark`，原生 `DwmSetWindowAttribute` 刷成同色
   （`flutter_window.cpp`）。系统切深浅色也接上了（`didChangePlatformBrightness`）。
4. **VC++ 运行库**：绿色版和安装包里都塞了 `msvcp140.dll` / `vcruntime140.dll` /
   `vcruntime140_1.dll`（从 `C:\Windows\System32` 取，**别从 SysWOW64 取**）。
   干净 Windows 没装过 VS 也能直接跑。

## 四、手动分步（调试用）

```powershell
# 引擎
$env:CARGO_HTTP_PROXY=''; cargo build --release
$env:CARGO_HTTP_PROXY=''; cargo build --release --features full   # 全能版会覆盖同一个 exe，注意先存常用版
# 界面
cd gui; flutter build windows --release
# 安装包
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" "/DAppVersion=0.9.57" scripts\yshjy-setup.iss
```

引擎 exe 必须和界面放同一目录（界面按「自己旁边的 `yshjy.exe`」找引擎）。

## 五、坑（都踩过）

1. **`cargo` 报 `os error 5（拒绝访问）`、build-script「never executed」** —— 移动硬盘上
   偶发（杀毒软件正在扫刚编出来的 exe）。**原样重跑一次**；还不行用 `-TargetDir` 换到
   D 盘。
2. **`flutter build` 报 `LNK1104: 无法打开 yshjy_gui.exe`** —— 上一个界面进程还开着。
   先 `Stop-Process -Name yshjy_gui -Force` 再编。
3. **全能版覆盖常用版**：`cargo build --features full` 输出到同一个
   `target\release\yshjy.exe`。脚本里已经先把常用版挪到临时文件；手动分步时别忘了。
4. **NTFS 丢执行位**：拷回 Linux 前注意 `mode change 100755 => 100644`，别提交。
5. **测试里有一个 Windows 专属红**：`test/clipboard_path_test.dart` 的
   「file:// URI 转本地路径」断言写死了 POSIX `/home/u/...`，在 Windows 上
   `Uri.toFilePath()` 给出 `\home\u\...`。**不是功能坏了**，是测试没按平台写；
   2026-11-07 已改成按平台断言（现在 `flutter test` 432 全过）。
6. **★ 启动竞态（踩过，已修）**：外壳的 `hello` 和首页的 `list` 会同时写引擎 stdin；
   `IOSink.flush()` 内部是 `addStream`，并发 `writeln` 抛
   `Bad state: StreamSink is bound to a stream`，界面永远停在「正在识别…」。
   `ProcessTransport.writeLine` 已串行化 —— **别再把它改回直接 `writeln` + `flush`**。

## 六、验收记录（2026-11-07）

- 引擎：`yshjy.exe --version` → 0.9.57；GUI 状态栏显示「引擎 0.9.57」；
  `cargo test` **439 过 / 1 ignored + 38 + 2 doctests**，`clippy -D warnings` 干净。
- 界面：首页 / 预览页 / 解压完成页 / 设置页，浅色 + 深色都截图看过；中文字重正常；
  `flutter test` **432 全过 / 3 跳过**。
- 功能：拖（命令行传参）一个伪装包 `中文 文件名.mp4` → 认出 ZIP、5 个文件、402 KB →
  点「开始解压」→ 5 个文件全出（含中文目录、空文件、200 KB 二进制），封面也切出来了。
- 安装包：静默装到临时目录 → 文件齐（含三个运行库 dll）→ 右键菜单指向新路径 →
  桌面/开始菜单快捷方式在 → 启动正常 → 卸载后目录、注册表、快捷方式全无残留。
- **没验的**：干净机器（没有 VS / 运行库）复验、老板本人肉眼验收、繁体/英文系统语言。
