#!/usr/bin/env bash
# 分卷 ZIP（`.zip + .z01`）能真的解开 —— 回归探针
#
# 由来（2026-11-02 审计 A-5）：
#   `C12_split` 这种 `zip -s 300k` 生成的分卷包，以前
#     `--list` 只能按文件头重建出 **1 项**（实际 5 个文件）
#     `--test` 直接报「这不是压缩包」
#   根因：分卷 ZIP 的中央目录偏移和每个条目的 local header 偏移都是**卷内坐标**
#   （相对各自那一卷的起点），而解码器只认一条连续流。
#   ★ 更要紧的是：`tests/` 里**零断言**用我们自己的引擎验过它
#     （`make_samples.py` 只拿 `7z t` 验）—— 账本点名的「假绿温床」。
#     这个探针就是补那一刀：7z 能解 ≠ 我们能解。
#
# 用法：bash tests/zip_split.sh [引擎路径]
#
# ⚠️ 变量名/函数名一律英文 —— bash 不认中文标识符（本项目踩过）
set -u

BIN_RAW="${1:-target/release/yshjy}"
DATA="$(cd "$(dirname "$0")" && pwd)/data"
ZIP="$DATA/C12_split.zip"
Z01="$DATA/C12_split.z01"
[ -x "$BIN_RAW" ] || { echo "找不到引擎: $BIN_RAW"; exit 2; }
BIN="$(readlink -f "$BIN_RAW")"
[ -f "$ZIP" ] && [ -f "$Z01" ] || { echo "找不到样本: $ZIP / $Z01"; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$TMP" || exit 2

FAIL=0

# ① 列出清单：8 个条目 / 5 个文件，且不能还在走「按文件头重建」的降级路
out=$("$BIN" --no-color --list "$ZIP" 2>&1)
entries=$(printf '%s\n' "$out" | grep -cE '^ +([^ ]+ +)?[0-9]+\.')
if [ "$entries" -eq 8 ]; then
    echo "  ✓ --list 列出 8 个条目"
else
    echo "  ✗ --list 只列出 $entries 个条目（应为 8）"
    printf '%s\n' "$out" | head -6 | sed 's/^/      /'
    FAIL=$((FAIL + 1))
fi
if printf '%s' "$out" | grep -q "5 个文件"; then
    echo "  ✓ 统计出 5 个文件"
else
    echo "  ✗ 没统计出 5 个文件"
    FAIL=$((FAIL + 1))
fi
if printf '%s' "$out" | grep -q "重建清单"; then
    echo "  ✗ 还在走「中央目录坏了、按文件头重建」的降级路（说明索引还没修好）"
    FAIL=$((FAIL + 1))
else
    echo "  ✓ 走的是正常中央目录，没降级重建"
fi

# ② --test：必须「校验通过」，不能出现「这不是压缩包」
out=$("$BIN" --no-color --test "$ZIP" 2>&1)
if printf '%s' "$out" | grep -q "校验通过"; then
    echo "  ✓ --test 校验通过"
else
    echo "  ✗ --test 没通过"
    printf '%s\n' "$out" | head -4 | sed 's/^/      /'
    FAIL=$((FAIL + 1))
fi

# ③ 从 .z01 进也要能列（用户经常直接点第一个分卷）
out=$("$BIN" --no-color --list "$Z01" 2>&1)
if [ "$(printf '%s\n' "$out" | grep -cE '^ +([^ ]+ +)?[0-9]+\.')" -eq 8 ]; then
    echo "  ✓ 从 .z01 入口也能列出 8 个条目"
else
    echo "  ✗ 从 .z01 入口列不全"
    printf '%s\n' "$out" | head -4 | sed 's/^/      /'
    FAIL=$((FAIL + 1))
fi

# ④ 真解压，内容逐字节核对
"$BIN" --no-color -o "$TMP/ours" "$ZIP" >/dev/null 2>&1
files=$(cd "$TMP/ours" 2>/dev/null && find . -type f | wc -l)
if [ "$files" -eq 5 ]; then
    echo "  ✓ 解出 5 个文件"
else
    echo "  ✗ 只解出 $files 个文件（应为 5）"
    FAIL=$((FAIL + 1))
fi

# 5 个文件的 sha256 是实测值（与 `7z x` 逐字节一致，2026-11-02）
check_hash() {
    local rel="$1" want="$2" got
    got=$(sha256sum "$TMP/ours/$rel" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$want" ]; then
        echo "  ✓ $rel"
    else
        echo "  ✗ $rel 内容不对（want=${want:0:12}… got=${got:0:12}…）"
        FAIL=$((FAIL + 1))
    fi
}
check_hash "payload/说明.txt" "35cceff75fd796e9eee6628c195005b4822226976c02b43b1b0c962f9f7232e4"
check_hash "payload/中文目录/文件1.txt" "dcce1c4d288bc946083c482f6c0b8d92fb888eb24b5137c47d882ba88d392033"
# 这条在**卷 1 里跨过卷边界**（loff 205211 + 204835 > 307200），最能证明拼接是对的
check_hash "payload/random.bin" "bd7be37e4d4e1032503d4ae8b174eb3cc790725c01758b11e2c49f79d5a862ea"
check_hash "payload/中文目录/sub/空格 文件名.bin" "931786beaabd68effab7028ee7114ad5bb49dee16049d0da464ae315855b812c"
check_hash "payload/empty.txt" "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

# ⑤ 有 7z 就再跟它比一遍（对方是"标准答案"，我们不看它脸色做事，只做对账）
if command -v 7z >/dev/null 2>&1; then
    7z x -y -o"$TMP/ref" "$ZIP" >/dev/null 2>&1
    if (cd "$TMP/ours" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$TMP/a.txt" 2>/dev/null \
       && (cd "$TMP/ref" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$TMP/b.txt" 2>/dev/null; then
        if diff -q "$TMP/a.txt" "$TMP/b.txt" >/dev/null; then
            echo "  ✓ 与 7z 解出的结果逐字节一致"
        else
            echo "  ✗ 与 7z 的结果不一致："
            diff "$TMP/a.txt" "$TMP/b.txt" | sed 's/^/      /'
            FAIL=$((FAIL + 1))
        fi
    fi
else
    echo "  - 没装 7z，跳过对账（上面 5 个哈希已经是实测值）"
fi

if [ "$FAIL" -eq 0 ]; then
    echo "分卷 ZIP：全部通过"
    exit 0
fi
echo "分卷 ZIP：$FAIL 项失败"
exit 1
