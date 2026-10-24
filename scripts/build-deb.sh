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
mkdir -p "$STAGE/usr/share/icons/hicolor/256x256/apps"
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
install -m644 gui/linux/yshjy.png "$STAGE/usr/share/icons/hicolor/256x256/apps/yshjy.png"

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
  for m in application/zip application/x-zip-compressed \
           application/x-7z-compressed application/vnd.rar \
           application/x-rar application/x-rar-compressed \
           application/x-tar application/x-compressed-tar \
           application/gzip application/x-xz application/zstd \
           application/x-bzip2 application/x-brotli application/x-lz4; do
    su -s /bin/sh "$SUDO_USER" -c "xdg-mime default yshjy.desktop $m" 2>/dev/null || true
  done
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
chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"

# ── 手册与补全 ──
gzip -9c docs/yshjy.1 > "$STAGE/usr/share/man/man1/yshjy.1.gz"
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
  usr/lib/yshjy/gui/yshjy_gui \
  usr/lib/yshjy/gui/lib/libapp.so \
  usr/share/applications/yshjy.desktop \
  usr/share/icons/hicolor/256x256/apps/yshjy.png; do
  if ! printf '%s\n' "$LIST" | grep -qx "$f"; then
    echo "!! 包里缺：$f" >&2
    fail=1
  fi
done
PKG_V=$(dpkg-deb -f "$OUT" Version)
[ "$PKG_V" = "$V" ] || { echo "!! 包版本 $PKG_V ≠ 代码版本 $V" >&2; fail=1; }
[ "$fail" -eq 0 ] || { echo "!! 自查未通过" >&2; exit 1; }

echo "   ✅ 自查通过：CLI + GUI + 菜单项 + 图标 + 版本 $PKG_V 全在"
echo
echo "安装： sudo apt install ./$OUT"
echo "卸载： sudo apt remove yshjy"
