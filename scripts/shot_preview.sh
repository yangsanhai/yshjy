#!/usr/bin/env bash
# 离屏截图：在 Xvfb 虚拟屏幕上跑界面，截首页预览。
# 用法：bash shot_preview.sh <语言代码> <输出图片路径>
set -uo pipefail

LANG_CODE="${1:-zh-CN}"
OUT="${2:-/tmp/shots/preview.png}"
GUI="$HOME/hermes/yshjy/gui/build/linux/x64/release/bundle/yshjy_gui"
BIN="$HOME/hermes/yshjy/target/release/yshjy"

mkdir -p "$(dirname "$OUT")"

# 1) 造样例：一个目录 + 几个文件，打成伪装包
WORK=$(mktemp -d)
mkdir -p "$WORK/示例游戏/游戏数据" "$WORK/示例游戏/补丁"
head -c 300000 /dev/urandom > "$WORK/示例游戏/游戏数据/资源包.bin"
head -c 120000 /dev/urandom > "$WORK/示例游戏/游戏数据/立绘 01.dat"
printf '说明文件\n' > "$WORK/示例游戏/说明.txt"
printf 'patch\n' > "$WORK/示例游戏/补丁/hotfix.dat"
( cd "$WORK" && "$BIN" ys 示例游戏 >/dev/null 2>&1 )
PKG=$(ls "$WORK"/示例游戏.mp4 2>/dev/null | head -1)
if [ -z "$PKG" ]; then
  echo "!! 没造出样例包"
  ls -la "$WORK"
  exit 1
fi

# 2) 隔离配置（绝不碰用户真实 ~/.config/yshjy）
CFG=$(mktemp -d); STA=$(mktemp -d)
mkdir -p "$CFG/yshjy" "$STA/yshjy"
printf '[general]\nui_language = "%s"\n' "$LANG_CODE" > "$CFG/yshjy/config.toml"
# 跳过首次打开的三步引导（不然截图里会盖一个弹窗）
: > "$STA/yshjy/onboarding_done"
# 预置窗口尺寸（GUI 会读 state 目录里的 gui-window.txt：宽 高 [是否最大化]）
printf '1100 760 0' > "$STA/yshjy/gui-window.txt"
# 让界面用**刚编的这份引擎**（bundle 里没有引擎时它会退回系统 PATH 上装着的旧版）
cp -f "$BIN" "$(dirname "$GUI")/yshjy"

# 3) 起虚拟屏幕
DISP_NUM=99
Xvfb :$DISP_NUM -screen 0 1240x880x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
XPID=$!
sleep 3
export DISPLAY=:$DISP_NUM
export LIBGL_ALWAYS_SOFTWARE=1
export GDK_BACKEND=x11
export XDG_CONFIG_HOME="$CFG" XDG_STATE_HOME="$STA"

# 4) 起界面（带包，直接进预览页）
"$GUI" "$PKG" > /tmp/gui.log 2>&1 &
GPID=$!
sleep 12

# 打印窗口几何（没有窗口管理器时 GTK 拿不到工作区，尺寸可能和预期不同）
xwininfo -display :$DISP_NUM -root -tree 2>/dev/null | grep -iE "杨三海|yshjy" | head -5

# 5) 截图（只截应用窗口，不要虚拟桌面的黑边）
WID=$(xwininfo -display :$DISP_NUM -root -tree 2>/dev/null | grep -i "杨三海解压" | head -1 | awk '{print $1}')
import -display :$DISP_NUM -window "${WID:-root}" "$OUT" 2>/tmp/import.log
RC=$?

kill $GPID 2>/dev/null
sleep 1
kill $XPID 2>/dev/null
wait 2>/dev/null

if [ $RC -eq 0 ] && [ -s "$OUT" ]; then
  echo "OK: $OUT ($(stat -c%s "$OUT") 字节)"
else
  echo "!! 截图失败 rc=$RC"
  echo "--- gui.log ---"; tail -20 /tmp/gui.log
  echo "--- import.log ---"; tail -5 /tmp/import.log
fi
echo "WORK=$WORK CFG=$CFG"
