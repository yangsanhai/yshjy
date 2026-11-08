# Windows 分发包 · 待修清单（Linux 侧实测后写的）

> **谁看这份**：在 Windows 那台机器上干活的人（或 AI）。
> **为什么写**：Linux 侧用 wine 10.0 把 `dist-win/` 里的包真跑了一遍，引擎没问题，但**分发包本身有洞**。
> **版本号以 `Cargo.toml` 为准**，本文不写死版本（写这份时是 0.9.55）。
> **修完把结果记回 `docs/TODO.md`**（唯一真源），别在本文里打勾就算完。

---

## 0. 先说结论：引擎是好的，别动代码

Linux 侧实测（wine 10.0，跑的是 `dist-win/yshjy-0.9.55-win-x64-cli/`）：

| 验的什么 | 结果 |
|---|---|
| `yshjy.exe --version` | ✅ 版本正确、中文字正常 |
| `yshjy.exe --list "中文 文件名.mp4"` | ✅ 认出 zip、包藏在偏移 14050、"包在文件中间已按真实位置定位" |
| `yshjy.exe "中文 文件名.mp4" -o out` | ✅ 5 个文件全解出，大小逐个对得上 |
| 中文目录 / 中文文件名 / 「空格 文件名.bin」 | ✅ 全部正常 |
| 耗时 | 0.1 秒，退出码 0 |
| 两个 zip 包 | ✅ `unzip -t` 无错 |
| 架构 | ✅ `yshjy.exe` 64 位控制台、`yshjy_gui.exe` 64 位图形 |
| GUI 包结构 | ✅ flutter_windows.dll 21 MB、app.so 6.7 MB、icudtl.dat、16 个资源、两个插件 dll 都在 |

**所以：不要改任何 `.rs` / `.dart` / 构建脚本。** 这一轮只修「打包」和「补校验 + 验收」。

（`yshjy.exe` 与 `yshjy-full.exe` 大小相同但 md5 不同：这是正常的，
`Windows-使用说明.txt` 里已老实写了「全能版当前覆盖与常用版一致」，不用修。）

---

## 1. 🔴 必修：VC++ 运行库没打包，别人电脑会打不开

### 症状

别人（或一台干净 Windows）双击 `yshjy_gui.exe`，弹窗报：

```
无法启动此程序，因为计算机中丢失 MSVCP140.dll。尝试重新安装该程序以解决此问题。
```

引擎 `yshjy.exe` 也可能报 `VCRUNTIME140.dll` 丢失。

### 证据（Linux 侧 `objdump -p` 读出的导入表）

| 文件 | 依赖的运行库 |
|---|---|
| `yshjy_gui.exe` | `MSVCP140.dll`、`VCRUNTIME140.dll`、`VCRUNTIME140_1.dll` |
| `yshjy.exe` / `yshjy-full.exe` | `VCRUNTIME140.dll` |

而 `dist-win/yshjy-0.9.55-win-x64/` 里只有三个 dll：
`desktop_drop_plugin.dll`、`file_selector_windows_plugin.dll`、`flutter_windows.dll`。
**三个运行库 dll 一个都没带。**

> 为什么你自己电脑没事：装了 VS / 其他软件会顺手带上运行库。换台干净机器就露馅。

### 修法（二选一，A 优先）

**A. 直接把 dll 放进包里（推荐，绿色版也能直接跑）**

要拷的 3 个文件（**必须是 64 位版本**，从 `C:\Windows\System32\` 取）：

```
msvcp140.dll
vcruntime140.dll
vcruntime140_1.dll
```

放到这两个目录：

```
dist-win\yshjy-0.9.55-win-x64\        ← 图形版，三个都要（msvcp140 是 gui 用的）
dist-win\yshjy-0.9.55-win-x64-cli\    ← 命令行版，只要 vcruntime140.dll（放三个也无害）
```

⚠️ **别从 `SysWOW64` 取** —— 那是 32 位的，放进来会变成「不是有效的 Win32 应用程序」。

⚠️ **改完目录要重新打 zip**（`yshjy-*-win-x64.zip` / `-cli.zip`），否则 zip 里还是旧的。

⚠️ **Inno Setup 脚本要跟着看**：如果 `.iss` 的 `[Files]` 段是显式列文件名的，
要把这三个 dll 加进去；如果是 `*.dll` 通配就自动带上。**不改的话，安装版依然缺 dll。**

**B. 让安装器顺手装运行库**

在 `.iss` 里加检测 + 静默安装 `VC_redist.x64.exe`（微软官方 24 MB 左右）：

```ini
[Files]
Source: "VC_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Run]
Filename: "{tmp}\VC_redist.x64.exe"; Parameters: "/install /quiet /norestart"; \
  StatusMsg: "正在安装运行库…"; Check: NeedsVCRedist; Flags: waituntilterminated

[Code]
function NeedsVCRedist: Boolean;
begin
  Result := not FileExists(ExpandConstant('{sys}\msvcp140.dll'));
end;
```

这条只对安装版有效；**绿色版（zip）还是得走 A**。所以建议 A + B 一起上。

**C（不推荐）**：在 README 里写「请先安装 VC++ 运行库」。
普通人看到这句就走了，等于把问题推给用户。只有 A、B 都做不了才用。

---

## 2. 🟡 必修：补 sha256 校验清单

Linux 的 `dist/` 每个包都配了 `.sha256`，Windows 这边一个都没有。发 GitHub 前要补齐。

**生成方法（PowerShell，在 `dist-win\` 目录里跑）：**

```powershell
Get-ChildItem -File -Include *.exe,*.zip | ForEach-Object {
  $h = (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower()
  "$h  $($_.Name)"
} | Set-Content -Encoding ascii SHA256SUMS.txt
```

或者逐条用 `certutil`：

```cmd
certutil -hashfile yshjy-0.9.55-setup.exe SHA256
```

**要覆盖的文件**（打 zip 之后**重新算**，顺序别搞反）：

```
yshjy-*-setup.exe
yshjy-*-win-x64.zip
yshjy-*-win-x64-cli.zip
```

格式照 `sha256sum` 的：`<64位小写hash><两个空格><文件名>`，这样 Linux 和 Windows 两边都能核对。

---

## 3. 🟡 必须验（不是修，是补验收层）

**Linux 侧验不了 GUI** —— 本机没装虚拟显示，也不能在用户桌面上弹窗（项目规矩：截屏和点击一律本人做）。
所以这一层只能你在 Windows 上补：

- [ ] **绿色版**：解压 `yshjy-*-win-x64.zip` → 双击 `yshjy_gui.exe` → 窗口出得来
- [ ] 拖一个伪装包（`.mp4` 那种）进窗口 → 进度动 → 解出文件 → 打开解出来的文件看内容对不对
- [ ] **安装版**：跑 `setup.exe` 装一遍 → 从开始菜单/桌面快捷方式启动 → 同样拖一个包解一次
- [ ] **干净环境验证**（关键）：如果手上有另一台没装过 VS/运行库的 Windows，
      或开一个全新虚拟机，重跑上面第一条 —— **这才是「①修好了」的真凭据**
- [ ] 顺手看一眼安装版装完的目录里，三个运行库 dll 在不在

**记结果时写清哪层验过、哪层没验**，没验就写「未验」，不写「应该没问题」。

---

## 4. 修完之后要做的三件事

1. **把结果记进 `docs/TODO.md`**：Windows CLI 那条从「🟡 缺分发包」改成实际状态
   （出了哪些包、sha256 在哪、GUI 验到哪一层、干净环境验没验）。
2. **别 `git commit`**（项目硬规矩 3）—— 改完停手，diff 交给用户看。
3. **别把 `mode change 100755 => 100644` 提交掉** —— 那是 NTFS 丢了执行位导致的假改动，
   提交了会把 Linux 那边的执行位一起改坏（详见 `AGENTS.md` §仓库在两台机器上 第 3 条）。

---

## 5. 一句话总结

| # | 问题 | 性质 | 严重度 | 谁修 |
|---|---|---|---|---|
| 1 | VC++ 三个 dll 没打包 | Windows 平台 / 分发 | 🔴 别人打不开 | Windows 侧 |
| 2 | 没 sha256 清单 | 发版流程（跨平台一致性） | 🟡 可信度 | Windows 侧 |
| 3 | GUI 没真人验 | 验收流程 | 🟡 不能算交付 | Windows 侧（用户本人点） |

**引擎核心 0 条**。这是打包没打全，不是软件做错了。
