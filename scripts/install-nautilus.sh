#!/usr/bin/env bash
# 把「用杨三海解压」装进文件管理器右键菜单（用户级，不用 sudo）
#
#   bash scripts/install-nautilus.sh            # 安装 / 更新
#   bash scripts/install-nautilus.sh --remove   # 卸掉
#
# 装完要**重启文件管理器**才看得到（脚本会提示）。Nautilus 会在
# 「右键 → 脚本」子菜单里列出它。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

SRC="gui/linux/nautilus-script-yshjy"
[ -f "$SRC" ] || { echo "!! 缺 $SRC" >&2; exit 1; }

NAME="用杨三海解压"
DEST_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/nautilus/scripts"
DEST="$DEST_DIR/$NAME"

if [ "${1:-}" = "--remove" ]; then
  if [ -f "$DEST" ]; then
    command rm -f "$DEST"
    echo "已卸载：$DEST"
  else
    echo "本来就没装：$DEST"
  fi
  echo "重启文件管理器后生效：nautilus -q"
  exit 0
fi

mkdir -p "$DEST_DIR"
cp "$SRC" "$DEST"
chmod 755 "$DEST"
echo "已安装：$DEST"
echo
echo "用法：在文件管理器里选中压缩包 → 右键 →「脚本」→「$NAME」"
echo "让菜单立刻出现：nautilus -q（关掉文件管理器，下次打开就有了）"
