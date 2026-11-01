#!/usr/bin/env bash
# T20 回归：压缩比闸在**四条不同路径**上都真的会拦，而且报的是「安全限额」不是「数据损坏」
#
# 为什么单独一个脚本：这个闸的判定散在 4 条代码路径里（zip / 7z / rar / 流式 tar / xz writer），
# 每一条的错误传播方式都不一样 —— 2026-11-02 实测就抓到两个只在真跑时才暴露的问题：
#   ① xz 单流走 `Write` 接口，错误码被压成 io::Error → 报「xz 解压失败」（像包坏了）
#   ② job.rs 的 tar 分支**漏传 packed_hint** → packed_total 恒为 0 → tar.gz/xz/zst
#      的压缩比闸**完全是哑的**（设了 100 倍也不拦）
# 单测只覆盖 ExtractBudget 本身，抓不到这两处「调用方没把数传对」的错。
#
# 用法：bash tests/t20_ratio.sh [二进制的绝对路径，默认 target/release/yshjy]

set -u
BIN="${1:-$PWD/target/release/yshjy}"
if [ ! -x "$BIN" ]; then
  echo "✗ 找不到可执行的引擎：$BIN（先 cargo build --release）"
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
PASS=0
FAIL=0

ok()   { PASS=$((PASS+1)); echo "  PASS $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL $1"; }

# 100 MB 全零数据：压缩比 1000~6600:1，足够触发任何档位
mkdir -p "$TMP/src"
truncate -s 100000000 "$TMP/src/数据.bin" || { echo "✗ truncate 不可用"; exit 1; }

cfg() {  # $1 = 倍数
  printf '[safety]\nzip_bomb_ratio = %s\n' "$1" > "$TMP/cfg.toml"
}

# JSON 里错误码是枚举名（`"Failed":"SafetyLimit"`），CLI 人话里是 `SAFETY_LIMIT`；两种都认
had() { grep -qE "$1|$2" "$TMP/out.log"; }
run() {  # $1 = 包路径, $2 = 输出目录
  "$BIN" --json --config "$TMP/cfg.toml" jy "$1" -o "$2" >"$TMP/out.log" 2>&1
  echo $?
}

echo "═══ 1. zip（extract.rs 路径）═══"
( cd "$TMP" && zip -9 -q p.zip src/数据.bin )
cfg 100
run "$TMP/p.zip" "$TMP/oz" >/dev/null
if had SAFETY_LIMIT SafetyLimit; then
  ok "设 100 倍 → 报 SAFETY_LIMIT"
else
  bad "设 100 倍没报 SAFETY_LIMIT（输出：$(head -c 200 "$TMP/out.log")）"
fi
if had DATA_LOST DataLost; then
  bad "又报成 DATA_LOST 了（用户会以为包坏了）"
else
  ok "没有误报成 DATA_LOST"
fi
cfg 0
run "$TMP/p.zip" "$TMP/oz0" >/dev/null
if [ -f "$TMP/oz0/src/数据.bin" ] && [ "$(stat -c%s "$TMP/oz0/src/数据.bin")" = "100000000" ]; then
  ok "默认（0=不检查）→ 解开且字节数完整"
else
  bad "默认配置下没解开（或大小不对）"
fi

echo "═══ 2. 纯 .xz（BudgetWriter 那条 Write 接口路径）═══"
xz -9 -c "$TMP/src/数据.bin" > "$TMP/pure.xz"
cfg 100
run "$TMP/pure.xz" "$TMP/ox" >/dev/null
if had SAFETY_LIMIT SafetyLimit; then
  ok "设 100 倍 → 报 SAFETY_LIMIT（不是「xz 解压失败」）"
else
  bad "纯 xz 没报 SAFETY_LIMIT（输出：$(head -c 200 "$TMP/out.log")）"
fi

echo "═══ 3. tar.xz（tar 家族路径 —— 曾经整条闸都是哑的）═══"
( cd "$TMP" && tar -cJf p.tar.xz src )
cfg 100
run "$TMP/p.tar.xz" "$TMP/ot" >/dev/null
if had SAFETY_LIMIT SafetyLimit; then
  ok "设 100 倍 → 报 SAFETY_LIMIT（packed_hint 没漏传）"
else
  bad "tar.xz 没拦（十有八九是 job.rs 的 tar 分支又漏了 packed_hint）"
fi
cfg 0
run "$TMP/p.tar.xz" "$TMP/ot0" >/dev/null
if [ -f "$TMP/ot0/src/数据.bin" ]; then
  ok "默认（不检查）→ 解开"
else
  bad "默认配置下 tar.xz 没解开"
fi

echo
echo "──── 通过 $PASS · 失败 $FAIL ────"
[ "$FAIL" -eq 0 ] && echo "全绿" || exit 1
