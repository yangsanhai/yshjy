# Windows 构建与出包

> 这份是 **Windows 这台机器** 的操作手册。Linux 那边看 `AGENTS.md`。
> 全部内容 2026-10-30 在本机实跑过，不是抄来的。

## 一、这台机器上已经装好什么

| 东西 | 位置 | 版本 | 干什么用 |
|---|---|---|---|
| Rust | `C:\Users\18484\.cargo` | 1.95.0，目标 `x86_64-pc-windows-msvc` | 编引擎 |
| MSVC 编译器 | VS 2026 Community（`Program Files\Microsoft Visual Studio\18`） | MSVC 14.50.35717 | 链接 |
| Windows SDK | `Program Files (x86)\Windows Kits\10` | 10.0.26100 | 同上 |
| **Inno Setup** | `%LOCALAPPDATA%\Programs\Inno Setup 6`（**按用户装，不需要管理员**） | 6.7.3 | 出安装包 |
| **Flutter** | `D:\flutter` | 3.47.8 stable / Dart 3.13.7 | 编界面 |

相关环境变量（已设成用户变量，重开终端生效）：

- `INNO_SETUP_PATH` = `C:\Users\18484\AppData\Local\Programs\Inno Setup 6` —— fastforge 找不到 Inno 时先看它
- `PUB_CACHE` = `D:\pub-cache` —— **故意挪出 C 盘**，Flutter 依赖默认会写到 `C:\Users\…\AppData\Local\Pub\Cache`
- 用户 `Path` 里加了 `D:\flutter\bin`

> **C 盘只剩约 10 G，装东西一律往 D 盘**（用户 2026-10-30 明确要求）。

## 二、编引擎（命令行版）

```bash
cd /e/yshjy-20261030
CARGO_HTTP_PROXY= cargo build --release
# 产物：target/release/yshjy.exe（约 7.8 MB）
```

**`CARGO_HTTP_PROXY=` 不能省**：注册表里留着 FlClash 的 `ProxyServer=127.0.0.1:7890`，
cargo 会照读；FlClash 没开时就报「download of config.json failed」。
**置空后走直连，实测能通**（FlClash 开着也能通，两种状态都不用改注册表）。

跑测试：

```bash
CARGO_HTTP_PROXY= cargo test
```

预期：**341 过 / 14 挂**（2026-10-30 实测）。挂的 14 条全部有账，见
`docs/TODO.md` 的 W2 / W6 / W8 —— **不是新问题**，别当回归看。

## 三、编界面

```bash
cd /e/yshjy-20261030/gui
flutter build windows --release
# 产物：build/windows/x64/runner/Release/yshjy_gui.exe（连同 data/ 与几个 dll）
```

引擎要**跟界面放同一个目录** —— 界面按「自己旁边的 `yshjy.exe`」找引擎
（`gui/lib/engine.dart:256-263`）。**Windows 上要找的是 `yshjy.exe`，不是 `yshjy`**（W9 待改）。

## 四、出两个版本（用户 2026-10-30 拍板：两个都要）

| 版本 | 形态 | 用户怎么用 |
|---|---|---|
| **免安装绿色版** | 一个 zip | 解压到任意文件夹，双击 exe 就能用，不写注册表 |
| **安装包** | `-setup.exe`（Inno Setup） | 双击装，自动放开始菜单、挂右键菜单/文件关联 |

一条命令同时出两个（fastforge，参考 `refs/upstream/fastforge/docs/en/packagers/windows.md`）：

```bash
fastforge package --targets exe,zip
# 产物在 dist/<版本>/：<名字>-setup.exe + <名字>.zip
```

- **引擎 exe 塞进安装目录**：放到 `.fastforge/packaging/windows/shared/files/yshjy.exe`（overlay 目录）
- 右键菜单**绿色版也能有**：参考 `refs/upstream/hatchzip/packaging/windows/hatchzip.iss` ——
  它的做法是「**由程序自己在设置页里加右键菜单**」，这样绿色版和安装版行为一致
- **不签名**：用户首次双击会看到 SmartScreen 的「仍要运行」，README 里写清怎么点

## 五、Windows 专属的坑（踩过，别再踩）

1. **`append` 打开的文件不能 `set_len`** —— 返回 `os error 5`（拒绝访问）。续传记录就是这么失效的（W2）。
2. **`#![forbid(unsafe_code)]` 管到 `trash.rs`，管不到 `crash.rs`** ——
   `lib.rs` 里禁 unsafe，所以 COM 那类活儿（回收站）只能引 crate；`crash.rs` 是 bin 的模块，能写 unsafe。
3. **测试套件依赖 `zip` / `rar` / `unrar`**，本机 Git Bash 缺这三个（有 `7z` / `unzip` / `file` / `python3`）——
   `bash tests/all.sh` 跑不全，见 W7。
4. **`target/` 按平台生成**，Linux 那份 14 G 在 Windows 上一个都用不上；别当资产备份。

## 六、还没做的（别以为已经有了）

见 `docs/TODO.md` 末尾「2026-10-30 Windows 实测审计」W1~W10。
最便宜的三件：**W1 时区**、**W2 续传**、**W4 密码回显**（换成 `dialoguer::Password`，还顺手删一段手写 termios）。
