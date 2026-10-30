# 发布检查（T5）—— 逐项核对

## 0.9.35 发布记录（2026-10-30，最新一次）

> 内容：清 `docs/AUDIT-GAP.md` 对账欠账 4 批 9 项 —— 空间不足三个数 + 三条出路、
> 自动找解压码拆两档、设置页两个假开关接成真勾选，以及两个只读代码看不出来的界面 bug
> 和「界面写 `"off"`、引擎 `as_bool()` 读不到」的静默失效坑。详见 `CHANGELOG.md` 的 `[0.9.35]` 节。
> 本轮 7 个提交、**没有**旧产物冲突（版本号是新的），一条龙跑完。

| 步骤 | 结果 |
|---|---|
| 五处版本号 → 0.9.35 | 6/6（`Cargo.toml` / `ffi/Cargo.toml` / `gui/pubspec.yaml` / `PROTOCOL.md` 两处 / `README` 两处；`docs/yshjy.1` 由生成器带出 = 0.9.35） |
| `cargo build --release` | 成功（18.8 秒） |
| `flutter build linux --release` | 成功（`version.json` = 0.9.35） |
| `release.sh` | 常用版 + 全能版各 3.8M |
| `build-deb.sh` | deb **9.9M**，自查通过：CLI + GUI + 菜单项(14 种格式) + 图标 + man + 三种补全 + 右键脚本，版本 0.9.35 |
| `test-packages.sh` | **37 项全绿 / 0 失败**（含「deb 内引擎版本 == 包版本」「真打包真解压逐字节一致」） |
| 装机 | 0.9.34 → **0.9.35**（`sudo apt install ./dist/yshjy_0.9.35_amd64.deb`，用户确认 sudo） |

**装机后四项实测（2026-10-30，全部真跑）**：`dpkg`/`--version` = 0.9.35 ✔；菜单 `MimeType` **14 种格式** ✔；
`bash tests/pick.sh /usr/bin/yshjy` **11 通过 / 0 失败** ✔；装机版真打包→真解压，
5 个文件（中文名 + 三层嵌套 + 二进制）**sha256 逐一致** ✔。

## 0.9.33 发布记录（2026-10-30）

> 打包页两处修复：输出名跟着输入变、完成后显示真实路径并给「打开所在文件夹」。
> 引擎 `pack` 新增 `out` 字段。流程与 0.9.32 完全相同（这轮**没有**旧产物冲突，
> 因为版本号是新的），直接 `release.sh` → `build-deb.sh` → `test-packages.sh` 一条龙。

| 步骤 | 结果 |
|---|---|
| 五处版本号 → 0.9.33 | 5/5 |
| `cargo build --release` | 成功（18.8 秒） |
| `flutter build linux --release` | 成功（`version.json` = 0.9.33） |
| `release.sh` / `build-deb.sh` | 两档 tar.gz 各 3.7M；deb 9.9M，自查 14 种格式通过 |
| `test-packages.sh` | **37 项全绿** |
| 装机 | 0.9.32 → **0.9.33** |

**装机后实测**：`dpkg`/`--version` = 0.9.33；菜单 **14 种格式**；`tests/pick.sh` **11 项全绿**；
装机版 `libapp.so` 里「文件在这儿」「打开所在文件夹」「复制路径」三个新文案都在（UTF-16LE）；
实跑 `--serve` 的 `pack` → **返回 `out` 字段**且产物 1108 B 真落地。

## 0.9.32 发布记录（2026-10-30）

> 内容：GUI「加密包是死路」修复（含预览页直接给解压码输入框）+ 从移动硬盘拉回的
> Windows 战役 W1~W9。**这一轮界面 Dart 改了两次**，所以界面重编了两遍、包也重出了一遍
> ——`release.sh` / `build-deb.sh` 遇到已存在的产物会**直接报错不覆盖**，
> 必须先把旧产物挪走再跑，否则会打出「tar.gz 是新的、deb 是旧的」这种不一致组合。

| 步骤 | 命令 | 结果 |
|---|---|---|
| 五处版本号同步 | `Cargo.toml` / `ffi/Cargo.toml` / `gui/pubspec.yaml` / `docs/PROTOCOL.md`（2 处）/ `CHANGELOG.md` 段落标题；`docs/yshjy.1` 由打包脚本带出 | 5/5 |
| 引擎 release 构建 | `cargo build --release` | 成功（22 秒，9.0M） |
| 界面 release 构建 | `cd gui && ~/flutter/bin/flutter build linux --release` | 成功（第二次含预览页输入框） |
| 出两档 tar.gz | `bash scripts/release.sh` | 各 3.7M |
| 出 deb | `bash scripts/build-deb.sh` | `dist/yshjy_0.9.32_amd64.deb` 9.9M，自查 14 种格式通过 |
| 验包 | `bash scripts/test-packages.sh` | **37 项全绿** |
| 装机 | `sudo apt install ./dist/yshjy_0.9.32_amd64.deb` | 0.9.31 → 0.9.32 |

**装机后实测**（用 `/usr/bin/yshjy` + `/usr/lib/yshjy/gui` 本身）：

- `dpkg -l yshjy` = **0.9.32**、`--version` = 0.9.32
- 桌面菜单认领 **14 种格式**（按 `build-deb.sh` 算法数 MIME 总数，别用手数 `x-` 开头那种）
- `bash tests/pick.sh /usr/bin/yshjy` **11 项全绿**
- 装机版界面 `libapp.so` 核对：老废码 `PASSWORD_REQUIRED` **0 次**、`NEED_PASSWORD` 在、
  预览页新文案「这个包有」「留空也行」在
  （**坑**：Dart AOT 里中文按 **UTF-16LE** 存，`strings`/UTF-8 搜中文一律 0 次，会误判成"没编进去"）
- 真打包 `ys 目录 -o 伪装视频.mp4` → `file` 认成 `ISO Media, MP4`；带码解压 AES 包 →
  **5 个文件 sha256 与源完全一致**
- 引擎回归 `cargo test` **390 过 / 0 挂 / 1 ignored**；`bash tests/all.sh` **41 过 / 0 挂 / 5 跳过**

**本机环境坑**：`flutter test` 必须带 `XDG_CONFIG_HOME=<临时目录>`（`tests/all.sh` 就是这么跑的），
否则 `app_settings_test.dart` 的安全闸会 `fail`「拒绝动用户真实配置」，七条全红——
**那不是代码问题**。

## 0.9.31 发布记录（2026-10-29）

> 只改了一处：`--pick` 被 Ctrl-C 打断后的光标复原（引擎 + 探针，界面逻辑一行没动）。
> 但版本号会进界面状态栏，所以界面也重编了一遍。

| 步骤 | 命令 | 结果 |
|---|---|---|
| 五处版本号同步 | 手工改 `Cargo.toml` / `ffi/Cargo.toml` / `gui/pubspec.yaml` / `docs/PROTOCOL.md` 两份示例 / `CHANGELOG.md` 段落标题；`docs/yshjy.1` 由 `gen-cli-assets.py` 在打包时带出 | 5/5 |
| 引擎 release 构建 | `cargo build --release` | 成功（18 秒） |
| 界面 release 构建 | `cd gui && ~/flutter/bin/flutter build linux --release` | 成功 |
| 出两档 tar.gz | `bash scripts/release.sh` | `yshjy-` / `yshjy-full-` 各 3.7M |
| 出 deb | `bash scripts/build-deb.sh` | `dist/yshjy_0.9.31_amd64.deb` 9.9M，自查 14 种格式通过 |
| 验包 | `bash scripts/test-packages.sh` | **37 项全绿** |
| 装机 | `sudo apt install ./dist/yshjy_0.9.31_amd64.deb` | 0.9.30 → 0.9.31（先 `apt-get -s` 模拟：只升级 1 个包，0 新装 0 卸载） |

**装机后实测（用 `/usr/bin/yshjy` 本身跑，不是仓库里的 `target/` 二进制）**：

- `dpkg -l yshjy` = **0.9.31**、`/usr/bin/yshjy --version` = 0.9.31
- 桌面菜单认领 **14 种格式**；`/usr/share/man/man1/yshjy.1.gz` 与 bash 补全时间戳随之更新
- `bash tests/pick.sh /usr/bin/yshjy` **11 项全绿**（含本轮新增的 Ctrl-C 第 ⑤ 组 5 项）——
  这条是关键：它证明修复真的进了装机版，而不是只在仓库里绿
- 真跑一遍：`ys 源目录 -o 伪装视频.mp4` → `file` 认成 `ISO Media, MP4`、`--list` 列出条目、
  解压回 4 个文件、**逐文件 sha256 与源目录完全一致**

**坑（仍然有效）**：打包要用 `ys` 子命令（`yshjy ys <目录> -o <产物>`），
直接 `yshjy <目录> -o x.mp4` 会被当成「这不是压缩包」（退出码 3）—— 验收脚本第一版就踩了。
顺序仍是先 `release.sh` 再 `build-deb.sh`（或两条都跑完再验包），否则 `test-packages.sh`
会红在「tar.gz 与 deb 不是同一次构建」。

## 0.9.29 发布记录（2026-10-29）

> 这一版把「窗口尺寸记忆 / 原包处置（CLI + 界面）/ 右键脚本只读目录 / packSize 封顶」四块
> 送进装机版。流程与 0.9.26 那轮完全一致（下表就是那轮的快照，数字换成 33 项）：

| 步骤 | 命令 | 结果 |
|---|---|---|
| 五处版本号同步 | 手工改 `Cargo.toml` / `gui/pubspec.yaml` / `docs/PROTOCOL.md` 两份示例 / `CHANGELOG.md` 段落标题，`docs/yshjy.1` 由生成器带出 | 5/5 |
| 界面 release 构建 | `cd gui && flutter build linux --release` | 成功 |
| 出 deb | `bash scripts/build-deb.sh` | `dist/yshjy_0.9.29_amd64.deb` 9.8M，自查 14 种格式通过 |
| 出两档 tar.gz | `bash scripts/release.sh` | `yshjy-` / `yshjy-full-` 各 3.8M |
| 验包 | `bash scripts/test-packages.sh` | **33 项全绿** |
| 装机 | `sudo apt install ./dist/yshjy_0.9.29_amd64.deb` | `dpkg -l yshjy` = 0.9.29 |

**这一轮踩到的（断言又一次立功）**：先出 deb、再单独跑验包时红了一项 ——
「tar.gz 里版本 0.9.28 ≠ deb 版本 0.9.29（两个包不是同一次构建）」。原因是只跑了
`build-deb.sh` 没跑 `release.sh`，`dist/` 里最新的 tar.gz 还是上一版。
**顺序记住：先 `release.sh`（出 tar.gz）再 `build-deb.sh`（出 deb），或者两个都跑完再验包。**

装机后的实测：`--version` = 0.9.29、桌面项认领 **14 种格式**、`--help` 里有
`--trash-source` / `--delete-source`、GUI 二进制里带窗口尺寸记忆的打点（`gui-window.txt`）。

---

# 0.9.26 那轮的逐项核对（历史，留作模板）

> 结论先行：**可以发**。deb 32 项实测全过；配套 tar.gz 与 deb 同版本、同一次构建。
> AppImage **本轮不做**，理由与将来的配方写在最后（§4），决定要做的条件是「要给非 Debian 系的人」。
> 本文只记「发布」这一层的规矩与证据；功能待办仍在 `docs/TODO.md`。

---

## 1. 对外版本号规则（用户 2026-10-28 定）

- **对外就叫 `0.9.26`** —— 在批次号上去掉 `-c`，不另起语义化大号。
- 内部批次号仍等同于版本号（第 N 批 = `0.0.N` 那套临时记法只用于早期，现统一到 `0.9.x`）。
- 发版时**五处必须同时是同一个号**，少一处就会出现「dpkg 说一个号、界面状态栏说另一个号」：

| 位置 | 现值 |
|---|---|
| `Cargo.toml` `version` | 0.9.26 |
| `gui/pubspec.yaml` `version` | 0.9.26 |
| `docs/yshjy.1` 的 `.TH` 行 | yshjy 0.9.26 |
| `docs/PROTOCOL.md` 两份示例的 `engine` | 0.9.26 |
| `CHANGELOG.md` 顶部段落 + `HANDOFF.md` 版本行 | 0.9.26 |

`scripts/build-deb.sh` 自己会在打包前比对前两处，不一致直接拒绝；`scripts/test-packages.sh`
会在打完后比对「包内引擎自称的版本」与「包版本」，以及 tar.gz 与 deb 是否同版本
（这条断言就是 0.9.26 这轮加的 —— 它当场抓到 `dist/` 里躺着的 tar.gz 还是 0.9.23-c）。

## 2. deb 逐项核对（0.9.26 实测，32 项通过 / 0 失败）

跑：`bash scripts/test-packages.sh`（自动取 `dist/` 里最新的 deb 与 tar.gz，不再写死版本号）。

| 组 | 查什么 | 结果 |
|---|---|---|
| 元数据 | Package / Version / Architecture / Depends / Installed-Size / Maintainer 齐全；包名 `yshjy`；架构与 `dpkg --print-architecture` 一致；Depends 含 `libc6` 与 `libgtk-3` | 全过 |
| 文件表 | CLI、GUI 启动器、`libapp.so`、`icudtl.dat`、桌面项、图标、man、bash/zsh/fish 三种补全、Nautilus 右键脚本 | 11/11 在 |
| 属性 | 全部 `root/root`（`dpkg-deb --root-owner-group`）；man 页 644 | 全过 |
| 依赖是否真够 | `libgtk-3-0t64` 递归闭包 279 个包 vs GUI 实测 70 个 `.so` 逐个反查归属 → **无缺口**（唯一报警的 `libdl.so.2` 是 dpkg 把 `libc6-i386` 的 32 位副本认错了，干净机器不会缺） | 够用，不用补 |
| 功能 | 用包里的二进制真打包成伪装视频 → 真解回来 → `cmp` 逐字节一致 | 全过 |
| tar.gz | sha256 校验；解包后二进制可运行；**版本与 deb 一致** | 全过 |

## 3. 这轮修掉的两个真事故（都固化成断言了，别再犯）

### ① 装完 deb，双击压缩包还是被 Nautilus 抢走

`postinst` 里本想给装包的人设默认程序，写法是：

```sh
su -s /bin/sh "$SUDO_USER" -c "xdg-mime default yshjy.desktop application/zip"
```

**不带横杠的 `su` 会继承 root 的 `HOME`**，于是 `xdg-mime` 把默认项写进了
`/root/.config/mimeapps.list` —— 用户账号里一条都没写成。症状：装完一切正常，
只有「双击 zip」这条新手最常走的路没生效。

修法（已落进 `scripts/build-deb.sh`）：先 `getent passwd "$SUDO_USER"` 取真实家目录，
再在 `su -c` 里显式设 `HOME=` 与 `XDG_CONFIG_HOME=`。
断言：`test-packages.sh` 检查 postinst 里只要有 `xdg-mime default` 就必须同时出现 `HOME=`。

### ② 装机版桌面项只认领 7 种格式（源码早已 14 种）

0.9.25-c 的包里 `yshjy.desktop` 只有 7 个 `MimeType`，而 `gui/linux/yshjy.desktop`
早就是 14 个 —— 说明那个 deb 是拿旧 desktop 打的，多出来的 7 种（zip-compressed /
x-rar / x-rar-compressed / x-tar / x-compressed-tar / x-brotli / x-lz4）用户根本摸不到。

修法：`build-deb.sh` 自查里加死断言 —— 包内桌面项的 `MimeType` 计数必须等于 14。
数不对就拒绝出包（宁可不发，也不发一个「少认领一半格式」的包）。

顺带修的两处小账：man 页打进包是 `664`（gzip 重定向跟 umask 走）→ 显式 `chmod 644`；
`scripts/test-packages.sh` 原来把版本写死 `0.9.21-c`（跑起来验的是三个月前的老包）→ 改成自动取最新产物。

## 4. AppImage 评估：本轮不做（附将来要做的完整配方）

调研来源（不是凭空判断）：
- KDAB / Industrial Flutter《Portability: A Case Study in Flutter & AppImage Distribution》(2026-07) ——
  Flutter + GTK3 做 AppImage 的真实踩坑记录；
- AppImage 官方文档「upstream packaging」；
- `appimage-builder` / `linuxdeploy` + `linuxdeploy-plugin-gtk` / `flutter_distributor` 三条路线。

### 结论

**现在不做。** 不是「做不出来」，是**在当前这台机器上做出来的 AppImage 基本没人能跑**：

- **glibc 是硬门槛**：glibc 向后兼容、不向前兼容。本机是 Debian 13 trixie（glibc 2.41），
  在这里编出来的 AppImage 要求宿主 glibc ≥ 2.41 —— Fedora/Ubuntu LTS/老 Debian 全都跑不了。
  要做就得**在容器里用老基线构建**（Debian 12 bookworm，glibc 2.36），这是另一整轮工程，不是半天的活。
- **图形栈必须排除，不能整包**：KDAB 那篇的原话是整包彩色库会让宿主驱动和捆绑的 GL/EGL 打架
  （`Could not make the context current`）。必须 bundle GTK3 + gdk-pixbuf loaders，
  同时排除 `libGL* / libEGL* / libGLES* / libgbm* / libdrm* / libwayland-*`
  以及 `libc6 / libstdc++6 / libgcc-s1 / zlib1g`；还要在 AppRun 里清掉 `GTK_MODULES`。
- **体积**：我们的 GUI bundle 已经 25 MB（含 `libflutter_linux_gtk.so` 17 MB），
  再捆 GTK3 栈约 30–50 MB → AppImage 约 60–80 MB，比 deb（9.7 MB，用宿主 GTK）大 7 倍。
- **收益对不上**：用户和朋友的机器都是 Debian/Ubuntu 系，`sudo apt install ./yshjy_*.deb` 一步到位，
  还自带菜单项/图标/右键脚本/默认打开方式；AppImage 免安装这点收益，抵不过上面三条代价。

### 将来真要做（配方，别重新调研）

1. 基线容器：`docker run --rm -v $PWD:/w -w /w debian:12`（glibc 2.36），容器里装 Rust + Flutter SDK + GTK 开发包。
2. 构建：容器内 `cargo build --release` + `flutter build linux --release`。
3. 打包：优先 `linuxdeploy` + `linuxdeploy-plugin-gtk`（它专门处理 GTK 主题/loaders），
   或 `appimage-builder`（用 apt 解析依赖，配方里显式 `exclude` 上面的图形栈与工具链）。
4. 运行时：用**静态 runtime** 的 `appimagetool`（老的依赖宿主 `libfuse2`，新发行版默认没有；
   实在没有 FUSE 就 `APPIMAGE_EXTRACT_AND_RUN=1`）。
5. 验收：至少在 5 个发行版上各跑一次（Debian stable/oldstable、Ubuntu LTS 前后两版、Fedora）——
   只在 build 机器上点开不算通过。

## 5. 发版这一步的命令（照抄即可）

```bash
cd ~/hermes/yshjy
bash tests/all.sh                    # 四道闸：编译 / clippy / 单测 / 协议探针 / 界面 / 冒烟
bash scripts/build-deb.sh            # 出 dist/yshjy_<版本>_amd64.deb（自带三条新断言）
bash scripts/release.sh              # 出 dist/yshjy-<版本>-linux-x86_64.tar.gz 两档位 + sha256
bash scripts/test-packages.sh        # 发布前最后一道：32 项包体检
```

安装/回滚：

```bash
sudo apt install ./dist/yshjy_0.9.26_amd64.deb     # 装
sudo apt install --reinstall ./dist/yshjy_0.9.25-c_amd64.deb   # 回滚到上一版
sudo apt remove yshjy                              # 卸载（不留垃圾：包里没有 conffiles、不写用户家目录）
```

安装后**验三件事**（这三件正是新手会撞上的）：① 应用菜单里能找到「杨三海解压」
② 双击一个 zip 打开的是我们（`xdg-mime query default application/zip` → `yshjy.desktop`）
③ `yshjy --version` 与菜单里「关于」页显示的号一致。

---

## 6. 0.9.26 真实装机验收（2026-10-28，用户亲手装的）

出包 → 装包 → 装机后实测，全部通过。这一节是**装机态**证据，不是「在 build 目录里点的」。

| 查什么 | 命令 | 结果 |
|---|---|---|
| 装的版本 | `dpkg -l yshjy` | `ii  yshjy  0.9.26` ✅ |
| 终端敲到的是哪个 | `command -v yshjy` → `~/.local/bin/yshjy` → 软链 `/usr/bin/yshjy` | 0.9.26（没踩「旧手工安装抢先」的坑）✅ |
| 系统级引擎 | `/usr/bin/yshjy --version` | 0.9.26 ✅ |
| **双击归谁** | `xdg-mime query default` 逐个查 14 种格式 | **14/14 全是 `yshjy.desktop`**（装之前 14 种全被 Nautilus 抢）✅ |
| postinst 有没有写对你的账号 | `~/.config/mimeapps.list` | 14 条全指向 `yshjy.desktop`，时间戳是装包那一刻 ✅ |
| 桌面项认领格式数 | 系统级 `yshjy.desktop` | 14 种 ✅ |
| 界面能不能起 | `timeout 6 /usr/lib/yshjy/gui/yshjy_gui` | 正常起，只有两条无害警告（光标主题、appindicator 弃用），无新崩溃日志 ✅ |
| **双击真实行为** | 造一个 zip → `gio open 测试压缩包.zip` | 拉起 `/usr/lib/yshjy/gui/yshjy_gui <该 zip 路径>` ✅ |

### 遗留：用户级旧桌面项（7 种格式）还在

`~/.local/share/applications/yshjy.desktop`（2026-10-24 由 `scripts/install-gui.sh` 留下）
仍在，只认领 7 种格式。`mimeapps.list` 的 `[Default Applications]` 优先级更高，所以双击已归我们；
但文件管理器「打开方式」列表等多处会读到用户级这份，看起来像「只支持 7 种」。
**处理建议：删掉这份用户级副本**（`Exec` 本来就指系统级路径，删掉后完全由 deb 里的那份接管）。
删之前要问用户 —— 属于「删用户的文件」，按项目铁律不擅自动手。
