# 发布检查（T5）—— 0.9.26 对外发布前的逐项核对

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
