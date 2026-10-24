#!/usr/bin/env bash
# zip 符号链接探针（阶段 1 梯队四）
#
# tar 早就支持符号链接（extract_stream.rs），zip 一直当普通文件解 —— 于是装 Linux
# 软件包（很多包里有 link -> target）时链接会变成一个内容为目标路径的小文本文件，
# 装出来的程序是坏的，还不报错。
#
# 规则跟 tar 共用一套：只接受「相对路径 + 规范化后仍在解压目录内」；绝对路径、
# 用 `..` 爬出去的一律拒绝并说明（不静默、不假装解出来了）。
#
# ⚠️ bash 变量名一律英文（bash 不认中文变量名）
set -u

BIN="${1:-./target/release/yshjy}"
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

if ! command -v zip >/dev/null 2>&1; then
  echo "  -- 没有 zip 命令，跳过"
  exit 0
fi

mkdir -p "$TMP/src"
cd "$TMP/src" || exit 1
echo '我是说明文件' > note.txt
ln -s note.txt good_link.txt
ln -s /etc/passwd abs_evil
ln -s ../../etc/passwd rel_evil
# -y 必须加：否则 zip 会跟随链接，把目标内容存进去（那就不是符号链接条目了）
zip -y -q "$TMP/test.zip" note.txt good_link.txt abs_evil rel_evil
cd /home/user/hermes/yshjy || exit 1

"$BIN" -o "$TMP/out" "$TMP/test.zip" >"$TMP/log" 2>&1
rc=$?
dest="$TMP/out/test"

check "整体解压没失败（退出码 $rc）" "$([ "$rc" = "0" ] || [ "$rc" = "5" ] && echo 0 || echo 1)"
check "普通文件解出来了" "$([ -f "$dest/note.txt" ] && echo 0 || echo 1)"
check "正常链接是真符号链接（不是文本文件）" \
  "$([ -L "$dest/good_link.txt" ] && echo 0 || echo 1)"
check "正常链接指向 note.txt" \
  "$([ "$(readlink "$dest/good_link.txt" 2>/dev/null)" = "note.txt" ] && echo 0 || echo 1)"
check "穿过链接能读到内容" \
  "$(grep -q '我是说明文件' "$dest/good_link.txt" 2>/dev/null && echo 0 || echo 1)"

check "绝对路径链接被拒（没建出来）" \
  "$([ ! -e "$dest/abs_evil" ] && [ ! -L "$dest/abs_evil" ] && echo 0 || echo 1)"
check "「..」逃逸链接被拒（没建出来）" \
  "$([ ! -e "$dest/rel_evil" ] && [ ! -L "$dest/rel_evil" ] && echo 0 || echo 1)"
check "拒绝时说清了原因（防逃逸）" \
  "$(grep -q '防逃逸' "$TMP/log" && echo 0 || echo 1)"
check "拒绝不是静默的（有「跳过」字样）" \
  "$(grep -q '跳过' "$TMP/log" && echo 0 || echo 1)"

echo
if [ "$fail" = "0" ]; then
  echo "  zip 符号链接：全部符合预期"
  exit 0
fi
echo "  zip 符号链接：$fail 项不符合预期"
exit 1
