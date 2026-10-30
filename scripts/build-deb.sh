#!/usr/bin/env bash
# 打 .deb 安装包（不需要 sudo）
#
# 用法: bash scripts/build-deb.sh
#
# 前置：
#   1) 引擎：bash scripts/release.sh 或 cargo build --release
#   2) 界面：cd gui && ~/flutter/bin/flutter build linux --release
#
# 产物：dist/yshjy_<版本>_<架构>.deb
#   含：命令行 yshjy + 图形界面 + 应用菜单项 + 图标 + 补全 + man + 文档
#
# 为什么不交给 cargo-deb：那是个额外的第三方工具，而 dpkg-deb 是系统自带的。
# 一个脚本就能说清楚包里到底装了什么 —— 可审计。

set -euo pipefail
cd "$(dirname "$0")/.."

# ── 补全与 man 页：打包前从 CLI 选项表重新生成 ──
# 手写副本必然漂移（2026-10-28 实测：补全漏 17 个选项、man 漏 17 个），
# 所以出包前一律重新生成，不做人工核对。做法见 docs/CLI-GUI-PLAN.md §1。
if [ -x target/release/yshjy ]; then
    python3 scripts/gen-cli-assets.py
else
    echo "⚠️  没有 target/release/yshjy，跳过补全/man 重新生成（沿用现有生成物）"
fi

# ── 版本一致性（PRD 验收标准 3/4）──
V_CARGO=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
V_GUI=$(grep -m1 '^version:' gui/pubspec.yaml | awk '{print $2}')
if [ "$V_CARGO" != "$V_GUI" ]; then
  echo "!! 版本不一致，拒绝打包：" >&2
  echo "   Cargo.toml        = $V_CARGO" >&2
  echo "   gui/pubspec.yaml  = $V_GUI" >&2
  echo "   先把两处改成同一个版本（批次号=版本号，内测加 -c）" >&2
  exit 1
fi
V="$V_CARGO"
ARCH=$(dpkg --print-architecture)
STAGE=$(mktemp -d)

cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

# ── 前置检查 ──
[ -x target/release/yshjy ] || {
  echo "!! 找不到 target/release/yshjy —— 先跑：cargo build --release" >&2
  exit 1
}
BUNDLE="gui/build/linux/x64/release/bundle"
[ -x "$BUNDLE/yshjy_gui" ] || {
  echo "!! 找不到图形界面构建产物：$BUNDLE/yshjy_gui" >&2
  echo "   先跑：cd gui && ~/flutter/bin/flutter build linux --release" >&2
  exit 1
}
[ -f gui/linux/yshjy.desktop ] || { echo "!! 缺 gui/linux/yshjy.desktop" >&2; exit 1; }
[ -f gui/linux/yshjy.png ] || { echo "!! 缺 gui/linux/yshjy.png" >&2; exit 1; }

echo "==> 组装 yshjy $V（$ARCH）"

# ── 目录结构（FHS）──
mkdir -p "$STAGE/DEBIAN"
mkdir -p "$STAGE/usr/bin"
mkdir -p "$STAGE/usr/lib/yshjy"
mkdir -p "$STAGE/usr/share/applications"
mkdir -p "$STAGE/usr/share/icons/hicolor"
mkdir -p "$STAGE/usr/share/metainfo"
mkdir -p "$STAGE/usr/share/man/man1"
mkdir -p "$STAGE/usr/share/bash-completion/completions"
mkdir -p "$STAGE/usr/share/zsh/vendor-completions"
mkdir -p "$STAGE/usr/share/fish/vendor_completions.d"
mkdir -p "$STAGE/usr/share/doc/yshjy"
mkdir -p "$STAGE/usr/share/nautilus/scripts"

# ── 程序 ──
install -m755 target/release/yshjy "$STAGE/usr/bin/yshjy"
ln -s yshjy "$STAGE/usr/bin/yshys"   # 打包模式的别名（同一个程序，看名字决定默认行为）

# ── 图形界面（不含引擎副本：统一用 /usr/bin/yshjy，避免两份版本打架）──
echo "    → 图形界面 $(du -sh "$BUNDLE" | cut -f1)（去掉引擎副本）"
cp -r "$BUNDLE"/. "$STAGE/usr/lib/yshjy/gui/"
rm -f "$STAGE/usr/lib/yshjy/gui/yshjy"

# ── 桌面集成：菜单项 + 图标 ──
sed "s|__EXEC__|/usr/lib/yshjy/gui/yshjy_gui|" gui/linux/yshjy.desktop \
  > "$STAGE/usr/share/applications/yshjy.desktop"
chmod 644 "$STAGE/usr/share/applications/yshjy.desktop"
# 多尺寸图标（只有一个 256 时，GNOME 缩到 16/24 会糊）
cp -a gui/linux/icons/hicolor/. "$STAGE/usr/share/icons/hicolor/"
# AppStream 元数据：软件中心靠它显示名称/简介/版本说明
install -m644 data/io.github.yangsanhai.yshjy.metainfo.xml \
  "$STAGE/usr/share/metainfo/io.github.yangsanhai.yshjy.metainfo.xml"

# ── 文件管理器右键菜单（N7）──
# 在 Nautilus 里选中压缩包 → 右键 →「脚本」→「用杨三海解压」
if [ -f gui/linux/nautilus-script-yshjy ]; then
  install -m755 gui/linux/nautilus-script-yshjy \
    "$STAGE/usr/share/nautilus/scripts/用杨三海解压"
else
  echo "!! 缺 gui/linux/nautilus-script-yshjy（右键菜单会缺）" >&2
fi

# ── 安装/卸载钩子：刷新菜单与图标缓存（否则要注销重登才看到）──
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || true
fi
# 让「双击压缩包」真的打开我们 —— 光有 MimeType 不够：GNOME 里文件管理器
# （Nautilus）默认就抢着处理 zip，用户装完还是双击出来一堆文件。
# 只给**装包这个人**设默认（$SUDO_USER），不动其他用户，也不用 system 级默认。
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ] \
   && command -v xdg-mime >/dev/null 2>&1; then
  # 注意 HOME：`su -s /bin/sh user -c …`（不带横杠）会继承 root 的 HOME，
  # xdg-mime 于是把默认项写进 /root/.config/mimeapps.list —— 用户账号里一条都没写成，
  # 装完双击压缩包还是 Nautilus。所以 HOME / XDG_CONFIG_HOME 必须显式指回用户家目录。
  UHOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
  if [ -n "$UHOME" ] && [ -d "$UHOME" ]; then
    for m in application/zip application/x-zip-compressed \
             application/x-7z-compressed application/vnd.rar \
             application/x-rar application/x-rar-compressed \
             application/x-tar application/x-compressed-tar \
             application/gzip application/x-xz application/zstd \
             application/x-bzip2 application/x-brotli application/x-lz4; do
      su -s /bin/sh "$SUDO_USER" -c \
        "mkdir -p '$UHOME/.config' && HOME='$UHOME' XDG_CONFIG_HOME='$UHOME/.config' xdg-mime default yshjy.desktop '$m'" \
        2>/dev/null || true
    done
  fi
fi
exit 0
EOF
cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
exit 0
EOF
# ── 卸载前钩子（2026-10-29 补：此前只有 postinst + postrm，卸载后托盘里会挂着
#    一个「已被删掉的程序」，用户配置里也留着一堆指向它的「双击默认打开」条目）──
cat > "$STAGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
# 只在**真要卸载**时动手。升级（dpkg 传的是 `upgrade <新版本>`）什么都不做 ——
# 用户可能正让它在解一个 8 GB 的包，升级把他的活杀掉是不可接受的。
case "${1:-}" in
  remove|purge) ;;
  *) exit 0 ;;
esac

# ① 停掉还在跑的界面：它「关窗口 = 藏到托盘」，卸载后托盘里会留一个已被删掉的进程。
#    用 TERM 不用 KILL —— 强杀会让正在解压的包留下 .yshjy-part；
#    卸载软件的副作用不该包括「砸坏他正在做的事」。
if command -v pkill >/dev/null 2>&1; then
  pkill -TERM -x yshjy_gui 2>/dev/null || true
fi

# ② 清掉 postinst 设过的「双击压缩包用我打开」。不清的话用户配置里会留一堆
#    指向已卸载程序的条目，重装/换路径时会莫名其妙。改文件前先备份；
#    改不动就算（卸载本身不该因为这个失败）。
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
  UHOME=$(getent passwd "$SUDO_USER" 2>/dev/null | cut -d: -f6)
  if [ -n "$UHOME" ] && [ -d "$UHOME" ]; then
    for f in "$UHOME/.config/mimeapps.list" "$UHOME/.local/share/applications/mimeapps.list"; do
      [ -f "$f" ] || continue
      grep -q 'yshjy\.desktop' "$f" 2>/dev/null || continue
      cp -p "$f" "$f.yshjy-uninstall.bak" 2>/dev/null || true
      sed -i '/yshjy\.desktop/d' "$f" 2>/dev/null || true
    done
  fi
fi
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm" "$STAGE/DEBIAN/prerm"

# ── 手册与补全 ──
gzip -9c docs/yshjy.1 > "$STAGE/usr/share/man/man1/yshjy.1.gz"
# gzip 重定向出来的文件跟着 shell 的 umask 走，实测打进去是 664（组可写），
# Debian 规范要求 644 —— 显式改一次，别指望 umask。
chmod 644 "$STAGE/usr/share/man/man1/yshjy.1.gz"
install -m644 scripts/completions/yshjy.bash "$STAGE/usr/share/bash-completion/completions/yshjy"
install -m644 scripts/completions/_yshjy     "$STAGE/usr/share/zsh/vendor-completions/_yshjy"
install -m644 scripts/completions/yshjy.fish "$STAGE/usr/share/fish/vendor_completions.d/yshjy.fish"

# ── 文档 ──
for f in LICENSE README.md CHANGELOG.md CONTRIBUTING.md; do
  [ -f "$f" ] && install -m644 "$f" "$STAGE/usr/share/doc/yshjy/$f"
done

# ── 控制文件 ──
INSTALLED_SIZE=$(du -sk "$STAGE" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<EOF
Package: yshjy
Version: $V
Section: utils
Priority: optional
Architecture: $ARCH
Installed-Size: $INSTALLED_SIZE
Depends: libc6 (>= 2.34), libgtk-3-0 | libgtk-3-0t64
Recommends: libnotify-bin, desktop-file-utils
Maintainer: yangsanhai <1848409757@qq.com>
Homepage: https://github.com/yangsanhai/yshjy
Description: 杨三海解压 —— 还原伪装成视频/图片的压缩包（含图形界面）
 网盘下载的 .mp4.zip 常常"所有工具都打不开"，因为真归档藏在视频数据中间。
 yshjy 自己定位归档起点、再交给成熟的解码库解压，支持 ZIP/ZIP64/7z/RAR/TAR
 以及 gzip/xz/zstd/bzip2/brotli/lz4 单流格式，含分卷拼接与中文密码。
 反向也能把目录打包成能播放的伪装视频/图片。不联网、不做字典爆破。
 安装后在应用菜单里点「杨三海解压」即可使用图形界面。
EOF

# 版权文件（Debian 政策要求可选的 copyright；有 LICENSE 就顺手放一份）
if [ -f LICENSE ]; then
  cp LICENSE "$STAGE/usr/share/doc/yshjy/copyright"
fi

# ── 打包前自检：desktop 里不能留占位符 ──
if grep -q '__EXEC__' "$STAGE/usr/share/applications/yshjy.desktop"; then
  echo "!! desktop 文件里的 __EXEC__ 没替换掉" >&2
  exit 1
fi

mkdir -p dist
OUT="dist/yshjy_${V}_${ARCH}.deb"
if [ -e "$OUT" ]; then
  echo "!! $OUT 已存在 —— 脚本不替你删，请自己确认怎么处理" >&2
  exit 1
fi

dpkg-deb --build --root-owner-group "$STAGE" "$OUT" >/dev/null
echo "    → $OUT（$(du -h "$OUT" | cut -f1)）"

# ── 出包后自查（PRD 验收标准 1/3）──
LIST=$(dpkg-deb -c "$OUT" | awk '{print $6}' | sed 's|^\./||')
fail=0
for f in \
  usr/bin/yshjy \
  usr/bin/yshys \
  usr/lib/yshjy/gui/yshjy_gui \
  usr/lib/yshjy/gui/lib/libapp.so \
  usr/lib/yshjy/gui/data/icudtl.dat \
  usr/share/applications/yshjy.desktop \
  usr/share/icons/hicolor/16x16/apps/yshjy.png \
  usr/share/icons/hicolor/24x24/apps/yshjy.png \
  usr/share/icons/hicolor/32x32/apps/yshjy.png \
  usr/share/icons/hicolor/48x48/apps/yshjy.png \
  usr/share/icons/hicolor/64x64/apps/yshjy.png \
  usr/share/icons/hicolor/128x128/apps/yshjy.png \
  usr/share/icons/hicolor/256x256/apps/yshjy.png \
  usr/share/metainfo/io.github.yangsanhai.yshjy.metainfo.xml \
  usr/share/man/man1/yshjy.1.gz \
  usr/share/bash-completion/completions/yshjy \
  usr/share/zsh/vendor-completions/_yshjy \
  usr/share/fish/vendor_completions.d/yshjy.fish \
  usr/share/nautilus/scripts/用杨三海解压; do
  if ! printf '%s\n' "$LIST" | grep -qx "$f"; then
    echo "!! 包里缺：$f" >&2
    fail=1
  fi
done
PKG_V=$(dpkg-deb -f "$OUT" Version)
[ "$PKG_V" = "$V" ] || { echo "!! 包版本 $PKG_V ≠ 代码版本 $V" >&2; fail=1; }

# ① 包里的桌面项必须是 14 种格式（0.9.25-c 就是打了旧 desktop、只 7 种，装完双击仍归 Nautilus）
MIME_N=$( (dpkg-deb --fsys-tarfile "$OUT" | tar -xO ./usr/share/applications/yshjy.desktop \
         2>/dev/null | grep '^MimeType' | tr ';' '\n' | sed '/^$/d' | wc -l) || echo 0 )
[ "$MIME_N" = "14" ] || { echo "!! 桌面项只认领 $MIME_N 种格式（应为 14）—— 是不是打了旧 desktop？" >&2; fail=1; }

# ② 包内引擎自称的版本必须跟包版本一致（否则界面状态栏和 dpkg 说的不是一个号）
TMPX=$(mktemp -d); trap 'rm -rf "$STAGE" "$TMPX"' EXIT
dpkg-deb -x "$OUT" "$TMPX"
# 注意：--version 的输出是「yshjy 0.9.26（常用版）—— …」，版本号后面紧跟中文括号，
# awk '{print $2}' 会把「0.9.26（常用版）」整块取出来 —— 必须用正则抠版本号本身。
ENG_V=$( ("$TMPX/usr/bin/yshjy" --version 2>/dev/null | head -1 \
          | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.-]*' | head -1) || echo "<跑不起来>" )
[ "$ENG_V" = "$V" ] || { echo "!! 包内引擎自称 $ENG_V ≠ 包版本 $V（是不是拿了旧二进制？）" >&2; fail=1; }

# ③ 安装钩子里不许出现「不带 HOME 的 su + xdg-mime」（那个组合会把默认项写进 /root）
if dpkg-deb -e "$OUT" "$TMPX/DEBIAN" 2>/dev/null || dpkg-deb --control "$OUT" "$TMPX/DEBIAN" 2>/dev/null; then
  if grep -q 'xdg-mime default' "$TMPX/DEBIAN/postinst" 2>/dev/null \
     && ! grep -q "HOME=" "$TMPX/DEBIAN/postinst" 2>/dev/null; then
    echo "!! postinst 里的 xdg-mime 没带 HOME——会写进 /root/.config/mimeapps.list" >&2
    fail=1
  fi
fi
# ④ 卸载钩子必须在，且**升级分支不许杀进程**（否则「升级」会干掉用户正在跑的解压）
if [ ! -f "$TMPX/DEBIAN/prerm" ]; then
  echo "!! 包里没有 prerm（卸载后托盘残留 + 用户配置留垃圾条目）" >&2; fail=1
else
  [ "$(stat -c %a "$TMPX/DEBIAN/prerm")" = "755" ] \
    || { echo "!! prerm 权限不是 755" >&2; fail=1; }
  sh -n "$TMPX/DEBIAN/prerm" 2>/dev/null \
    || { echo "!! prerm 语法错误" >&2; fail=1; }
  grep -q 'upgrade' "$TMPX/DEBIAN/prerm" \
    || { echo "!! prerm 没区分 upgrade —— 升级会把用户正在跑的解压杀掉" >&2; fail=1; }
fi
[ "$fail" -eq 0 ] || { echo "!! 自查未通过" >&2; exit 1; }

echo "   ✅ 自查通过：CLI + GUI + 菜单项(14 种格式) + 图标 + man + 三种补全 + 右键脚本，版本 $PKG_V"
echo
echo "安装： sudo apt install ./$OUT"
echo "卸载： sudo apt remove yshjy"
