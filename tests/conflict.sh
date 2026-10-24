#!/usr/bin/env bash
# 重名策略接线回归探针（T9-P2）
#
# 由来：设置页「遇到同名文件」的下拉（自动改名 / 跳过 / 覆盖）以前只存不用 ——
# 引擎从来没读过 `config.output.conflict`，三种策略实测输出一模一样（永远改名）。
#
# 用法：bash tests/conflict.sh [引擎路径]
#
# ⚠️ 变量名/函数名一律英文（bash 不认中文标识符）
set -u

BIN="${1:-target/release/yshjy}"
DATA="$(cd "$(dirname "$0")" && pwd)/data"
[ -x "$BIN" ] || { echo "找不到引擎: $BIN"; exit 2; }
[ -f "$DATA/C1_plain.zip" ] || { echo "找不到样本: $DATA/C1_plain.zip"; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAIL=0

mk_conf() { printf '[output]\nconflict = "%s"\n' "$1" > "$TMP/$2.toml"; }

# 解压两遍（中间改一个文件），返回一份「文件数|副本数|原文件首行|退出码」
probe() {
    # ⚠️ 必须一句一个 local：bash 的 `local a=1 b=$a` 会先展开全部参数再赋值，
    # 同一句里引用刚定义的变量会撞上 set -u 的「未绑定变量」
    local mode="$1"
    local kind="$2"
    local dir="$TMP/${kind}_${mode}"
    mkdir -p "$dir"
    mk_conf "$mode" "${kind}_$mode"

    if [ "$kind" = "zip" ]; then
        local pkg="$DATA/C1_plain.zip" victim="$dir/payload/说明.txt"
        "$BIN" --no-color -q --config "$TMP/${kind}_$mode.toml" -o "$dir" "$pkg" >/dev/null 2>&1
        printf '我改过的内容\n' > "$victim"
        "$BIN" --no-color -q --config "$TMP/${kind}_$mode.toml" -o "$dir" "$pkg" >/dev/null 2>&1
    else
        local pkg="$TMP/src/a.txt.gz" victim="$dir/a.txt"
        "$BIN" --no-color -q --config "$TMP/${kind}_$mode.toml" -o "$dir" "$pkg" >/dev/null 2>&1
        printf '我改过的内容\n' > "$victim"
        "$BIN" --no-color -q --config "$TMP/${kind}_$mode.toml" -o "$dir" "$pkg" >/dev/null 2>&1
    fi
    local rc=$?

    printf '%s|%s|%s|%s' \
        "$(find "$dir" -type f | wc -l)" \
        "$(find "$dir" -type f -name '* (1)*' | wc -l)" \
        "$(head -1 "$victim")" \
        "$rc"
}

# 简单断言：字段必须等于期望
check() {
    local title="$1" got="$2" field="$3" want="$4"
    local val
    case "$field" in
        副本)   val=$(printf '%s' "$got" | cut -d'|' -f2) ;;
        内容)   val=$(printf '%s' "$got" | cut -d'|' -f3) ;;
        退出码) val=$(printf '%s' "$got" | cut -d'|' -f4) ;;
    esac
    if [ "$val" = "$want" ]; then
        echo "  ✓ $title"
    else
        echo "  ✗ $title：$field=$val（期望 $want）"
        FAIL=$((FAIL + 1))
    fi
}

# 造单流样本
mkdir -p "$TMP/src"
printf '这是单流样本的原文\n' > "$TMP/src/a.txt"
gzip -c "$TMP/src/a.txt" > "$TMP/src/a.txt.gz"

for kind in zip stream; do
    [ "$kind" = "zip" ] && label="ZIP" || label="单流 gz"

    r=$(probe rename "$kind")
    if [ "$kind" = "zip" ]; then
        check "$label rename：生成副本" "$r" 副本 5
    else
        check "$label rename：生成副本" "$r" 副本 1
    fi
    check "$label rename：原文件不被覆盖" "$r" 内容 "我改过的内容"

    r=$(probe replace "$kind")
    check "$label replace：不生成副本" "$r" 副本 0
    if [ "$kind" = "zip" ]; then
        check "$label replace：原文件被覆盖" "$r" 内容 "这是一个测试载荷。"
    else
        check "$label replace：原文件被覆盖" "$r" 内容 "这是单流样本的原文"
    fi

    r=$(probe skip "$kind")
    check "$label skip：不生成副本" "$r" 副本 0
    check "$label skip：原文件保留" "$r" 内容 "我改过的内容"
    # 5 = 部分成功（有内容按设置跳过）—— 这是设计语义，不是错误
    check "$label skip：退出码为部分成功" "$r" 退出码 5
done

if [ "$FAIL" -gt 0 ]; then
    echo "  重名策略探针：$FAIL 项不达标"
    exit 1
fi
echo "  重名策略探针：全部达标"
