#!/usr/bin/env bash
# 条目 packSize 校验（libarchive 的 entry_bytes_declared 语义）探针
#
# 要防的是「声明 1 KB、实际吐 1 GB」这类包：老做法是写完再比对，磁盘先被写满；
# 新做法是**边写边按声明封顶**，多出来的字节根本不落盘，这一条判数据错误。
#
# 断言清单：
#   ① zip 正常包 → 正常解开（别把好包改坏）
#   ② zip 声明 < 实际（多写）→ 判数据错误，且**一个字节都不落盘**
#   ③ zip 声明 > 实际（少写）→ 判截断，且一个字节都不落盘
#   ④ 7z 正常包 → 正常解开（7z 侧也加了封顶，必须回归）
#   ⑤ 7z 被截短 → 不许「成功」返回（少写不能静默通过）
#   ⑥ rar 正常包 → 正常解开（rar 侧没动，回归确认没被带坏）
#
# 造样本：zip 用 python zipfile + 手改中央目录/本地头的 uncompressed size
# （±几行即可，见下）；7z/rar 用系统工具，缺了就跳过那几项。
#
# 用法：bash tests/pack_size_cap.sh [引擎路径]

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

S=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_packsize.XXXXXX")
PASS=0
FAIL=0
FAILED=()
ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

# ── 造 zip 样本 ────────────────────────────────────────────────
python3 - "$S" <<'PY'
import sys, struct, zipfile, os
S = sys.argv[1]

def make(path, name, payload):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as z:
        z.writestr(name, payload)

def patch_size(src, dst, declared):
    """把本地头 + 中央目录里的 uncompressed size 都改成 declared"""
    raw = bytearray(open(src, "rb").read())
    i = raw.find(b"PK\x03\x04")
    struct.pack_into("<I", raw, i + 22, declared)
    j = raw.find(b"PK\x01\x02")
    struct.pack_into("<I", raw, j + 24, declared)
    open(dst, "wb").write(raw)

make(os.path.join(S, "正常.zip"), "a.txt", b"x" * 300)
patch_size(os.path.join(S, "正常.zip"), os.path.join(S, "声明少.zip"), 100)
patch_size(os.path.join(S, "正常.zip"), os.path.join(S, "声明多.zip"), 1000)
PY

run() { # run <包> <输出目录>
  mkdir -p "$2"
  "$BIN" jy "$1" -o "$2" >"$2/.log" 2>&1
  echo $?
}
落盘数() { find "$1" -type f -not -name '.log' 2>/dev/null | wc -l; }

# ① zip 正常
rc=$(run "$S/正常.zip" "$S/o1")
if [ "$rc" -eq 0 ] && [ -f "$S/o1/a.txt" ] && [ "$(stat -c%s "$S/o1/a.txt")" -eq 300 ]; then
  ok "zip 正常包：解开 300 字节"
else
  bad "zip 正常包：rc=$rc，落盘 $(落盘数 "$S/o1") 个文件（内容 $(stat -c%s "$S/o1/a.txt" 2>/dev/null || echo 无) 字节）"
fi

# ② 多写（声明 100，实际 300）
rc=$(run "$S/声明少.zip" "$S/o2")
if [ "$rc" -ne 0 ]; then ok "zip 多写：没有当成成功（rc=$rc）"; else bad "zip 多写：居然返回成功"; fi
if [ "$(落盘数 "$S/o2")" -eq 0 ]; then
  ok "zip 多写：一个字节都没落盘"
else
  bad "zip 多写：落了 $(落盘数 "$S/o2") 个文件（$(find "$S/o2" -type f -not -name '.log' -exec ls -l {} \; | awk '{print $5}' | tr '\n' ' ')）"
fi
if grep -qiE "声明|截断|数据损|larger than" "$S/o2/.log"; then
  ok "zip 多写：给了人话原因"
else
  bad "zip 多写：没说清原因（$(tail -2 "$S/o2/.log" | tr '\n' ' ')）"
fi

# ③ 少写（声明 1000，实际 300）
rc=$(run "$S/声明多.zip" "$S/o3")
if [ "$rc" -ne 0 ]; then ok "zip 少写：没有当成成功（rc=$rc）"; else bad "zip 少写：居然返回成功"; fi
if [ "$(落盘数 "$S/o3")" -eq 0 ]; then
  ok "zip 少写：一个字节都没落盘"
else
  bad "zip 少写：落了 $(落盘数 "$S/o3") 个文件"
fi
if grep -q "条目声明 1000 字节" "$S/o3/.log"; then
  ok "zip 少写：报出「声明 1000、实际不足」"
else
  # 人类输出会把原因归成「数据损坏」这类人话，具体数字只在 --json 的 failed[].reason 里
  mkdir -p "$S/o3j"
  "$BIN" jy "$S/声明多.zip" -o "$S/o3j" --json >"$S/o3.json" 2>&1
  if grep -q "条目声明 1000 字节" "$S/o3.json"; then
    ok "zip 少写：--json 的 reason 里有「声明 1000、实际不足」"
  else
    bad "zip 少写：reason 里没有声明值（$(grep -o '"reason":"[^"]*"' "$S/o3.json" | head -1)）"
  fi
fi

# ④ 7z 正常（7z 侧也加了封顶，必须先证明好包还能解）
if command -v 7z >/dev/null 2>&1; then
  mkdir -p "$S/7zsrc" && printf 'y%.0s' $(seq 1 500) > "$S/7zsrc/b.txt"
  ( cd "$S/7zsrc" && 7z a -t7z -bso0 -bsp0 "$S/正常.7z" b.txt >/dev/null 2>&1 )
  rc=$(run "$S/正常.7z" "$S/o4")
  if [ "$rc" -eq 0 ] && [ -f "$S/o4/b.txt" ] && [ "$(stat -c%s "$S/o4/b.txt")" -eq 500 ]; then
    ok "7z 正常包：解开 500 字节"
  else
    bad "7z 正常包：rc=$rc，落盘 $(落盘数 "$S/o4") 个（$(tail -2 "$S/o4/.log" | tr '\n' ' ')）"
  fi

  # ⑤ 7z 被截短：少写不许静默成功
  head -c $(( $(stat -c%s "$S/正常.7z") - 40 )) "$S/正常.7z" > "$S/截短.7z"
  rc=$(run "$S/截短.7z" "$S/o5")
  if [ "$rc" -ne 0 ]; then
    ok "7z 截短：没有当成成功（rc=$rc）"
  else
    bad "7z 截短：居然返回成功"
  fi
else
  printf '  \033[33m–\033[0m %s\n' "7z 两项（跳过：没装 7z 命令）"
fi

# ⑥ rar 正常（rar 侧本轮没动，纯回归）
if command -v rar >/dev/null 2>&1; then
  mkdir -p "$S/rarsrc" && printf 'z%.0s' $(seq 1 400) > "$S/rarsrc/c.txt"
  ( cd "$S/rarsrc" && rar a -idq "$S/正常.rar" c.txt >/dev/null 2>&1 )
  rc=$(run "$S/正常.rar" "$S/o6")
  if [ "$rc" -eq 0 ] && [ -f "$S/o6/c.txt" ] && [ "$(stat -c%s "$S/o6/c.txt")" -eq 400 ]; then
    ok "rar 正常包：解开 400 字节"
  else
    bad "rar 正常包：rc=$rc，落盘 $(落盘数 "$S/o6") 个（$(tail -2 "$S/o6/.log" | tr '\n' ' ')）"
  fi
else
  printf '  \033[33m–\033[0m %s\n' "rar 一项（跳过：没装 rar 命令）"
fi

echo
printf '  packSize 校验：通过 %d · 失败 %d（现场：%s）\n' "$PASS" "$FAIL" "$S"
if [ "$FAIL" -gt 0 ]; then
  for c in "${FAILED[@]}"; do printf '    · %s\n' "$c"; done
  exit 1
fi
exit 0
