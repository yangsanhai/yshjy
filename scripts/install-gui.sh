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
ICON_ROOT="$HOME/.local/share/icons/hicolor"
APPDATA="$HOME/.local/share/metainfo"

if [ "${1:-}" = "--uninstall" ]; then
  rm -rf "$DEST"
  rm -f "$APPS/yshjy.desktop"
  for n in 16 24 32 48 64 128 256; do rm -f "$ICON_ROOT/${n}x${n}/apps/yshjy.png"; done
  rm -f "$APPDATA/io.github.yangsanhai.yshjy.metainfo.xml"
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

# 引擎：**永远用 target/release 的最新构建**覆盖
#
# 坑：bundle 里的引擎是 `flutter build` 当时拷进去的快照，之后 cargo 改了代码
# 它不会自动更新 —— 结果界面是新写的、引擎是旧的，协议命令对不上（BAD_PARAM/UNKNOWN_COMMAND）。
if [ -x target/release/yshjy ]; then
  cp target/release/yshjy "$DEST/"
else
  cargo build --release && cp target/release/yshjy "$DEST/"
fi
chmod 755 "$DEST/yshjy_gui" "$DEST/yshjy"

echo "==> 应用菜单"
mkdir -p "$APPS"
sed "s|__EXEC__|$DEST/yshjy_gui|" gui/linux/yshjy.desktop > "$APPS/yshjy.desktop"
chmod 644 "$APPS/yshjy.desktop"

echo "==> 图标"
mkdir -p "$ICON_ROOT" "$APPDATA"
cp -a gui/linux/icons/hicolor/. "$ICON_ROOT/"
install -m644 data/io.github.yangsanhai.yshjy.metainfo.xml "$APPDATA/"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

# 双击压缩包打开我们 —— 光有 desktop 不够：GNOME 的文件管理器默认就抢着处理 zip，
# 装完双击出来的是「解压到文件夹」，不是我们的界面。
# 这个脚本会写 ~/.config/mimeapps.list；想还原跑 scripts/set-default-mime.sh --restore
echo "==> 默认打开方式"
bash scripts/set-default-mime.sh 2>/dev/null | head -1 || true

echo
echo "  ✅ 装好了：应用菜单里搜「杨三海解压」"
echo "     也可以直接运行：$DEST/yshjy_gui 文件路径"
