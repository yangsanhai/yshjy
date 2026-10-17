#!/usr/bin/env bash
# 把图形界面装进用户目录 + 应用菜单（**不需要 sudo**）
#
# 用法： bash scripts/install-gui.sh
# 卸载： bash scripts/install-gui.sh --uninstall
#
# 装完效果：应用菜单里出现「杨三海解压」，点开就能用；
#          在文件管理器里右键 .zip/.mp4，可以选「用杨三海解压打开」。

set -euo pipefail
cd "$(dirname "$0")/.."

DEST="$HOME/.local/opt/yshjy"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"

if [ "${1:-}" = "--uninstall" ]; then
  rm -rf "$DEST"
  rm -f "$APPS/yshjy.desktop" "$ICONS/yshjy.png"
  update-desktop-database "$APPS" 2>/dev/null || true
  gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
  echo "  已卸载（程序目录、菜单项、图标都清掉了）"
  exit 0
fi

BUNDLE="gui/build/linux/x64/release/bundle"
[ -x "$BUNDLE/yshjy_gui" ] || {
  echo "!! 没找到 GUI 构建产物：$BUNDLE" >&2
  echo "   先编译：cd gui && flutter build linux --release" >&2
  exit 1
}

echo "==> 安装到 $DEST"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -r "$BUNDLE"/. "$DEST"/

# 引擎：优先用 bundle 里的，没有就从 target/ 补
if [ ! -x "$DEST/yshjy" ]; then
  [ -x target/release/yshjy ] || cargo build --release
  cp target/release/yshjy "$DEST/"
fi
chmod 755 "$DEST/yshjy_gui" "$DEST/yshjy"

echo "==> 应用菜单"
mkdir -p "$APPS"
sed "s|__EXEC__|$DEST/yshjy_gui|" gui/linux/yshjy.desktop > "$APPS/yshjy.desktop"
chmod 644 "$APPS/yshjy.desktop"

echo "==> 图标"
mkdir -p "$ICONS"
cp gui/linux/yshjy.png "$ICONS/yshjy.png"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo
echo "  ✅ 装好了：应用菜单里搜「杨三海解压」"
echo "     也可以直接运行：$DEST/yshjy_gui 文件路径"
