#!/usr/bin/env bash
# 救援模式的两条硬承诺 —— 回归探针
#
# 由来（2026-11-02 审计 A-8）：
#   `--test --rescue` 以前**照样**在当前目录建 `<包名>.rescue/` 并把文件真写出来，
#   违反 `--test` 的「一个字节都不落盘」承诺。更糟的是对压缩炸弹开 `--rescue --test`
#   会真的写出（安全预期被打破）。根因：`do_rescue` 没把 verify_only 传进 UnpackCtx，
#   而且 `extract_by_locals_ctx` 第一句无条件 create_dir_all。
#
# 用法：bash tests/rescue_verify.sh [引擎路径]
#
# ⚠️ 变量名/函数名一律英文 —— bash 不认中文标识符（本项目踩过）
set -u

BIN_RAW="${1:-target/release/yshjy}"
DATA="$(cd "$(dirname "$0")" && pwd)/data"
SAMPLE="$DATA/extended_timestamp_bad.zip"
[ -x "$BIN_RAW" ] || { echo "找不到引擎: $BIN_RAW"; exit 2; }
# 下面要 cd 进临时目录，路径必须先转绝对，否则相对路径会失效
BIN="$(readlink -f "$BIN_RAW")"
[ -f "$SAMPLE" ] || { echo "找不到样本: $SAMPLE"; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$TMP" || exit 2

FAIL=0

# ① --test --rescue：一个字节都不落盘
out=$("$BIN" --no-color --test "$SAMPLE" --rescue 2>&1)
rc=$?
leftover=$(ls -A . 2>/dev/null)
if [ -n "$leftover" ]; then
    echo "  ✗ --test --rescue 写盘了：$(printf '%s' "$leftover" | tr '\n' ' ')"
    FAIL=$((FAIL + 1))
else
    echo "  ✓ --test --rescue 没写盘"
fi
if printf '%s' "$out" | grep -q "只测不写"; then
    echo "  ✓ 提示了「只测不写」"
else
    echo "  ✗ 没提示「只测不写」"
    printf '%s\n' "$out" | head -3 | sed 's/^/      /'
    FAIL=$((FAIL + 1))
fi

# ② 正常的 --rescue 必须照常落盘（别把功能一起修没了）
out=$("$BIN" --no-color "$SAMPLE" --rescue 2>&1)
if [ -d "extended_timestamp_bad.zip.rescue" ] && [ -f "extended_timestamp_bad.zip.rescue/mimetype" ]; then
    echo "  ✓ --rescue 照常落盘（产物在）"
    rm -rf "extended_timestamp_bad.zip.rescue"
else
    echo "  ✗ --rescue 不再落盘了 —— 功能被修坏"
    printf '%s\n' "$out" | head -4 | sed 's/^/      /'
    FAIL=$((FAIL + 1))
fi

if [ "$FAIL" -eq 0 ]; then
    echo "救援模式承诺：全部通过"
    exit 0
fi
echo "救援模式承诺：$FAIL 项失败"
exit 1
