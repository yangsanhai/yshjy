# PeaZip 工程层拆解（外壳 / 系统集成 / CLI / 格式路由 / 后端边界 / 上手 / 打包 / 文档）

分析对象：`/home/user/hermes/yshjy/refs/PeaZip/`（PeaZip Free，LGPLv3，Lazarus/FreePascal + 少量 C++/C#）
分析日期：2026-10-16
数据来源：源码逐文件 grep + 读原文，行号均经二次核对。

> 术语约定：
> - `peach.pas` = PeaZip 主程序（GUI 前端 + CLI 调度，80893 行，`peazip-sources/dev/peach.pas`）
> - `list_utils.pas` = 通用工具（格式识别 / 进程启动，`peazip-sources/dev/list_utils.pas`）
> - `pea.pas` = PEA 格式引擎（独立可执行 `pea`）
> - `externalprograms.pas` = 外部后端清单 + 哈希白名单
> - `PeaZip.ShellEx/` = Windows 11 现代右键菜单壳扩展（C++/WinRT）
> - 所有路径相对仓库根 `refs/PeaZip/`。

---

## 0. 全貌：PeaZip 的"外壳 vs 内核"分工

PeaZip 是**薄 GUI 壳 + 多个独立后端可执行文件**（非 in-process 一体化）。它的工程价值集中在：

| 层 | 载体 | 作用 |
|---|---|---|
| GUI/CLI 前端 | `peach.pas`（编译为 `peazip`/`peazip.exe`） | 界面、命令行解析、任务编排、进度/错误展示 |
| PEA 引擎 | `pea.pas`（编译为 `pea`/`pea.exe`） | 唯一自研格式后端（PEA 加密归档 + 文件工具） |
| 外部后端 | `res/bin/{7z,arc,brotli,zstd,zpaq,lpaq,paq,quad,upx}/` | 各格式真正干活的可执行文件（需自行放入，源码包不提供） |
| 系统集成 | `res/share/batch/**` + `PeaZip.ShellEx/` | 右键菜单 / 文件关联 / SendTo / 服务菜单 |

关键设计取向（对我们做 yshjy 有直接借鉴意义）：**它自己不实现大部分算法**，而是把"识别 → 路由 → 调用 → 解析结果"这套外壳做到极致。这恰好是任务书说"PeaZip 真正值钱的地方"。

---

## 一、系统集成（右键菜单 / 文件关联 / 拖放 / 剪贴板）

### 1.1 跨平台总览

PeaZip 的系统集成**不是一份代码多平台编译**，而是**每个桌面环境各一份静态集成文件**，全部放在 `res/share/batch/` 下按平台分目录。核心事实：**它是"便携优先"应用，默认零安装、零集成**，集成交给发行版安装脚本或用户手动执行（`res/share/batch/freedesktop_integration/readme_freedesktop_integration.txt` 开头明确写了这点）。

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 1.1.1 | 系统集成 | 总目录按平台分层：`bat/`(Win) `sh/`(`*x`) `freedesktop_integration/`(Linux) `macOS service menus/`(Automator) `Windows/`(SendTo+reg) | `peazip-sources/res/share/batch/readme-batch.txt:1-13` | 每类集成一个目录，脚本内容几乎都是一行"调用 peazip 主程序 + 一个 CLI 开关"。 | 结构极简、可整目录拷给发行版打包器；零运行时依赖 | 需为每个 DE 各维护一份（Nautilus/Nemo/KDE/Thunar 都不通用） | ✅已采用 |
| 1.1.2 | 系统集成 | 所有集成入口都收敛到**同一个可执行文件 + 不同 CLI 开关** | `peazip-sources/res/share/batch/bat/Extract here (smart new folder).bat:1`、`sh/Add to 7z.sh:1` | `.bat`/`.sh` 各级菜单项内容就是 `peazip -ext2folder %F` 这种一行调用。 | 集成层极薄，新增菜单项=加一行 shell；GUI 与 CLI 共用同一入口 | 每个菜单项都要起一个进程（无 in-process 集成） | ✅已采用 |

### 1.2 Linux / FreeDesktop

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 1.2.1 | 系统集成 | 主 `.desktop`：`MimeType=` 一口气列 60+ 个 MIME（文件关联），`Actions=` 声明 4 个右键快捷动作 | `peazip-sources/res/share/batch/freedesktop_integration/peazip.desktop:11`(MimeType) `:12`(Actions) | `Exec=peazip %F`；`Actions=add;extract;extracthere;extractheresmart;`；每个 Action 是独立的 `[Desktop Action xxx]` 段 | 标准 FreeDesktop 机制，GNOME/KDE/Xfce 通吃；`%F` 支持多选 | 只有支持 `.desktop Actions` 的 DE 才显示子菜单；老 DE 需退回"单功能 .desktop 文件" | ✅已采用 |
| 1.2.2 | 系统集成 | 4 个 Action 分别映射到 4 个 CLI 开关 | `.../peazip.desktop:14-31` | `add→-add2archive`、`extract→-ext2main`、`extracthere→-ext2here`、`extractheresmart→-ext2folder` | 菜单项与 CLI 一一对应，方便脚本复刻 | 无 | ✅已采用 |
| 1.2.3 | 系统集成 | 兜底：为不支持 Actions 的 DE 另备"单功能 .desktop 文件"目录 | `.../freedesktop_integration/additional-desktop-files/`（目录） | 每个功能一个独立桌面文件（如 `peazip-add-to-wim.desktop`），放进 applications 目录即成为独立菜单项 | 兼容性兜底 | 文件数量膨胀 | 🔄待采用 |
| 1.2.4 | 系统集成 | GNOME/Nautilus：**用 "脚本目录" 实现右键菜单**（非 .desktop） | `.../Nautilus-scripts/PeaZip/`（8 个脚本）、`Nautilus-scripts/readme_add_peazip_to_gnome_context_menu.txt:1-8` | 把脚本拷进 `~/.local/share/nautilus/scripts/`，GNOME 自动在右键"Scripts"子菜单列出 | 零权限、纯用户级；GNOME 官方支持的扩展点 | 需要用户手动 `chmod +x`；位于二级"Scripts"子菜单，非顶级 | ✅已采用 |
| 1.2.5 | 系统集成 | Nautilus 脚本附带"一键安装脚本" | `.../Nautilus-scripts/add_peazip_to_gnome_context_menu.sh:1-2` | 内容仅两行：`cp -r PeaZip ~/.local/share/nautilus/scripts/` + `chmod +x ...` | 用户自助安装极简 | 硬编码路径，非 XDG 兼容 | 🔄待采用 |
| 1.2.6 | 系统集成 | Cinnamon/Nemo：用 `.nemo_action` 文件（Nemo 专有语法） | `.../Nemo-actions/PeaZip, extract here.nemo_action:1-9`、`Nemo-actions/add_peazip_to_cinnamon_context_menu.sh:1-2` | `[Nemo Action]` + `Exec=peazip -ext2smart %F` + `Selection=notnone` + `Extensions=any;` | 语法直观；`Selection=notnone/s` 可控制多选/单选 | Nemo 专有，排除其它 DE | ✅已采用 |
| 1.2.7 | 系统集成 | KDE Dolphin：Service Menu，按 KDE 版本分 4 套（KDE3/4/5/6） | `.../KDE-servicemenus/KDE6-dolphin/peazip-kde6.desktop:1-50` | `Type=Service` + `X-KDE-Submenu=PeaZip`(子菜单分组) + `X-KDE-Priority=TopLevel` + `MimeType=application/octet-stream;inode/directory` | 有**子菜单分组**和**优先级**控制，体验最接近原生 | KDE 3→6 的安装路径与 `ServiceTypes` 关键字逐代变化，维护 4 份 | ✅已采用 |
| 1.2.8 | 系统集成 | KDE 提供"完整版"与"精简版"两份菜单 | `KDE6-dolphin/peazip-kde6.desktop:4`(9 项) vs `peazip-kde6-minimal.desktop:4`(5 项) | 完整版 9 个 Action；精简版只留 add/extract/extracthere/folder/open | 用户按口味二选一；精简版降低菜单长度 | 需维护两份 | 🔄待采用 |
| 1.2.9 | 系统集成 | KDE 安装脚本区分 `kio/servicemenus`(KDE5.85+/6) 与旧路径 | `.../KDE-servicemenus/add_peazip_to_kde_context_menu.sh:1-2` | `cp ./KDE6-dolphin/peazip-kde6.desktop ~/.local/share/kio/servicemenus/` + `chmod +x` | 一行装好 | 只覆盖最新 KDE；旧版需读 txt 手工放 | ✅已采用 |
| 1.2.10 | 系统集成 | Xfce/Thunar：提供 `thunar-archive-plugin` 的 `.tap` 适配器 | `.../thunar-archive-plugin/peazip.tap:1-46` | Thunar 归档插件按约定 `action=$1;shift; folder=$1;shift`，脚本把 `create/extract-here/extract-to` 三种动作翻译成 peazip CLI | 直接接入 Thunar 内置的"创建/解压"流程，无需自造菜单 | 依赖 thunar-archive-plugin，且各发行版 `.tap` 目录不同（readme 里列了 3 种路径） | 🔄待采用 |
| 1.2.11 | 系统集成 | Flatpak/沙箱下的路径改写规则写进文档 | `.../readme_freedesktop_integration.txt` 第 3 节 | 沙箱里 `Exec=peazip` 要换成 `flatpak run --fileforwarding ...`，`%*` 换成 `@@ '$basedir/$*' @@` | 明确交底沙箱限制，不假装能透明工作 | 仅文档说明，无自动化 | ✅已采用 |

### 1.3 Windows

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 1.3.1 | 系统集成 | Win11 现代右键菜单：装 `.reg` 脚本，用 **whitelisted ID 冒充** Microsoft 白名单动词 | `.../Windows/Windows 11 mini context menu/add PeaZip, extract here.reg:10-26` | `[HKEY_CURRENT_USER\...\*\shell\SetDesktopWallpaper]`（`SetDesktopWallpaper` 是 MS 白名单 ID，借壳让项出现在 Win11 一级小菜单） | 绕过 Win11 一级菜单只允许白名单 ID 的限制，无需 MSIX 打包 | **借用别人的白名单 ID**，属灰色技巧；只是"能在小菜单出现"，语义不对 | ⚠️已评估-不采用（借名 hack 不稳且不体面；我们走 MSIX/原生注册或经典菜单） |
| 1.3.2 | 系统集成 | 注册项结构：`MUIVerb`(显示名) + `MultiSelectModel=Player` + `Icon` + `command\@` | `add PeaZip, extract here.reg:14-19` | `"MUIVerb"="PeaZip, extract here"`；`command\@="\"...\PEAZIP.EXE\" \"-ext2multihere\" \"%1\""` | 同时给 `*`(所有文件) 和 `Directory` 两处注册 = 文件和文件夹都生效 | 硬编码安装路径 `C:\Program Files\PeaZip`，便携版要改 | ✅已采用 |
| 1.3.3 | 系统集成 | 提供"卸载/移除"配对脚本 + 白名单 ID 清单文档 | `remove PeaZip entries.reg:1-6`、`readme mini context menu.txt`（末段列 7 个白名单 ID） | 移除脚本只 `[-HKEY_...]` 删两处键；文档解释 `ConnectNetworkDrive/Extract/PinToHome/...` 等可用 ID | 干净可逆；把平台黑话写进文档降低维护门槛 | 仍是借壳 | ⚠️已评估-不采用（同上；但"可逆安装脚本+ID 清单"的做法值得学） |
| 1.3.4 | 系统集成 | **SendTo 菜单**：预置 33 个 `.lnk` 快捷方式（每个功能一个） | `.../Windows/SendTo/SendTo_Program Files/`（33 个 .lnk）、`SendTo/readme-SendTo.txt:1-4` | 每个 .lnk 指向 `peazip.exe <开关>`；分 `Program Files` / `Program Files (x86)` 两套 | 零注册表改动、纯文件拷贝；用户可自行增删 | 33 个 .lnk 无法用源码生成，需二进制资源；仅 Windows | 🔄待采用（机制可用，但 .lnk 需程序化生成） |
| 1.3.5 | 系统集成 | Win11 现代菜单壳扩展（C++/WinRT `IExplorerCommand`） | `PeaZip.ShellEx/PeaZip.ShellEx/ExplorerCommandBase.cpp:87-186` | 实现 `IExplorerCommand`，`InvokeAsync` 拿 `IShellItemArray` 逐项 `SHELLEXECUTEINFOW` 拉起 `peazip.exe` | 真正的一级菜单集成，可多选、可分组、可本地化 | 需 MSIX 打包（见 1.3.6）；C++/WinRT 复杂度高 | ❓未判定（Windows 端是否自研壳扩展，取决于我们是否要 MSIX 分发） |
| 1.3.6 | 系统集成 | 现代菜单通过 MSIX `AppxManifest` 声明 `windows.fileExplorerContextMenus` | `PeaZip.ShellEx/PeaZip.Package/Package.appxmanifest:113-137` | `<ItemType Type="*">` + `<ItemType Type="Directory">` + `<ItemType Type="Directory\Background">` 各绑一个 CLSID；另用 `windows.fileTypeAssociation` 声明约 35 个扩展名 | 一份清单同时搞定右键菜单 + 文件关联 + 执行别名 | 需签名证书；`Publisher="CN=PeaZip"` 未签名只能自签测试 | ❓未判定 |
| 1.3.7 | 系统集成 | 菜单项定义集中在一个头文件 `Commands.h`（ID/图标/CLI 开关三元组） | `PeaZip.ShellEx/PeaZip.ShellEx/Commands.h:3-21` | 每个动作一行宏：`#define EXTRACT_HERE_COMMAND_ARGS L"ExtHere", g_extract_icon_path, L"-ext2multihere "` | **单一事实源**：菜单项 = (资源ID, 图标, CLI 开关)；改一处即可 | 无 | ✅已采用（这个三元组模式可直接抄进 Rust） |
| 1.3.8 | 系统集成 | 菜单顺序/分组由**注册表读配置 + 代码兜底默认**决定 | `PeaZip.ShellEx/PeaZip.ShellEx/ExplorerCommandRoot.cpp:118-135`(读注册表) `:255-275`(默认预设) | 优先读 `HKCR\(AllFilesystemObjects\shell\PeaZip)\"SubCommands"`（`;` 分隔 ProgID），读不到就用内置 15 项默认顺序 | 用户可自定义菜单项/顺序而无需改代码 | 解析逻辑较长（每个 ProgID 一个 else-if 分支） | ✅已采用（"配置优先、代码兜底"值得学） |
| 1.3.9 | 系统集成 | 经典注册表右键菜单由 **InnoSetup 安装脚本**统一注册/卸载 | `peazip-sources/dev/installer/peazip-setup_script_WIN64.iss:1866-1905`(卸载清理) 全文 3231 行 | 安装时写 `HKLM\SOFTWARE\Classes\*\shell\Extract here\...` 等；卸载时 `RegDeleteKeyIncludingSubkeys` 逐条删 | 安装/卸载对称，无残留 | .iss 单文件 3200 行，逐条硬编码菜单项，难维护 | ⚠️已评估-不采用（我们用 MSI/WiX 或配置表生成，不手写 3000 行安装脚本） |
| 1.3.10 | 系统集成 | 拖放：Windows 用**自研 DLL** `dragdropfilesdll.dll` 提供系统的文件拖放（含拖到图标） | `peazip-sources/dev/dragdropfilesdll.src/`（37 文件）、`peazip-sources/readme.txt`（dragdrop 段） | 基于第三方 Lazarus 包 DragDrop 5.2；编译成 dll，主程序运行时加载 | 便携应用无安装也能支持"拖到图标" | 多一个 DLL；见 1.3.11 的安全代价 | ❓未判定 |
| 1.3.11 | 系统集成 | **DLL 反劫持**：启动时校验 `dragdropfilesdll.dll` 的 SHA256 是否在白名单内，不匹配则 `halt` | `peazip-sources/dev/peach.pas:17683-17714`（`internaldllcheck`）、调用点 `:73222` | 硬编码 32/64 位两个已知哈希；不匹配弹错误并退出 | 防 DLL 劫持攻击，纵深防御 | 每次升级 DLL 必须同步改源码常量；跨平台本地构建几乎必被挡 | ✅已采用（"加载自有 DLL 前校验哈希"习惯值得抄；但要用可配置白名单而非硬编码） |

### 1.4 剪贴板

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 1.4.1 | 剪贴板 | 内部"剪贴板"是**应用内自管的面板**（记录 剪切/复制 待办），不是系统剪贴板 | `peach.pas:3123`(PanelClipboard) `:6148`(clipcontent 变量) `:13543-13602` | 用 `StringGridClipboard` + `TClipcontent` 记录"操作/名称/类型"，用于跨目录粘贴、跨归档移动 | 与文件管理器语义一致（剪切/复制/粘贴）；不污染系统剪贴板 | 与外部程序的粘贴不互通 | 🔄待采用（我们做 GUI 时可用同款自管剪贴板） |
| 1.4.2 | 剪贴板 | 配置项 `[Clipboard mode]` 可切换行为 | `peazip-sources/res/conf/conf.txt`（`[Clipboard mode]` 段） | 默认 `0`，作为可选项暴露 | 把有争议的行为做成开关 | 无 | ✅已采用 |

---

## 二、命令行接口设计

### 2.1 架构模型

**核心事实：PeaZip 是"单入口双壳"**。`peazip.exe` 既是 GUI 也是 CLI——启动时先解析 `paramstr(1)`（argv[1]），命中已知开关就走对应功能函数，未命中则按"输入文件路径"走自动推断（`condopenaction`）。**没有独立的 `peazip-cli` 二进制**，也没有 `--help` 式导出人类文档，CLI 参数表主要在帮助 PDF 和批处理样例里。

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 2.1.1 | CLI | GUI 与 CLI 共用同一入口，靠 `paramstr(1)` 分派 | `peach.pas:62626`/`:62874`/`:63332`（`case lowercase(paramstr(1)) of` 三层分派） | 顶层 `case paramstr(1)` 命中 → 执行 → `exit_nosave`；未命中 → 走 GUI 或按输入推断 | 用户只需记一个可执行文件；脚本可复刻 GUI 动作 | 单个 8 万行文件的巨型 case，无集中参数表 | 🔄待采用（共入口的思路要，但要换成结构化解析，不做巨型 case） |
| 2.1.2 | CLI | CLI 开关命名规范：**动作前缀 + 目标**，全小写、单横线 | `peach.pas:72792-73100`、`sh/*.sh` 全套 | 三大前缀：`-add2*`(打包) / `-ext2*`(解压·浏览·测试) / `-setcomment*`；后缀编码目标（`here`/`smart`/`folder`/`7z`/`zip`…） | 一眼看出动作类型；补齐 shell 自动补全友好 | 开关爆炸（`-add2multi7zultra` 这种叠加命名）；无层级 | 🔄待采用（前缀约定好，但要做 subcommand 分层：`yshjy extract --here`） |
| 2.1.3 | CLI | **多选模型** `multi`：支持一次传多个对象 | `peach.pas:72793-72934`（`-add2multi*`）、`:72935-73017`（`-ext2multi*`）、`readme mini context menu.txt` 末段 | 带 `multi` 的开关把选中项全部传给单实例（`MultiSelectModel=Player`）；非 multi 版本只处理一个 | 右键多选不炸开 N 个窗口 | 无 | ✅已采用（必学：多选必须走单实例批处理） |
| 2.1.4 | CLI | 参数位置约定：`paramstr(1)`=动作，`paramstr(2)`=目标目录/等级，`paramstr(3..)`=输入文件 | `peach.pas:56872`(`dn:=paramstr(2)`)、`:56889`、`:56932`(`level:=paramstr(2)`)、`:56966`(`dn:=paramstr(3)`)、`:57089`(第一个文件参数) | 例：`-add2archivemulti7z <level> <dir> <files...>` | 位置化简单，shell 里直接拼 | 位置含义随开关变化，无自描述；易错 | 🔄待采用（改用命名参数 `--level` `--dest`） |
| 2.1.5 | CLI | 解压"目标策略"用独立开关表达（默认目标 = 桌面） | `peach.pas:63161-63195`（`-ext2here`/`-ext2smart`/`-ext2folder`/`-ext2neutral`/`-ext2newfolder`） | `-ext2here` 直接在当前目录、`-ext2smart` 按需新建目录（"smart new folder"）、`-ext2folder` 强制新建、`-ext2neutral` 用已存设置 | 覆盖用户直觉里的几种"解压到哪" | 六七个近义开关难分辨 | ✅已采用（"解压到当前/智能新建/强制新建"三态很有用） |
| 2.1.6 | CLI | 目标快捷目录开关化：Desktop/Documents/Downloads/bookmark1..8 | `peach.pas:72965-73017`、`ext2multidirect` `:57350-57372` | `-ext2multidesktop` → 内部转发 `-ext2archivemultidesktop` | 把"解压到下载夹"变成一行 | 无 | ✅已采用 |
| 2.1.7 | CLI | 维护/工具类开关：`-peazipreset`/`-peaziptotalreset`/`-peaziplanguage <file>` | `peach.pas:62843-62854`、`:28791` | `-peazipreset` 在**读配置之前**拦截（`paramstr(1)` 硬判断），用于配置损坏时自救 | "崩了也能重置"的逃生开关 | 无 | ✅已采用 |
| 2.1.8 | CLI | 注释/元数据操作：`-setcomment <str> <files>` / `-setcommentf <file> <files>` / `-removecomment <files>` | `peach.pas:55052-55098`(`setcommentcl`)、`sh/Set comment.sh:1` | 参数 2 是注释文本或注释文件，参数 3.. 是归档；只对 `.rar/.zip/.zipx` 生效 | 批量写注释 | 硬编码只支持 3 种格式 | 🔄待采用 |
| 2.1.9 | CLI | **GUI 任务 → CLI 脚本导出**（"Save task definition as script"） | `peach.pas:22716-22717`(文案)、`:15834-15845`(日志)、`:65569`(展示) | GUI 里配好一次任务，一键导出等价的命令行脚本；`logcommands=1` 时把每次实际命令记入 `cllog` | 新手能"先点后学命令行"；调试利器 | 需维护"GUI 状态 → CLI 参数"的双向映射 | ✅已采用（强烈推荐：降低 CLI 学习门槛的核心招） |
| 2.1.10 | CLI | 命令行长度保护：超 `max_cl` 报错并中止 | `peach.pas:55106`(`launch_cl` "control command line size")、`:55232-55246` | `if length(cl)>max_cl then` 弹 `txt_cl_long` 并写"拖放失败"标记文件 | 显式失败优于静默截断 | 阈值随平台变化 | ✅已采用 |

### 2.2 退出码约定

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 2.2.1 | CLI/退出码 | **直接沿用 7z 的退出码语义**，并在 UI 里翻译成人类文案 | `peach.pas:53213-53261`(`decode_7za_exitcode`)、文案 `:22318-22334` | 0=成功、1=警告(非致命)、2=致命错误、7=命令行错误、8=内存不足、255=用户中止，其它=未知 | 与后端一致，脚本可断言；但 UI 层务必翻译 | 语义绑定 7z，换后端需重新映射 | ✅已采用（照抄 7z 退出码体系对我们做统一退出码很有价值） |
| 2.2.2 | CLI/退出码 | PeaZip 自身不另造退出码，**透传后端 `ExitStatus`** | `peach.pas:55272`(`result:=P.ExitStatus`)、`:55292`、`:55208`(`decode_7za_exitcode(P.ExitStatus,...)`) | `exit=0` 时置 `setsequenceerror:=0`，非 0 走错误弹窗 | 简单直接 | 不同后端语义不一（UPX 的 1/2 含义不同，见 `:53215-53228` 特判） | 🔄待采用（透传可，但需定义我们自己的稳定退出码层，别把后端码暴露给脚本） |
| 2.2.3 | CLI/退出码 | 小工作量任务后台执行阈值（16MB） | `peach.pas:55313-55318`(`executesmallbk`) | 输入 <16MB 的归档/解压/转换转后台，避免弹窗闪烁 | 体感更快 | 无 | 🔄待采用 |

### 2.3 面向脚本的输出

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 2.3.1 | CLI/输出 | 列表/测试走**控制台窗口**（`cmd /k` 保持不关，`bash -c "...; read line"`） | `peach.pas:55174-55185` | `subfun='list'/'test'/'bench'` 时把命令包成"开控制台 + 执行完不关" | 用户能直接看到 7z 原始输出 | 依赖系统 console；非机器可读 | 🔄待采用（我们做 GUI 时可选"显示原始输出"） |
| 2.3.2 | CLI/输出 | 机器可读输出主要靠**后端自身的 `-slt`/`l` 格式**，前端只做正则解析 | `peach.pas:37885-37901`(`browse_special_cases`，按扩展名选 `slt` 与 `l`) | 默认用 `-slt`（详细技术列表），`.gz/.xz/.rpm/.deb/.cpio` 等退回 `l` | 复用 7z 的稳定输出格式 | 格式适配散落在代码里，脆弱 | ⚠️已评估-不采用（我们自研内核，应提供**结构化输出** `--json`，而非解析第三方文本） |
| 2.3.3 | CLI/输出 | 未找到 `--help`/`-h` 命令行帮助开关 | 全仓库 grep 无 `--help` 处理分支 | — | — | 用户只能查 PDF/网页 | ❓未判定（我们是自研，应补 `--help`，本条记录 PeaZip 缺失） |

---

## 三、格式矩阵与识别策略

### 3.1 支持格式一览（"矩阵表在哪"）

**答案：格式矩阵不在一个表里，而是散在两处**：

1. **面向用户的支持清单（人读）**：`READE_LIST` / `WRITEE_LIST` 两个字符串常量，直接显示在"关于"对话框。
2. **面向机器的路由表（代码）**：`testext()` 一个巨型 `case` 把扩展名 → 数字类型 ID，再由 `testinput()` 把 ID 映射到后端编号，再由 `testarctype()` 把编号映射到内部后端函数名。

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 3.1.1 | 格式矩阵 | 人类可读的"支持格式一览"以两个常量字符串存在 | `peach.pas:6123`(READE_LIST) `:6124`(WRITEE_LIST)、显示点 `:21534-21536` | `READE_LIST` 列读取格式（"7Z, ACE, ARC/WRC, ARJ, BR, BZ/TBZ, CAB, CHM/CHW/HXS, COMPOUND…"）；`WRITEE_LIST` 列写入格式 | 一处维护、多语言共用；关于对话框直接引用 | 只是名字列表，无能力/限制信息 | ✅已采用（好做法：把"能读/能写"拆成两个清单） |
| 3.1.2 | 格式矩阵 | **机器路由表**：`testext()` 用巨型 `case ext of` 把所有扩展名映射到数字类型 ID，分区间编号 | `list_utils.pas:2504-2630`(testext) | 区间约定：`0..99` 传统归档、`100..499` 容器/包/文件系统、`500..599` 非归档但可当归档、`1000+` 其它后端（quad/zpaq/paq/ace/arc/brotli/zstd）、`10000` 合并卷、`10001` Pea | 单一函数即全格式识别入口；区间留空便于扩展 | 200+ 行 case；新增格式要改三处（testext/testinput/testarctype） | 🔄待采用（思路对，但我们要用**数据驱动表**：一个 `formats.toml` 生成代码，避免三处不同步） |
| 3.1.3 | 格式矩阵 | 扩展名 → 后端编号的二级映射 `testinput()` | `list_utils.pas:2645-2669` | 先 `testdir` 判断目录(1000)，再 `testext()`，`i>=0` 默认判为 7z；再对 `2000/3001/4001/5001/5002/10000/10001` 等特例覆盖为 zpaq/ace/freearc/brotli/zstd/合并卷/Pea | 默认走万能 7z，特例才分流 | 默认"任何已知扩展名都当 7z"，掩盖了真实后端 | ✅已采用（万能后端兜底 + 特例分流，是可用的降复杂度手段） |
| 3.1.4 | 格式矩阵 | 编号 → 内部后端函数名的三级映射 `testarctype()` | `peach.pas:37790-37838` | `0→UNCUSTOM`、`1→UNPEA`、`3/4→UN7Z`、`5→UNPAQ`、`6→UNQUAD`、`7→UNACE`、`9→UNARC`、`10→UNZPAQ`、`11→UNBROTLI`、`12→UNZSTD`、`1000→FILEBROWSER` | 分层清晰；能按后端能力分发 | 三张表 + 分散逻辑，扩展成本高 | 🔄待采用 |

### 3.2 识别策略：扩展名 vs 内容嗅探

**关键事实：PeaZip 的"格式识别"几乎纯粹基于扩展名**（`testext`），内容嗅探（magic bytes）只用在一个地方：**显示诊断信息**（状态栏文案）和**几个特例判断**（RAR5、ZPAQ、特殊浏览模式）。**它不用 magic bytes 决定路由**——这是一个明显的薄弱点，也正是我们 yshjy"专治伪装成 MP4 的压缩包"必须超越的地方。

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 3.2.1 | 识别策略 | 主识别 = 扩展名（`testext`），内容嗅探**不参与路由** | `list_utils.pas:2504-2630` | 只看 `extractfileext()`，未知扩展名一律 `-1` | 快、零 IO | **无法识别伪装/改扩展名文件**；这就是我们的差异化机会 | ⚠️已评估-不采用（我们不follow 纯扩展名；必须内容优先、扩展名辅助） |
| 3.2.2 | 识别策略 | 内容嗅探函数 `getmagicbytes_arc()`：读前 8 字节比 magic | `list_utils.pas:3271-3335` | 60+ 条 `if (sbuf[0]=..) and (sbuf[1]=..)…`，返回 `扩展名/类型名`（如 `.zip/ZIP`、`.rar/RAR5+`），仅用于**状态栏显示**（`:40149`） | 给用户"文件真实类型"提示 | 只读前 8 字节；不参与决定用哪个后端 | 🔄待采用（要把它从"显示用"提升为"路由用"，并支持偏移量签名） |
| 3.2.3 | 识别策略 | 嗅探覆盖的签名（含带偏移的特例，注释里标了 offset） | `list_utils.pas:3290-3332` | ZIP `PK`、7Z `37 7A BC AF 27 1C`、RAR4/5、ARJ、BCM、BR、BZIP2、CAB `MSCF`、CHM `ITSF`、COMPOUND `D0 CF 11 E0`、CPIO、DEB `!<arch>`、ELF、EXE `MZ`、FREEARC、GZIP `1F 8B`、LZ4、LZH(offset 2)、LZIP、OGG、PDF、PEA `EA 01`、QCOW `QFI`、RPM、SWF、VDI、VHD、VHDX、VMDK、WAD、WIM `MSWIM`、XAR `xar!`、XZ、Z(1F 9D)、ZST `28 B5 2F FD`、ZPAQ；注释标出 TAR 需 offset 257、ACE offset 7、DMG 末尾 512、ISO 0x8001 | 覆盖面广，可直接当我们的签名表起点 | 注释掉的（TAR/ACE/ISO/DMG）恰好是需偏移量的，说明它偷懒没做 | ✅已采用（把这张表连同偏移量补全，落成 Rust 常量表） |
| 3.2.4 | 识别策略 | RAR5 特判：读第 7 字节 =`$01` 判 RAR5，再决定用哪个 unrar | `peach.pas:36724-36763`(`testifrar5`)、`:51529-51530` | 扩展名 `.RAR` + 第 7 字节 `01` → RAR5 → 走 `unrar`；否则 7z 内置处理 | 用最小代价区分 RAR4/RAR5 | 仅此一处做了"内容判版本" | ✅已采用（局部内容判定值得学） |
| 3.2.5 | 识别策略 | 归档可编辑性判定 `isarchiveeditable`、以及"强制当归档打开"开关 | `peach.pas:6035`(`isarchiveeditable`)、`:37812-37818`(`forceopenasarchive`) | 未知类型时若用户显式要求则按 7z 打开 | 给用户"我就是想试试"的出口 | 无 | ✅已采用（"强制按归档打开"是个好用的兜底开关） |
| 3.2.6 | 识别策略 | "打开为归档"独立开关 `-ext2openasarchive` / `-ext2browseasarchive` | `peach.pas:58210-58244`、`ShellEx Commands.h:13` | 对任意文件强制按归档尝试 | 同上 | 无 | ✅已采用 |
| 3.2.7 | 识别策略 | 特殊浏览模式：按扩展名决定用 `-slt` 还是 `l` 列表输出 | `peach.pas:37862-37901`(`browse_special_cases`) | `.z/.xz/.lz/.txz/.lzma/.rpm/.deb/.man/.slp/.bz2/...` 及 `.cpio` 用 `l`，其余用 `slt` | 适配后端对单流格式的输出差异 | 硬编码扩展名清单 | 🔄待采用 |

### 3.3 "自带格式" PEA

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 3.3.1 | 格式矩阵 | 唯一自研格式 PEA（`pea` 可执行），独立引擎 | `peazip-sources/dev/pea.pas`(9116 行)、`project_pea.lpi` | PEA = 支持级联加密/对象级压缩的归档，由 `pea.exe` 处理，主程序通过 `compose_pea_cl`/`compose_unpea_cl` 驱动 | 掌握一个可控的自有格式（加密/分卷） | 自研格式生态小，无外部工具可读 | ⚠️已评估-不采用（我们不做自有格式，专注"解别人的包"） |

---

## 四、调用外部后端时的边界处理

这是 PeaZip 的**真本事所在**。它的外壳不实现算法，全部价值就在于"把一堆第三方可执行文件当库用"时的健壮处理。

### 4.1 后端清单与信任模型

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 4.1.1 | 后端边界 | 所有第三方后端集中在一个 unit 里声明版本与许可 | `peazip-sources/dev/externalprograms.pas:55-126` | 每平台一组 `APPLICATION_7Z/ZPAQ/UNRAR/BROTLI/ZSTD/FREEARC/UNACE/QUAD/UPX/STRIP` 字符串，含版本号+作者+许可 | 许可信息一处可见，合规友好 | 常量随版本升级要手改 | ✅已采用 |
| 4.1.2 | 后端边界 | 后端二进制**不随源码分发**，安装包按平台/架构放 `res/bin/<名>/` | `peazip-sources/res/bin/readme-bin.txt:1-3`、`res/bin/7z/place 7z binary here.txt` | 源码包只留空目录说明，实际二进制由打包者下载放入 | 回避许可再分发问题；平台可裁剪 | 构建者需自行备齐二进制 | ✅已采用（我们若走"一体化"则相反——内嵌库；但"按架构分目录"的布局值得学） |
| 4.1.3 | 后端边界 | **每个后端二进制的 SHA256 白名单硬编码**，运行时校验 | `externalprograms.pas:160-266`（`H7Z_*`/`HBROTLI_*`/`HZSTD_*`/`HZPAQ_*`/`HARC_*`/`HPAQ8O`… 共 60+ 条） | 例：`H7Z_LIN_X = '277907BC...' //7z linux x86_64`；随包分架构 | 防替换/防投毒，纵深防御 | 升级后端必须同步改哈希，易忘→构建期可校验 | ✅已采用（我们内嵌库可用构建期 `cargo vet`/哈希锁替代） |
| 4.1.4 | 后端边界 | 各平台/架构能力差异用 `{$IFDEF}` 显式留空 | `externalprograms.pas:76-145`（Linux aarch64 下 ZPAQ/UNACE/FREEARC 为空串，Windows 独有 UNRAR/UPX） | 缺能力的格式直接声明为不可用 | 诚实交底平台差异，而非假装支持 | 无 | ✅已采用 |
| 4.1.5 | 后端边界 | **"自由软件指令" `libre_directive`**：0=关 / 1=灵活 / 2=严格只用 OSI 许可后端 | 定义 `peach.pas:23139`、加载 `:25565-25569` 与 `:23139`、应用点 `:36733-36734`、`:36777-36778`、`:37381`、`:38083` | 级别 >0 时，检测到 RAR5/ACE 且只能靠非自由 `unrar`/`unace` 时，直接报"不支持"而不调用 | **把许可约束做成可配置的运行时开关**；满足"只用开源"红线 | 用户开了严格模式会突然"某格式不支持" | ✅已采用（这是我们"只用开源技术"原则的现成范式，直接抄） |

### 4.2 进程启动、密码交互、进度与错误

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 4.2.1 | 后端边界 | 统一进程封装：`peapexecute()`/`peapparamexecute()` | `list_utils.pas:3384-3414` | `pmode=0` 用整串 `CommandLine`；`pmode=1` 用 `ParseCmdLine`（非 Windows 传 `ReadBackslash=true`）；`logcommands=1` 时记录命令 | 全部后端调用走同一入口，便于统一记日志/超时 | 命令拼接靠字符串，注入面需自行把关 | ✅已采用 |
| 4.2.2 | 后端边界 | 7z 输出解析走 `-slt` 技术列表 + 正则 | `peach.pas:36592`(`listun7z_do`)、`:37885-37901` | 调 7z `l -slt`，逐行解析成内部结构 | 复用成熟输出 | 后端版本升级可能改格式→解析失配 | ⚠️已评估-不采用（我们自研内核 + 结构化 API，不做文本解析） |
| 4.2.3 | 后端边界 | 密码交互三种模式：GUI 弹窗 / 强制控制台输入 / 管道 | `peach.pas:23167`(`intpw` 默认0)、`:38285-38292`(`if intpw=1 then [poWaitOnExit/poNewConsole] else [poUsePipes, poNoConsole]`)、`FormPW` 准备 `:16603-16627` | 默认用管道喂密码；勾选"强制交互输入"则开真控制台让后端自己提示 | 覆盖"后端会自己在终端问密码"的麻烦场景 | 强制控制台时无法用管道同时读进度 | ✅已采用（关键洞察：**接第三方后端时必须留"交互透传控制台"逃生口**） |
| 4.2.4 | 后端边界 | 后端"卡住/无法自动喂密码"的已知坑写成条件分支 | `peach.pas:52559`、`:57666`、`:58054`（ZPAQ：非加密强制空密码、加密强制默认密码，"否则会 hang"） | `if getmagicbytes_arc(in_param)='.zpaq/ZPAQ' then` 特殊处理密码 | 把踩过的坑固化进代码 | 后端专有 hack 散落 | ✅已采用（做法要学：为"会挂死后端"写显式特判） |
| 4.2.5 | 后端边界 | 进度/耗时/速度由**前端自己算**（掐时间、按输入大小估速），不解析后端进度 | `peach.pas:55138`(`tsin`)、`:55201-55207`(`tdiff/speed` 计算)、`:55128-55137`(拼状态串) | 记录启动时间戳，等进程结束后用 `输入字节数×1000/耗时ms` 估 B/s | 不依赖后端进度输出，通吃所有后端 | 只有"完成时长"，无实时进度条 | 🔄待采用（我们自研能拿真实进度；但对不可解析的第三方可保留此估算法） |
| 4.2.6 | 后端边界 | 退出码 → 人类文案的集中翻译 | `peach.pas:53213-53261`(`decode_7za_exitcode`)、文案 `:22318-22334` | 按 `case i of 0/1/2/7/8/255/else` 弹不同级别对话框（警告/错误），并附上原始命令行 | 错误信息含"原始命令"，可自助排查 | 无 | ✅已采用（"错误里带原始命令行"是极好的可诊断性设计） |
| 4.2.7 | 后端边界 | 完成后处理钩子：清理临时目录、写"失败标记"、移动输出 | `peach.pas:55117-55121`(`clean_after_launch`)、`:55235-55243`(写 `STR_TMPDROPE` 标记)、`:53263-53315`(`removezpaqnesting`/`movezpaq` 修 ZPAQ 的多层嵌套输出) | 拖放任务失败时写标记文件让调用方知道；ZPAQ 输出多余嵌套目录时自动上提 | 处理第三方输出不规范 | 每个后端特判 | ✅已采用（"为不合规后端的输出做后处理"是真需求） |
| 4.2.8 | 后端边界 | 目录清理带重试（防文件被锁） | `peach.pas:34735-34749`(`cleardir` 注释"若首次未清空(被系统/安全软件锁)则 sleep 重试") | `rmdir /s /q` 或 `rm -r`，失败则重试若干次 | 抗 Windows 文件锁 | 无 | ✅已采用 |
| 4.2.9 | 后端边界 | 任务窗口"是否保持打开看报告"可配置 4 档 | `res/conf/conf.txt`（`[0: always keep open... 3: never...]` 段） | 0 总是留、1 默认只在错误/列表/测试留、2 只在错误/列表留、3 从不 | 把"看不看日志"交给用户 | 无 | ✅已采用 |
| 4.2.10 | 后端边界 | 未找到**超时/kill** 机制 | 全仓库 grep `timeout`/`kill` 无对应后端管控逻辑 | 依赖 `TProcess` 的 `Running` 轮询（`peach.pas:55196-55200`），无超时中止 | 简单 | **后端卡死=前端卡死**，只能用户手动关；这是可借鉴的反面 | ⚠️已评估-不采用（我们自研内核无此问题；若将来接外部后端必须补超时+kill） |
| 4.2.11 | 后端边界 | 前端最大命令行长度限制保护 | 见 2.1.10 | — | — | — | ✅已采用 |

---

## 五、新手上手设计

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 5.1 | 上手 | **首启动零向导**：默认配置全套内置，直接可用 | `peach.pas:23060-23160`（初始化默认值区）、`res/conf/conf.txt`（847 行默认配置，全带注释） | 不弹"首次运行向导"；所有默认值硬编码在源码 + 一份带注释的 `conf.txt` 作参考 | 便携应用免打扰；名值对+注释的配置格式自解释 | 新手要改行为得自己翻配置 | ✅已采用（"默认即最佳体验"符合我们"简单好用就行"） |
| 5.2 | 上手 | 便携 vs 可安装由**一个空文件**是否存在决定 | `res/portable`(空文件)、`res/readme-res.txt:1-2`、`peach.pas:28745-28747`、`:22866` | 存在 `res/portable` → 配置写 `res/conf/`；否则写用户目录（XDG/AppData） | 零参数切换便携模式，极致简单 | 无 | ✅已采用（这个"空文件当开关"的便携约定非常优雅，直接抄） |
| 5.3 | 上手 | 配置布局：便携=程序目录；可安装=用户目录（Windows 显式排除虚拟化） | `peach.pas:28745-28777`、`PeaZip.Package/Package.appxmanifest:44-49`（`ExcludedDirectory = RoamingAppData\PeaZip`） | 配置路径分两级判定；MSIX 里声明不虚拟化配置目录 | 便携/安装两种场景都自然 | 无 | ✅已采用 |
| 5.4 | 上手 | 危险操作确认：删除/覆写/安全擦除前的 Yes/No 弹窗 | `peach.pas:57382`(`pMessageWarningYesNo(txt_wipe)`) 等 | 安全擦除等破坏性操作弹 `pMessageWarningYesNo` | 防误操作 | 无 | ✅已采用 |
| 5.5 | 上手 | 危险开关带醒目提示文案（"be careful"） | `res/share/presets/0.txt`（"Delete files after archiving (be careful)"） | 高风险选项在 UI/配置里显式标注 | 低成本降低误操作 | 无 | ✅已采用 |
| 5.6 | 上手 | 设置分层：常用(App options) / 高级(Advanced) 分页 | `peach.pas`(FormPeach 大量 `txt_7_x/txt_8_x/txt_9_x` 分节文案)、`res/conf/conf.txt` 按 `[标题]` 分节 | 配置项按主题分节，UI 对应分页；新手看"app options"，进阶翻高级页 | 渐进披露 | 配置文件 847 行仍偏长 | ✅已采用 |
| 5.7 | 上手 | 一次任务可"另存为脚本"，让 GUI 用户平滑过渡到 CLI | 见 2.1.9 | — | — | — | ✅已采用 |
| 5.8 | 上手 | 语言：56 个语言文件，`default.txt` 兜底 | `res/share/lang/`(56 个 `.txt`)、`peach.pas:62843-62845`(`-peaziplanguage <file>`) | 语言是纯文本键值文件；`default.txt` 为英文兜底 | 加语言=加文件 | 字符串散在代码 `txt_*` 变量里 | 🔄待采用 |
| 5.9 | 上手 | Windows 端"系统集成向导"是**独立外部程序** | `peach.pas:28845-28864`(`configure_systemintegration` 调 `Configure PeaZip.exe /DIR=...`)、`res/bin/readme-bin.txt`（Windows 独有配置向导） | 主程序只负责拉起向导并传安装路径 | 主程序不必内嵌所有注册表逻辑 | 多一个二进制；仅 Windows | ❓未判定 |
| 5.10 | 上手 | "重置"逃生开关：普通重置 / 完全重置 | `peach.pas:62847-62854`、`:28791` | `-peazipreset` 与 `-peaziptotalreset`，且在**读配置前**拦截以应对配置损坏 | 出错也能救回 | Windows 下重置还会问"要不要重配系统集成"（`:35431`） | ✅已采用 |

---

## 六、构建 / 打包 / 分发

### 6.1 构建脚本数量与形态

**答案：构建"脚本"其实很少——主力是 2 个 Lazarus 工程文件 + 1 个 CI 用 Pascal 构建程序**。`dev/` 下那 30+ 个 `.bat` 大多是**加密算法测试/一次性小工具**的编译脚本（`#ca_*.bat` 等），不是产品构建。

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 6.1.1 | 构建 | 产品只需编 2 个工程：`project_pea`(引擎) + `project_peach`(GUI+CLI) | `peazip-sources/readme.txt`(QUICK START 第2步)、`dev/project_peach.lpi`、`dev/project_pea.lpi` | `lazbuild project_peach.lpi` + `lazbuild project_pea.lpi`；Windows 另编 `dragdropfilesdll.lpi` | 工程文件极少，构建路径清晰 | 需先装 Lazarus + 两个可选包 | ✅已采用（我们要 2 个 crate：lib + bin，思路一致） |
| 6.1.2 | 构建 | 用 Pascal 写的**跨平台 CI 构建程序** `make.pas`，自动发现并构建所有 `.lpi` | `/.github/workflows/make.pas:173-210`(`BuildAll`) | `FindAllFiles('*.lpi')` → 排序 → 逐个 `lazbuild --build-all --recursive`；`consoletestrunner` 工程自动跑测试；`library`+`exports` 工程自动装到 `/usr/lib` + `ldconfig` | 新增工程零配置自动纳入；构建+测试+安装一条龙 | 约定式（靠项目类型猜动作），不透明 | ✅已采用（"发现式构建"比手写 N 条规则好维护） |
| 6.1.3 | 构建 | CI 精简：GitHub Actions 矩阵 = {ubuntu, windows} × {x86_64, aarch64} | `/.github/workflows/make.yml:24-31` | 两个 runner OS，两个 arch；Unix 用 `setup.sh`(apt 装 lazarus / brew 装)，Windows 用 `setup.ps1`(下载 Lazarus 4.0 静默安装) | 4 组合覆盖全平台 | macOS/BSD 未纳入 CI（手工发） | ✅已采用 |
| 6.1.4 | 构建 | 依赖以 **Lazarus 包(.lpk)** 形式管理，含子模块 | `peazip-sources/readme.txt`(lazbuild --add-package 段)、`.gitmodules`(metadarkstyle 子模块)、`dev/dragdropfilesdll.src/DragDropLazarus.lpk` | 需额外包：`metadarkstyle`(Win 暗黑模式)、`DragDropLazarus`(拖放)；用 `--add-package` 注册 | 依赖声明式；可选依赖可关 | 包依赖逻辑绑在 IDE 上（readme 专门写了一段"如何加/移除 .lpk 依赖"） | 🔄待采用（我们依赖走 Cargo，语义更强） |
| 6.1.5 | 构建 | 版本信息集中在 `compvers.pas` | `peazip-sources/dev/compvers.pas` | 版本相关常量单文件 | 一处改版本 | 无 | ✅已采用 |
| 6.1.6 | 构建 | 遗留的一次性 `.bat` 散落（加密算法测试、资源编译、链接） | `dev/*.bat`(61 个，如 `#ca_*.bat`、`ppas.bat`、`src.bat`、`windres_*.bat`)、`dev/cl.bat` | 多是单文件编译/打包辅助（如 `windres_peazip.bat:1` = `windres -i peazip.rc -o peazip.res`） | — | 61 个 .bat 无索引、命名混乱（`!`/`#` 前缀），新人无从下手 | ⚠️已评估-不采用（反面教材：我们要有统一的 `justfile`/`Makefile` 单一入口） |

### 6.2 打包与分发

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 6.2.1 | 分发 | 分发形态 5 种：Portable / DEB / RPM / Flatpak / DMG / Windows 安装器 / MSIX | `res/share/readme/readme_Linux.txt:3-4`、`readme-macOS.txt`、`readme_Windows.txt`、`PeaZip.Package/`、`res/share/readme-pack.txt` | 各有独立制作流程 | 覆盖广 | 维护成本高 | 🔄待采用（我们先做 portable + 1~2 种主流包） |
| 6.2.2 | 分发 | 打包说明单列一份 `readme-pack.txt`（面向发行版打包者） | `peazip-sources/readme-pack.txt`(全文)、位置 `dev/README.md` 与 `.github` 说明 | 逐平台写"怎么改 Portable/DEB/RPM/DMG/Windows 包" | 把打包者的活与开发者分开交代 | 与主 readme 有部分重复 | ✅已采用 |
| 6.2.3 | 分发 | Windows 安装器用 **InnoSetup**（Pascal 方言 `.iss`），按 WIN32/WIN64 分脚本 | `dev/installer/peazip-setup_script_WIN64.iss`(3231行)、`peazip-setup_script_WINDOWS.iss`(3229行)、`peazip-setup_script-configure.iss`(3189行) | 三份近 3200 行的安装脚本 | 功能齐（含系统集成向导） | 三份高度重复、逐条硬编码注册表键，维护成本极高 | ⚠️已评估-不采用（我们改用表格驱动/单一脚本生成多平台，避免三份 3000 行） |
| 6.2.4 | 分发 | macOS 未签名，需用户手动 `xattr -dr com.apple.quarantine` | `res/share/readme/readme_macOS.txt:1-9` | 文档里直接给出解除隔离属性的命令 | 诚实交底，用户可自救 | 无签名影响信任度 | 🔄待采用（我们要尽量签名/公证，退而求其次才写 xattr） |
| 6.2.5 | 分发 | Linux：GTK2/Qt6 双 widgetset 构建，aarch64/x86_64 双架构 | `readme_Linux.txt:3-4` | 预编译包覆盖 {GTK2, Qt6} × {aarch64, x86_64}；Qt6 需 `libQt6Pas.so` | 兼容新旧桌面 | 组合数多 | ⚠️已评估-不采用（Rust GUI 用单一后端即可，避免 widgetset 矩阵） |
| 6.2.6 | 分发 | Flatpak 沙箱路径改写与 `--fileforwarding` 提示 | `readme_freedesktop_integration.txt` 第3节 | 沙箱内 `Exec`/`%*` 需改写 | 支持沙箱分发 | 仅文档 | 🔄待采用 |

### 6.3 体积/依赖控制

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 6.3.1 | 体积 | 资源按"架构相关 / 架构无关"分离 | `res/readme-res.txt:7-10`：`bin`=架构相关二进制；`share`=架构无关(脚本/许可/图标/语言)；`conf`=便携配置 | 打多架构包时 `share` 只存一份 | 多架构分发省体积 | 无 | ✅已采用 |
| 6.3.2 | 体积 | 主程序 strip + UPX 压缩，体积可控 | `externalprograms.pas:67`(APPLICATION_STRIP 含 UPX)、`peach.pas`(UPX 后端支持) | 发布前 strip 再 UPX | 体积小 | UPX 可能触发杀软误报 | 🔄待采用（Rust 发布直接 `strip`/`lto`，UPX 视情况） |
| 6.3.3 | 体积 | 后端按需选装（架构/平台不同，部分后端为空） | `externalprograms.pas:76-145` | Linux aarch64 少若干后端 | 体积与能力权衡 | 无 | ✅已采用 |

---

## 七、文档组织

| # | 维度 | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|---|
| 7.1 | 文档 | 文档分**开发者向(源码包 readme.txt)** 与 **用户向(help PDF + readme_\*) **两类 | `peazip-sources/readme.txt`(18KB 开发者构建指南)、`res/share/peazip_help.pdf`(1.4MB 用户手册)、`res/share/readme/readme_{Linux,macOS,Windows}.txt` | 源码包顶层 readme 讲"怎么编译/打包"（含 QUICK START + DETAILED GUIDE 两段）；用户手册 PDF 讲"怎么用" | 受众分离，各取所需 | 用户手册是 PDF，不便检索/翻译 | 🔄待采用（我们把用户手册做成 Markdown/HTML 站，开发者指南留 README） |
| 7.2 | 文档 | 开发者 readme 用"QUICK START（N 步 to-do）+ DETAILED GUIDE（逐目录讲解）"双层结构 | `peazip-sources/readme.txt:9-20`(QUICK START)、`:60+`(DETAILED GUIDE) | 先给 6 步可跑通的最小路径，再展开细节 | 新人能 5 分钟上手，进阶有深度 | 无 | ✅已采用（"先跑通再讲原理"的文档结构，非常值得学） |
| 7.3 | 文档 | 每个资源目录都放一个 `readme-*.txt` 就地说明 | `res/readme-res.txt`、`res/bin/readme-bin.txt`、`res/share/batch/readme-batch.txt`、`res/share/presets/`(配置文件头注释) | 目录即文档，就近解释"这里放什么" | 维护者走到哪读到哪 | 无 | ✅已采用 |
| 7.4 | 文档 | 每个集成子目录都有 `readme_*.txt` 说明安装路径（分 DE 版本） | `freedesktop_integration/readme_freedesktop_integration.txt`、`Nautilus-scripts/readme_add_peazip_to_gnome_context_menu.txt`、`KDE-servicemenus/readme_add_peazip_to_kde_context_menu.txt`、`thunar-archive-plugin/README.md` | 明确写"拷到哪个目录（含多个发行版路径变体）" | 用户按图索骥 | 路径随发行版变，需常更新 | ✅已采用 |
| 7.5 | 文档 | 官网 wiki 承载 FAQ/帮助/更新日志/主题/翻译，仓库只留"如何构建" | `README.md`(Useful Links 段) 指向 `peazip.github.io/peazip-help-faq.html` 等 | 仓库 README 保持精简并外链官网 | 仓库不臃肿；文档可热更 | 离线用户拿不到完整文档 | 🔄待采用（我们可仓库放 Markdown、再生成静态站） |
| 7.6 | 文档 | 离线帮助随包分发（`peazip_help.pdf` 在 `res/share/`） | `res/share/peazip_help.pdf`、`peach.pas:63816`/`:65035`(`cp_open(sharepath+'peazip_help.pdf')`) | 帮助菜单直接打开随包 PDF | 离线可用 | 体积 +1.4MB；PDF 难 diff | 🔄待采用（改 Markdown，体积小且可检索） |
| 7.7 | 文档 | 变更日志单文件 + 仓库 `CHANGELOG.md` | `peazip-sources/dev/history`(被 `src.bat` 打包)、`CHANGELOG.md`(124B) | 两份（详细 history + 仓库简版 CHANGELOG） | 有历史可查 | 仓库 CHANGELOG 仅 124B，几乎空 | ❓未判定 |

---

## 八、给 yshjy 的落地清单（按优先级）

**强烈建议直接采用（低成本高收益）**

1. **CLI 三层模型**：单入口 + `-add2*`/`-ext2*` 前缀约定 + `multi` 多选单实例批处理（2.1.1–2.1.3）。但**改用命名参数分层**（`yshjy extract --here --smart`），不做巨型 case。
2. **沿用 7z 退出码语义**并在 UI 层翻译，**错误信息带原始命令**（2.2.1、4.2.6）。
3. **"GUI 任务导出为 CLI 脚本"**（2.1.9）——降低 CLI 门槛的核心招，符合我们"新手上手简单"。
4. **"自由软件指令"运行时开关**（4.1.5）——我们"只用开源技术"的现成范式。
5. **内容嗅探签名表**（3.2.3）落成 Rust 常量表，**并补全被 PeaZip 注释掉的带偏移签名**（TAR offset 257、ISO 0x8001、ACE offset 7、DMG 末尾 512）。
6. **"空文件当便携开关"**（5.2）+ 资源按架构相关/无关分离（6.3.1）。
7. **DLL/外部依赖 SHA256 白名单校验**的习惯（1.3.11、4.1.3）——我们内嵌库可用构建期锁替代。
8. **文档"QUICK START + DETAILED GUIDE"双层结构** + 每目录 `readme-*.txt` 就地说明（7.2、7.3）。
9. **菜单项 = (资源ID, 图标, CLI开关) 三元组单点定义**（1.3.7）。
10. **多桌面集成各备一份静态文件**（1.2.x），并把"安装路径随发行版变"写进各自 readme。

**在 PeaZip 弱项上超越（我们的差异化）**

- **识别路由内容优先**：PeaZip 纯扩展名路由（3.2.1），我们做 magic bytes + 扩展名 + "强制按归档打开"（3.2.5/3.2.6）三管齐下，直击"伪装 MP4 的压缩包"。
- **补 `--help`/`--json`**：PeaZip 无 CLI 帮助、无机器可读输出（2.3.2、2.3.3），我们必做。
- **超时/kill**：PeaZip 无后端超时（4.2.10），我们自研内核无此问题，但要为将来外部后端预留。

**明确不采用（附原因）**

- Win11 借 Microsoft 白名单 ID 注册右键菜单（1.3.1/1.3.3）——不稳、不体面。
- 手写三份 3000 行 InnoSetup 脚本（6.2.3）——改表格驱动/单一脚本生成。
- GUI 与 CLI 塞进同一个 8 万行文件、无集中参数表（2.1.1）——我们做结构化解析。
- 复制 61 个无索引 `.bat` 构建脚本（6.1.6）——统一 `justfile`/`Makefile` 入口。
- 用文本解析第三方后端输出（2.3.2、4.2.2）——我们自研内核给结构化 API。
- 自研格式 PEA（3.3.1）——我们专注"解别人的包"，不做自有格式。
- Linux 双 widgetset 矩阵（6.2.5）——Rust GUI 单后端即可。

---

## 附：本文未覆盖/存疑项（❓未判定）

| # | 项 | 出处 | 存疑原因 |
|---|---|---|---|
| A1 | Windows 端是否采用现代壳扩展（MSIX）路线 | `PeaZip.ShellEx/**`、`Package.appxmanifest:113-137` | 取决于 yshjy 是否要 MSIX 分发；C++/WinRT 复杂度 vs 收益未评估 |
| A2 | Windows "系统集成向导"独立程序的分发细节 | `peach.pas:28845-28864`、`res/bin/readme-bin.txt` | 向导本体不在源码仓（`res/bin` 为空），只知调用约定 |
| A3 | 拖放 DLL（`dragdropfilesdll.dll`）的自研必要性 | `dev/dragdropfilesdll.src/**` | 若我们用原生 GUI 框架自带 DnD 则不需要；跨平台后端选择未定 |
| A4 | 变更日志双份（`history` + `CHANGELOG.md`）如何同步 | `dev/history`、`CHANGELOG.md`(124B) | 未见同步脚本 |
| A5 | `thunar-archive-plugin` 各发行版 `.tap` 路径的正确性 | `thunar-archive-plugin/README.md` | README 自述"2020 年信息，可能已重构" |
