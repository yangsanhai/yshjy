#!/usr/bin/env bash
# 文件管理器右键脚本（nautilus-script-yshjy）探针
#
# 断言两条路，重点是第二条 —— 用户会右键点 U 盘、/mnt、别人共享过来的只读目录：
#   ① 包所在目录可写 → 原地解到同名文件夹（原行为，别回退）
#   ② 不可写 → **不许静默失败**：解到「下载/解压-<包名>」，并在通知里说清去了哪
#
# 为什么不用真 Nautilus 跑：右键脚本的输入就是一个环境变量
# （NAUTILUS_SCRIPT_SELECTED_FILE_PATHS），直接喂它等价于右键点击；
# 起个文件管理器反而测不了「只读目录」这种条件。
#
# 通知是弹窗（用户讨厌），所以这里塞一个假的 notify-send 拦下来，只看内容。
#
# 用法：bash tests/nautilus_script.sh [引擎路径]

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
  for c in target/release/yshjy target/debug/yshjy; do
    [ -x "$c" ] && BIN="$(pwd)/$c" && break
  done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "失败：找不到引擎（先 cargo build --release）"
  exit 1
fi

SCRIPT="$(pwd)/gui/linux/nautilus-script-yshjy"
if [ ! -f "$SCRIPT" ]; then
  echo "失败：找不到 $SCRIPT"
  exit 1
fi

S=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_nau.XXXXXX")
mkdir -p "$S/ro" "$S/rw" "$S/dl" "$S/cache" "$S/fakebin"
PASS=0
FAIL=0
FAILED=()
ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

# 假 notify-send：只记录参数，不真弹通知
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> "%s/notify.txt"\n' "$S" > "$S/fakebin/notify-send"
chmod +x "$S/fakebin/notify-send"

python3 - "$S" <<'PY'
import sys, zipfile, os
S = sys.argv[1]
for sub in ("ro", "rw"):
    with zipfile.ZipFile(os.path.join(S, sub, "样本包.zip"), "w") as z:
        z.writestr("里面/中文.txt", "hello 中文\n")
PY

run_script() { # run_script <包路径>
  rm -f "$S/notify.txt"
  NAUTILUS_SCRIPT_SELECTED_FILE_PATHS="$1" \
  YSHJY_BIN="$BIN" \
  XDG_CACHE_HOME="$S/cache" \
  XDG_DOWNLOAD_DIR="$S/dl" \
  PATH="$S/fakebin:$PATH" \
    bash "$SCRIPT" >/dev/null 2>&1
}

# ① 可写：原地解
run_script "$S/rw/样本包.zip"
rc=$?
if [ "$rc" -eq 0 ]; then ok "可写目录：退出码 0"; else bad "可写目录：退出码 $rc"; fi
if [ -f "$S/rw/里面/中文.txt" ]; then
  ok "可写目录：原地解出了 里面/中文.txt"
else
  bad "可写目录：没解出内容（$(find "$S/rw" -type f | wc -l) 个文件）"
fi
if grep -q "跟包在同一个文件夹里" "$S/notify.txt" 2>/dev/null; then
  ok "可写目录：通知说清了落在原地"
else
  bad "可写目录：通知内容不对（$(cat "$S/notify.txt" 2>/dev/null)）"
fi

# ② 不可写：改投「下载」，且通知里说明
chmod 555 "$S/ro"
run_script "$S/ro/样本包.zip"
rc=$?
chmod 755 "$S/ro"
if [ "$rc" -eq 0 ]; then ok "只读目录：退出码 0（没有当成失败）"; else bad "只读目录：退出码 $rc"; fi
if [ -f "$S/dl/解压-样本包.zip/里面/中文.txt" ]; then
  ok "只读目录：解到了 下载/解压-样本包.zip 里"
else
  bad "只读目录：没落到「下载」（$(find "$S/dl" -type f 2>/dev/null | wc -l) 个文件）"
fi
if grep -q "原文件夹不能写" "$S/notify.txt" 2>/dev/null; then
  ok "只读目录：通知里说清了「原文件夹不能写、已解到下载」"
else
  bad "只读目录：通知没说清落点（$(cat "$S/notify.txt" 2>/dev/null)）"
fi
if [ ! -e "$S/ro/里面" ]; then
  ok "只读目录：没有往只读目录里硬写"
else
  bad "只读目录：只读目录里出现了产物"
fi

echo
printf '  右键脚本：通过 %d · 失败 %d（现场：%s）\n' "$PASS" "$FAIL" "$S"
if [ "$FAIL" -gt 0 ]; then
  for c in "${FAILED[@]}"; do printf '    · %s\n' "$c"; done
  exit 1
fi
exit 0
