#!/usr/bin/env bash
# 窗口尺寸记忆（P10）探针 —— 断言「上次的窗口大小真的被采用」「用完随手落盘」「不会越开越大」。
#
# 为什么只能这么测（这几条路都试过，别再试）：
#   · Wayland 协议不许客户端查询别人的窗口几何 —— 外部量不出窗口多大；
#   · GNOME 48 的 `org.gnome.Shell.Screenshot` 对非 shell 进程回 AccessDenied（实测）；
#   · xdotool / wmctrl 在 Wayland 会话里拿不到窗口（本机也没装）。
# 所以界面里留了一条打点（`YSHJY_WINDOW_TRACE=1` → stderr 打
# 「load raw WxH -> apply WxH」/「configure surface … content …」/「save WxH」），
# 探针断言这条线。
#
# 默认不跑：会真的启动界面窗口，桌面会闪几下。要跑得显式设
# `YSHJY_GUI_VISIBLE_TEST=1` —— 用户明确讨厌回归时往桌面弹窗（踩过两次）。
#
# 用法：YSHJY_GUI_VISIBLE_TEST=1 bash tests/gui_window_state.sh
#       YSHJY_GUI_BIN=/path/to/yshjy_gui …（默认自动找构建产物）

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

PASS=0
FAIL=0
FAILED=()
ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
  echo "跳过：没有图形会话（WAYLAND_DISPLAY / DISPLAY 都是空的）"
  exit 0
fi
if [ "${YSHJY_GUI_VISIBLE_TEST:-}" != "1" ]; then
  echo "跳过：本探针会真的开界面窗口、桌面会闪几下；要跑请设 YSHJY_GUI_VISIBLE_TEST=1"
  exit 0
fi

# ── 找界面二进制 ────────────────────────────────────────────────
BIN="${YSHJY_GUI_BIN:-}"
if [ -z "$BIN" ]; then
  for c in gui/build/linux/x64/debug/bundle/yshjy_gui \
           gui/build/linux/x64/release/bundle/yshjy_gui \
           /usr/lib/yshjy/gui/yshjy_gui; do
    if [ -x "$c" ]; then BIN="$c"; break; fi
  done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "失败：找不到界面二进制（先 cd gui && flutter build linux --debug）"
  exit 1
fi
echo "界面二进制：$BIN"

# ── 隔离状态目录：绝不碰用户真实的 ~/.local/state/yshjy ──────────
STATE=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_guiwin.XXXXXX")
export XDG_STATE_HOME="$STATE"
WSTATE="$STATE/yshjy/gui-window.txt"
mkdir -p "$STATE/yshjy"

# 起界面、等到「窗口真的 configure 过并且落盘」再杀。
#
# 为什么不能只 sleep 固定秒数：debug 构建首帧要 6 秒上下（release 快得多），
# 固定 4 秒会拿到「还没 configure」的空日志 —— 2026-10-29 实测就是这样误判过。
launch() { # launch <日志前缀> <最多等几秒>
  local prefix="$1"
  local limit="${2:-20}"
  local i=0
  YSHJY_WINDOW_TRACE=1 "$BIN" >"$prefix.out" 2>"$prefix.err" &
  local pid=$!
  while [ "$i" -lt "$((limit * 2))" ]; do
    sleep 0.5
    i=$((i+1))
    if grep -q "window: save " "$prefix.err" 2>/dev/null; then break; fi
    if ! kill -0 "$pid" 2>/dev/null; then break; fi
  done
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
}

saved_line() { grep -o 'window: save [0-9]*x[0-9]* maximized=[01]' "$1" | tail -1; }
apply_line() { grep -o 'load raw [0-9]*x[0-9]* -> apply [0-9]*x[0-9]*' "$1" | head -1; }

# ── 场景 1：上次是 880x700 → 按 880 开，并把内容区尺寸写回 ────────────
#
# 断言「宽正好 880」而不是「完全等于 880x700」：GTK 在 map 时会按主题微调
# 高度（实测 700 → 688，差 12px），这是 GTK 的自由度，不是我们的 bug。
printf '880 700 0\n' > "$WSTATE"
L1="$STATE/c1"
launch "$L1" 25
if [ "$(apply_line "$L1.err")" = "load raw 880x700 -> apply 880x700" ]; then
  ok "读回上次尺寸 880x700 并原样应用"
else
  bad "读回上次尺寸 880x700 并原样应用（实际：$(apply_line "$L1.err")）"
fi
S1=$(saved_line "$L1.err")
S1W=$(echo "$S1" | sed -n 's/.*save \([0-9]*\)x.*/\1/p')
S1H=$(echo "$S1" | sed -n 's/.*save [0-9]*x\([0-9]*\) .*/\1/p')
if [ -n "$S1W" ] && [ "$S1W" -eq 880 ] && [ "$S1H" -ge 660 ] && [ "$S1H" -le 705 ]; then
  ok "尺寸随手落盘（${S1W}x${S1H}）"
else
  bad "尺寸随手落盘（实测 $S1；期望宽 880、高 660–705）"
fi

# ── 场景 2：拿场景 1 存下的值再开一次 → 必须**一模一样**（不许滚雪球）─────
#
# 这条是这轮真正的 bug 所在：早先保存的是「含 CSD 装饰的 surface 尺寸」，
# 载入时又当内容区用，实测 880→992→1104 一轮比一轮宽。
BEFORE=$(cat "$WSTATE")
L2="$STATE/c2"
launch "$L2" 25
AFTER=$(cat "$WSTATE")
if [ "$BEFORE" = "$AFTER" ]; then
  ok "再开一次尺寸不变（$AFTER，不滚雪球）"
else
  bad "再开一次尺寸变了（$BEFORE → $AFTER，滚雪球）"
fi

# ── 场景 3：文件里是超大值 → clamp 进工作区 ──────────────────────
printf '5000 4000 0\n' > "$WSTATE"
L3="$STATE/c3"
launch "$L3" 25
APPLIED=$(apply_line "$L3.err" | sed 's/.*apply //')
AW=${APPLIED%x*}
if [ -n "$APPLIED" ] && [ "$AW" -lt 5000 ]; then
  ok "超大值 5000x4000 被 clamp 成 $APPLIED"
else
  bad "超大值没被 clamp（实际：${APPLIED:-无 trace}）"
fi
S3=$(cat "$WSTATE")
S3W=$(echo "$S3" | cut -d' ' -f1)
if [ "$S3W" != "5000" ]; then
  ok "clamp 后的尺寸被写回（现在：$S3）"
else
  bad "clamp 后的尺寸没写回（文件还是：$S3）"
fi
# 宽度不许贴到屏幕边缘：surface 超屏时合成器会强制最大化，用户看到的是
# 「我明明记着大小，它自己变全屏了」—— 实测 1254 宽内容 = 1366 surface > 1280 屏。
#
# 极端值场景只记录不断言：clamp 到 1254x705 后合成器仍可能把窗口最大化
# （实测会，而且 clamp 到 1140 也照样最大化 —— 触发条件是「窗口三边都贴着
# 工作区」这类启发式，不是单纯的宽度比较）。真实路径里存的就是窗口真实尺寸，
# 不会走到这里，所以不当失败。
printf '  · 极大值场景实测窗口状态：%s（仅供参考，不断言）\n' "$S3"

# ── 场景 4：文件是垃圾 → 走默认比例，不许崩、不许把垃圾留着 ─────────
printf 'abc\n' > "$WSTATE"
L4="$STATE/c4"
launch "$L4" 25
A4=$(apply_line "$L4.err")
case "$A4" in
  "load raw 0x0 -> apply 0x0"|"")
    bad "坏文件处理不对（实际：${A4:-没有 trace}）" ;;
  "load raw 0x0 -> apply "*)
    ok "坏文件当没读过（走默认比例：$A4）" ;;
  *)
    bad "坏文件处理不对（实际：$A4）" ;;
esac
if grep -qE '^[0-9]+ [0-9]+ [0-9]+$' "$WSTATE" 2>/dev/null; then
  ok "坏文件被合法尺寸覆盖（现在：$(cat "$WSTATE")）"
else
  bad "坏文件没被合法尺寸覆盖（文件：$(cat "$WSTATE" 2>/dev/null)）"
fi
if grep -qiE "Gtk-CRITICAL|segmentation|abort" "$L4.err"; then
  bad "坏文件场景界面报了严重错误"
  grep -iE "Gtk-CRITICAL|segmentation|abort" "$L4.err" | head -3 | sed 's/^/    /'
else
  ok "坏文件场景无 Gtk-CRITICAL / 崩溃"
fi

echo
printf '  窗口尺寸记忆：通过 %d · 失败 %d（现场：%s）\n' "$PASS" "$FAIL" "$STATE"
if [ "$FAIL" -gt 0 ]; then
  for c in "${FAILED[@]}"; do printf '    · %s\n' "$c"; done
  exit 1
fi
exit 0
