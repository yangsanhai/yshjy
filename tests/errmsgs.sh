#!/usr/bin/env bash
# 错误原因说人话 —— 四场景回归探针
#
# 由来：体检 T9-P3 / T8-P5 实测的四个「说不清」场景：
#   ① 输出目录没写权限 → 曾报「中间有一段数据丢了」（用户以为包坏了）
#   ② -o 选到一个普通文件 → 曾报「内部错误」
#   ③ 路径超长 → 曾报「内部错误」
#   ④ 截断的压缩包 → 曾报「这不是压缩包」，同时又附「看起来是：ZIP 压缩包」（自相矛盾）
#
# 用法：bash tests/errmsgs.sh [引擎路径]
#
# ⚠️ 变量名和函数名一律用英文 —— bash 不认中文标识符（本项目已踩过多次）
set -u

BIN="${1:-target/release/yshjy}"
DATA="$(cd "$(dirname "$0")" && pwd)/data"
[ -x "$BIN" ] || { echo "找不到引擎: $BIN"; exit 2; }
[ -f "$DATA/C1_plain.zip" ] || { echo "找不到样本: $DATA/C1_plain.zip"; exit 2; }

TMP=$(mktemp -d)
trap 'chmod -R u+rwX "$TMP" 2>/dev/null; rm -rf "$TMP"' EXIT
FAIL=0

# 断言：输出里必须出现某个词，退出码必须等于期望
# 参数：标题 想要的词 期望退出码 —— 其余参数原样传给引擎
expect() {
    local title="$1" want="$2" want_rc="$3"
    shift 3
    local out rc
    out=$("$BIN" --no-color "$@" 2>&1)
    rc=$?
    if [ "$rc" -ne "$want_rc" ]; then
        echo "  ✗ $title：退出码 $rc（期望 $want_rc）"
        printf '%s\n' "$out" | head -3 | sed 's/^/      /'
        FAIL=$((FAIL + 1))
        return
    fi
    if ! printf '%s' "$out" | grep -q "$want"; then
        echo "  ✗ $title：没看到「$want」"
        printf '%s\n' "$out" | head -3 | sed 's/^/      /'
        FAIL=$((FAIL + 1))
        return
    fi
    echo "  ✓ $title"
}

# ① 输出目录没写权限 → 应该明确说「不让写」，不能说「数据丢了」
mkdir -p "$TMP/readonly"
chmod 555 "$TMP/readonly"
expect "只读目录报「不让写」" "不让写" 1 -o "$TMP/readonly" "$DATA/C1_plain.zip"
chmod 755 "$TMP/readonly"

# ② 目标位置是个普通文件 → 应该明确说「这是个文件」
: > "$TMP/plainfile"
expect "目标是文件报「不是文件夹」" "不是文件夹" 1 -o "$TMP/plainfile" "$DATA/C1_plain.zip"

# ③ 路径超长 → 应该明确说「路径太长」
LONGPATH="$TMP/$(printf 'verylongdirname%.0s' {1..20})"
expect "路径超长报「路径太长」" "路径太长" 1 -o "$LONGPATH" "$DATA/C1_plain.zip"

# ④ 截断的压缩包 → 应该报「文件不完整」，且不能再冒「这不是压缩包」
TRUNC="$TMP/truncated.zip"
head -c 3000 "$DATA/C1_plain.zip" > "$TRUNC"
expect "截断包报「文件不完整」" "文件不完整" 1 -o "$TMP/out4" "$TRUNC"
if "$BIN" --no-color -o "$TMP/out4b" "$TRUNC" 2>&1 | grep -q "这不是压缩包"; then
    echo "  ✗ 截断包仍在说「这不是压缩包」（自相矛盾）"
    FAIL=$((FAIL + 1))
fi

# ⑤ 对照组：正常包正常目录必须照旧成功（别把好路堵死）
expect "对照：正常解压成功" "解开" 0 -o "$TMP/ok" "$DATA/C1_plain.zip"
if [ ! -d "$TMP/ok/payload" ]; then
    echo "  ✗ 对照组没解出 payload 目录"
    FAIL=$((FAIL + 1))
fi

if [ "$FAIL" -gt 0 ]; then
    echo "  错误文案探针：$FAIL 项不达标"
    exit 1
fi
echo "  错误文案探针：全部达标"
