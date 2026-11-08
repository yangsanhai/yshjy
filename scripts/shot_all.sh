#!/usr/bin/env bash
# 离屏截一整套界面图（4 张）：预览 / 打包 / 设置 / 完成
#
# 用法：bash scripts/shot_all.sh <语言代码> <输出目录>
#   例：bash scripts/shot_all.sh zh-CN /tmp/shots/zh
#       bash scripts/shot_all.sh en-US /tmp/shots/en
#
# 全程在 Xvfb 虚拟屏幕上跑：不弹用户桌面、不碰用户真实配置（用临时 XDG_* 目录）。
set -uo pipefail

LANG_CODE="${1:-zh-CN}"
OUTDIR="${2:-/tmp/shots/$LANG_CODE}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUI="$ROOT/gui/build/linux/x64/release/bundle/yshjy_gui"
BIN="$ROOT/target/release/yshjy"
DISP_NUM=99

mkdir -p "$OUTDIR"

# ── 1) 造样例：一个目录 + 几个文件，打成伪装包 ────────────────────
# 用假 HOME：截图里出现的路径是 /tmp/yshjy-demo/...，不会暴露真实家目录
FAKEHOME=/tmp/yshjy-demo
rm -rf "$FAKEHOME"; mkdir -p "$FAKEHOME/下载"
if [ "${LANG_CODE%%-*}" = "en" ]; then
  DL="Downloads"
  SAMPLE="SampleGame"; SUB_A="data"; SUB_B="patch"
  F1="assets.bin"; F2="sprite 01.dat"; F3="readme.txt"; F4="hotfix.dat"
else
  DL="下载"
  SAMPLE="示例游戏"; SUB_A="游戏数据"; SUB_B="补丁"
  F1="资源包.bin"; F2="立绘 01.dat"; F3="说明.txt"; F4="hotfix.dat"
fi
WORK="$FAKEHOME/$DL/$SAMPLE"
mkdir -p "$WORK/$SUB_A" "$WORK/$SUB_B"
head -c 300000 /dev/urandom > "$WORK/$SUB_A/$F1"
head -c 120000 /dev/urandom > "$WORK/$SUB_A/$F2"
printf 'readme\n' > "$WORK/$F3"
printf 'patch\n' > "$WORK/$SUB_B/$F4"
( cd "$FAKEHOME/$DL" && "$BIN" ys "$SAMPLE" >/dev/null 2>&1 )
PKG=$(ls "$FAKEHOME/$DL/$SAMPLE".mp4 2>/dev/null | head -1)
[ -n "$PKG" ] || { echo "!! 没造出样例包"; exit 1; }

# ── 2) 隔离的用户环境（全部放 /tmp，截图里不出现真实家目录）───────
CFG=/tmp/yshjy-demo-cfg; STA=/tmp/yshjy-demo-state
rm -rf "$CFG" "$STA"
mkdir -p "$CFG/yshjy" "$STA/yshjy"
printf '[general]\nui_language = "%s"\n' "$LANG_CODE" > "$CFG/yshjy/config.toml"
: > "$STA/yshjy/onboarding_done"                       # 不弹首次引导
printf '1100 760 0' > "$STA/yshjy/gui-window.txt"      # 固定窗口尺寸
cp -f "$BIN" "$(dirname "$GUI")/yshjy"                 # 让界面用刚编的引擎

# ── 3) 虚拟屏幕 + 界面 ──────────────────────────────────────────
Xvfb :$DISP_NUM -screen 0 1240x880x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
XPID=$!
sleep 3
export DISPLAY=:$DISP_NUM
export LIBGL_ALWAYS_SOFTWARE=1
export GDK_BACKEND=x11
export HOME="$FAKEHOME"
export XDG_CONFIG_HOME="$CFG" XDG_STATE_HOME="$STA"

"$GUI" "$PKG" > /tmp/gui.log 2>&1 &
GPID=$!
sleep 12

WID=$(xwininfo -display :$DISP_NUM -root -tree 2>/dev/null | grep -i "杨三海解压" | head -1 | awk '{print $1}')
if [ -z "$WID" ]; then
  echo "!! 没找到主窗口"; tail -20 /tmp/gui.log
  kill $GPID $XPID 2>/dev/null; exit 1
fi
echo "窗口 $WID：$(xwininfo -display :$DISP_NUM -id $WID 2>/dev/null | grep -E 'Width|Height' | tr '\n' ' ')"

shot() { import -display :$DISP_NUM -window "$WID" "$OUTDIR/$1" 2>/tmp/import.log && echo "  ✓ $1"; }

# 导航栏坐标（1100x760 窗口，左侧 rail：x=42，y=126/194/262/330/398）
click() { python3 "$ROOT/scripts/xclick.py" "$1" "$2" ":$DISP_NUM" >/dev/null; sleep "${3:-2}"; }

shot preview.png
# 导航栏实测坐标（1100x760 窗口）：解压 126 / 打包 194 / 历史 262 / 设置 296 / 关于 364
click 42 194;                    shot pack.png
click 42 296;                    shot settings.png
click 42 364;                    shot about.png
click 42 262;                    shot history.png
click 42 126; click 918 687 6;   shot done.png

kill $GPID 2>/dev/null; sleep 1; kill $XPID 2>/dev/null; wait 2>/dev/null
echo "完成：$OUTDIR"
ls -la "$OUTDIR"
