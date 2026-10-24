#!/usr/bin/env bash
# RAR 缺卷判定探针（阶段 1 梯队四·第 1 项）
#
# 改之前的样子：缺最后一卷时报
#   ❌ 这不是压缩包（没找到可识别的压缩数据）
#      ℹ 这个文件看起来是：RAR 压缩包
# —— 自相矛盾，用户完全不知道自己缺了什么。
#
# 改之后：靠 RAR 头里的 split_after 位（「本条目内容续到下一卷」）判定，
# 缺卷就说话明白，齐全照常解。
#
# ⚠️ bash 变量名一律英文（中文变量名 bash 不认，这轮踩过两次）
set -u

BIN="${1:-./target/release/yshjy}"
SRC=tests/data/rar_vol_sample
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
fail=0

check() {
  local name="$1"
  local ok="$2"
  if [ "$ok" = "0" ]; then
    echo "  ✓ $name"
  else
    echo "  ✗ $name"
    fail=$((fail + 1))
  fi
}

if [ ! -f "$SRC/stored_volume_0.rar" ]; then
  echo "  -- 样本不在（$SRC），跳过"
  exit 0
fi

# ① 四卷齐全：应当正常解开，并解出 split.bin
mkdir -p "$TMP/full"
cp "$SRC"/stored_volume_0.rar "$SRC"/stored_volume_1.rar \
   "$SRC"/stored_volume_2.rar "$SRC"/stored_volume_3.rar "$TMP/full/"
"$BIN" -o "$TMP/out_full" "$TMP/full/stored_volume_0.rar" >"$TMP/full.log" 2>&1
rc_full=$?
check "四卷齐全能解开（退出码 $rc_full）" "$([ "$rc_full" = "0" ] && echo 0 || echo 1)"
check "解出了 split.bin" "$([ -f "$TMP/out_full/split.bin" ] && echo 0 || echo 1)"

# ② 缺最后一卷：只说「分卷不齐」，不许再说「这不是压缩包」
mkdir -p "$TMP/miss"
cp "$SRC"/stored_volume_0.rar "$SRC"/stored_volume_1.rar \
   "$SRC"/stored_volume_2.rar "$TMP/miss/"
"$BIN" -o "$TMP/out_miss" "$TMP/miss/stored_volume_0.rar" >"$TMP/miss.log" 2>&1
rc_miss=$?

check "缺卷时退出码非 0（实际 $rc_miss）" "$([ "$rc_miss" != "0" ] && echo 0 || echo 1)"
check "缺卷时文案说了「分卷」" \
  "$(grep -q '分卷' "$TMP/miss.log" && echo 0 || echo 1)"
check "缺卷时不再说「这不是压缩包」" \
  "$(grep -q '不是压缩包' "$TMP/miss.log" && echo 1 || echo 0)"
check "缺卷时一个文件都不写" \
  "$([ ! -e "$TMP/out_miss/split.bin" ] && echo 0 || echo 1)"

# ③ 错误码必须是 MissingVolume（Rust 枚举名，界面查文案时再转 MISSING_VOLUME）
"$BIN" --json -o "$TMP/out_miss2" "$TMP/miss/stored_volume_0.rar" 2>/dev/null \
  | grep -q '"Failed":"MissingVolume"'
check "错误码是 MissingVolume" "$?"

# ④ 单卷路径也要判得出（目录里只有第一卷）
mkdir -p "$TMP/only_one"
cp "$SRC"/stored_volume_0.rar "$TMP/only_one/"
"$BIN" -o "$TMP/out_one" "$TMP/only_one/stored_volume_0.rar" >"$TMP/one.log" 2>&1
check "只给一卷时也说「分卷」" \
  "$(grep -q '分卷' "$TMP/one.log" && echo 0 || echo 1)"

echo
if [ "$fail" = "0" ]; then
  echo "  RAR 缺卷判定：全部符合预期"
  exit 0
fi
echo "  RAR 缺卷判定：$fail 项不符合预期"
exit 1
