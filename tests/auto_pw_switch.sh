#!/usr/bin/env bash
# T8 行为探针：「自动找密码」的两档各自只关自己那一档。
#
# 以前只有一个 --no-auto-password 总开关，界面想给两个勾选框也接不了。
# 这条探针证明拆开之后真的是两档：
#
#   包用「说明.txt 里的码」加密 → 关掉密码本那档，照样能解
#   包用「密码本里的码」加密   → 关掉说明文件那档，照样能解
#
# 用临时 XDG_CONFIG_HOME，不碰用户真实密码本。
#
# ★ bash 的变量名不认中文（会报"不是有效的标识符"），所以这里的标识符全用英文，
#   中文只放在字符串里。这条坑踩过一次，别再踩。
set -u
BIN="${1:-./target/release/yshjy}"

command -v zip >/dev/null 2>&1 || { echo "- skip: no zip command"; exit 0; }

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

export XDG_CONFIG_HOME="$T/cfg"
mkdir -p "$T/cfg/yshjy"
printf 'bookPW\n' > "$T/cfg/yshjy/passwords.txt"

# ★ 2026-10-31 起「自动找解压码」**默认关**（用户口径：程序不许默认翻他的东西）。
# 所以这条探针要验“两档各自只关自己”时，必须先把两档显式打开 —— 否则
# “default: should extract” 验的就不再是默认开，而是默认关（会误判成功能坏了）。
cat > "$T/cfg/yshjy/config.toml" <<'TOMLEOF'
[password]
auto_from_notes = true
auto_from_book = true
TOMLEOF

printf 'hello\n' > "$T/a.txt"
( cd "$T" && zip -q -P notePW note.zip a.txt && zip -q -P bookPW book.zip a.txt ) || {
  echo "FAIL: cannot build test zips"; exit 1; }
# 说明文件里只写了 notePW 那一个码
printf '解压码：notePW\n' > "$T/说明.txt"

bad=0
try() { # $1=expect(ok|fail)  $2=desc  rest=package + extra args
  local expect=$1 desc=$2; shift 2
  local out="$T/out"
  rm -rf "$out"; mkdir -p "$out"
  "$BIN" "$@" -o "$out" >/dev/null 2>&1
  local got=fail
  [ -f "$out/a.txt" ] && got=ok
  if [ "$got" = "$expect" ]; then
    echo "  PASS  $desc ($got)"
  else
    echo "  FAIL  $desc: expected $expect, got $got"
    bad=1
  fi
}

echo "-- package encrypted with the PASSWORD-FILE code (bookPW) --"
try ok   "default: should extract"                       "$T/book.zip"
try ok   "notes off only: book still on -> should extract" "$T/book.zip" --no-auto-password-notes
try fail "book off only: notes have the wrong code -> must fail" "$T/book.zip" --no-auto-password-book

echo "-- package encrypted with the NOTES code (notePW) --"
try ok   "default: should extract"                       "$T/note.zip"
try fail "notes off only: must fail"                     "$T/note.zip" --no-auto-password-notes
try ok   "book off only: notes remain, should extract"   "$T/note.zip" --no-auto-password-book

echo "-- master switch still turns both off --"
# ★ 2026-10-31 修：这两行以前只在**描述里**写了 `--no-auto-password`，参数根本没传
#   （默认开着的时候"不加开关能解开"= ok，被当成 fail 报出来才发现）。参数漏写 = 假测试。
try fail "note.zip + --no-auto-password"                 "$T/note.zip" --no-auto-password
try fail "book.zip + --no-auto-password"                 "$T/book.zip" --no-auto-password

# ★ 默认必须是关（用户 2026-10-31 口径）：什么都不配的时候，程序不许替用户
#   翻说明文件、也不许拿他的密码本去试 —— 只报「这个包带锁，需要解压码」。
#   这条**反向断言**才是这个功能真正的要求，别只验“打开后能用”。
echo "-- default is OFF: no config at all -> must ask, not auto-find --"
export XDG_CONFIG_HOME="$T/cfg_default"
mkdir -p "$T/cfg_default/yshjy"
printf 'notePW\nbookPW\n' > "$T/cfg_default/yshjy/passwords.txt"   # 现成的码也不许自动试
printf '解压码：notePW\n' > "$T/说明.txt"
try fail "default off: note.zip must NOT auto-find from notes"  "$T/note.zip"
try fail "default off: book.zip must NOT auto-try password book" "$T/book.zip"

exit $bad
