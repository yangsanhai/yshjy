#!/usr/bin/env bash
# yshjy 冒烟测试 —— 发布前跑一遍，覆盖所有关键场景
#
# 用法: bash tests/smoke.sh [二进制路径]
# 默认用 target/release/yshjy

set -u
BIN="${1:-target/release/yshjy}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/yshjy_smoke.XXXXXX")"
PASS=0
FAIL=0
FAILED_CASES=()

# ── 工具函数 ──
ok()   { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); FAILED_CASES+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }
check() { # check <描述> <期望退出码> <实际退出码>
  if [ "$2" = "$3" ]; then ok "$1（退出码 $3）"; else bad "$1（期望 $2，实际 $3）"; fi
}
head_line() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# 先把二进制路径解析成绝对路径（后面会 cd 走）
BIN="$(cd "$(dirname "$BIN")" 2>/dev/null && pwd)/$(basename "$BIN")"
[ -x "$BIN" ] || { echo "找不到可执行文件: $BIN"; exit 1; }

cd "$WORK" || exit 1

head_line "═══ 0. 准备测试素材 ═══"
mkdir -p src/sub
printf 'hello world\n' > src/a.txt
printf '中文内容测试\n' > "src/中文名.txt"
head -c 3000 /dev/urandom > src/b.bin
printf 'nested\n' > src/sub/c.txt
echo "  素材就绪（含中文名 + 嵌套目录）"

head_line "═══ 1. 帮助与用法 ═══"
"$BIN" --help >/dev/null 2>&1;   check "--help" 0 $?
"$BIN" >/dev/null 2>&1;          check "无参数（应提示用法）" 2 $?
"$BIN" /nonexistent-xyz >/dev/null 2>&1; check "不存在的文件（应报一般错误）" 1 $?

head_line "═══ 2. 打包 ═══"
"$BIN" ys -o plain.mp4 src >/dev/null 2>&1;  check "打包（默认 free 原子布局）" 0 $?
"$BIN" ys -o tail.mp4 --tail src >/dev/null 2>&1; check "打包（尾部追加布局）" 0 $?
"$BIN" ys -o pw.mp4 -p '密码123' src >/dev/null 2>&1; check "打包（带中文密码）" 0 $?

if [ -f plain.mp4 ]; then
  # 产物必须是合法 MP4（第 4..8 字节是 ftyp）
  if [ "$(dd if=plain.mp4 bs=1 skip=4 count=4 2>/dev/null)" = "ftyp" ]; then
    ok "产物外壳是合法 MP4（ftyp）"
  else
    bad "产物外壳不是合法 MP4"
  fi
fi

head_line "═══ 3. 列表 ═══"
"$BIN" --list plain.mp4 >/dev/null 2>&1; check "--list 正常包" 0 $?
if "$BIN" --list plain.mp4 2>/dev/null | grep -q "中文名.txt"; then
  ok "列表里中文名显示正确"
else
  bad "列表里中文名乱码"
fi
if "$BIN" --list pw.mp4 2>/dev/null | grep -q "🔒"; then
  ok "加密包显示锁标记"
else
  bad "加密包没显示锁标记"
fi

head_line "═══ 4. 解压（无加密）═══"
"$BIN" -o out1 plain.mp4 >/dev/null 2>&1; check "解压" 0 $?
if [ -f "out1/src/中文名.txt" ]; then ok "中文文件名落盘正确"; else bad "中文文件名落盘错误"; fi
if diff -q src/a.txt out1/src/a.txt >/dev/null 2>&1; then ok "内容逐字节一致"; else bad "内容不一致"; fi
if diff -q src/b.bin out1/src/b.bin >/dev/null 2>&1; then ok "二进制内容一致"; else bad "二进制内容不一致"; fi

head_line "═══ 5. 解压（加密）═══"
"$BIN" -o out2 -p '密码123' pw.mp4 >/dev/null 2>&1; check "带正确密码解压" 0 $?
"$BIN" -o out3 pw.mp4 >/dev/null 2>&1; check "不带密码（应报密码问题）" 4 $?
"$BIN" -o out4 -p '错误密码' pw.mp4 >/dev/null 2>&1; check "错密码（应报密码问题）" 4 $?

head_line "═══ 6. 尾部追加布局 ═══"
"$BIN" -o out5 tail.mp4 >/dev/null 2>&1; check "解压尾部追加布局的包" 0 $?
if diff -q src/a.txt out5/src/a.txt >/dev/null 2>&1; then ok "内容一致"; else bad "内容不一致"; fi

head_line "═══ 7. 边界情况 ═══"
head -c 5000 /dev/urandom > random.bin
"$BIN" random.bin >/dev/null 2>&1; check "随机数据（应报不是压缩包）" 3 $?
: > empty.bin
"$BIN" empty.bin >/dev/null 2>&1; check "空文件" 3 $?
printf 'PK\x03\x04garbagegarbage' > fake.bin
"$BIN" fake.bin >/dev/null 2>&1; rc=$?
if [ "$rc" = "3" ] || [ "$rc" = "5" ]; then ok "只有假签名（退出码 $rc，正确拒绝）"; else bad "假签名处理不当（退出码 $rc）"; fi

head_line "═══ 8. 安全护栏 ═══"
"$BIN" -o ../escape plain.mp4 >/dev/null 2>&1   # 输出目录带 .. 属于用户自愿，不该崩
check "输出目录含 .. 不崩" 0 $?

head_line "═══ 9. 安静模式（脚本用）═══"
LINES=$("$BIN" -q -o out6 plain.mp4 2>/dev/null | wc -l)
if [ "$LINES" -ge 1 ]; then ok "-q 只输出路径（$LINES 行）"; else bad "-q 输出为空"; fi

head_line "═══ 10. 自检闭环 ═══"
"$BIN" ys -o roundtrip.mp4 -p '闭环' src >/dev/null 2>&1
"$BIN" -o out7 -p '闭环' roundtrip.mp4 >/dev/null 2>&1
if diff -r src out7/src >/dev/null 2>&1; then
  ok "打包→解压→diff -r 完全一致"
else
  bad "闭环有差异"
fi

# ── 汇总 ──
printf '\n\033[1m═══════ 结果 ═══════\033[0m\n'
printf '  通过: %d\n  失败: %d\n' "$PASS" "$FAIL"
if [ "$FAIL" -gt 0 ]; then
  printf '\n  失败的用例:\n'
  for c in "${FAILED_CASES[@]}"; do printf '    · %s\n' "$c"; done
fi
printf '\n  工作目录: %s\n' "$WORK"
[ "$FAIL" -eq 0 ]
