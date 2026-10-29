#!/usr/bin/env bash
# 预览树「按目录勾选」的产物一致性 —— 第三期 A（`docs/THIRD-PHASE-PLAN.md` §A 验收第 4 条）
#
# **界面里的树只是显示层**：勾一个目录，最终送给引擎的还是一串**文件序号**
# （`src/select.rs:8` 的 index 锚点）。所以这里钉的不是像素，而是这条接线：
#   ① 「一串序号」与「等价的通配表达式」解出的东西必须**逐字节一致**
#   ② 只勾一部分 → 产物条数正好是勾的条数（不多解、不少解）
#   ③ 挑选不改变内容（与全解里的同名文件 sha256 一致）
#
# 为什么不在探针里开界面：tree 的构建/三态/面包屑是纯函数，已经有 19 条 flutter 单测
# （`gui/test/entry_tree_test.dart`）；这里补的是「勾完点开始解压之后对不对」。
#
# 用法：bash tests/tree_select.sh [引擎路径]

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

S=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_treesel.XXXXXX")
PASS=0
FAIL=0
FAILED=()
ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

# ── 样本：三层目录 + 中文名（6 个文件、4 个目录）──
mkdir -p "$S/src/合集A/第一季" "$S/src/合集A/第二季" "$S/src/另一个"
printf 'A-1\n'   > "$S/src/合集A/第一季/一.txt"
printf 'A-2\n'   > "$S/src/合集A/第一季/二.txt"
printf 'A-3\n'   > "$S/src/合集A/第一季/三.txt"
printf 'B-1\n'   > "$S/src/合集A/第二季/四.txt"
printf 'C-1\n'   > "$S/src/另一个/五.txt"
printf 'TOP\n'   > "$S/src/六.txt"
(cd "$S/src" && zip -q -r "$S/包.zip" .) || { echo "失败：造样本失败（缺 zip）"; exit 1; }

# ── 拿序号：界面里树勾「合集A/第一季」等于把这几个 index 一起传出去 ──
mapfile -t META < <(python3 - "$S" "$BIN" <<'PY'
import json, subprocess, sys
S, BIN = sys.argv[1], sys.argv[2]
out = subprocess.run([BIN, "--list", "--json", f"{S}/包.zip"],
                     capture_output=True, text=True).stdout
ent = None
for line in out.splitlines():
    line = line.strip()
    if not line:
        continue
    try:
        d = json.loads(line)
    except Exception:
        continue
    if "PreviewReady" in d:
        ent = d["PreviewReady"]["entries"]
if ent is None:
    print("ERR")
    print("ERR")
    print("0")
    raise SystemExit
files = [e for e in ent if not e["is_dir"]]
sub = sorted([e for e in files if e["path"].startswith("合集A/第一季/")],
             key=lambda x: x["index"])
one = [e for e in files if e["path"] == "另一个/五.txt"]
print(",".join(str(e["index"]) for e in sub))
print(",".join(str(e["index"]) for e in one))
print(len(files))
PY
)
IDX_SUB="${META[0]:-ERR}"
IDX_ONE="${META[1]:-ERR}"
TOTAL="${META[2]:-0}"

if [ "$IDX_SUB" = "ERR" ] || [ -z "$IDX_SUB" ]; then
  echo "失败：--list 拿不到条目（引擎或样本有问题）"
  rm -rf "$S"
  exit 1
fi
echo "样本：$TOTAL 个文件；「合集A/第一季」= 序号 $IDX_SUB；单挑「另一个/五.txt」= 序号 $IDX_ONE"

run() { # run <输出目录> <select 表达式>
  local out="$1" sel="$2"
  "$BIN" --no-smart-dir -o "$out" --select "$sel" "$S/包.zip" >"$S/log_$(basename "$out").txt" 2>&1
}

count_files() { find "$1" -type f | wc -l; }

# ── ① 序号串 vs 通配：逐字节一致 ──
run "$S/by_index" "$IDX_SUB"
run "$S/by_glob" "合集A/第一季/*"
if [ "$(count_files "$S/by_index")" = "3" ]; then
  ok "按序号串（$IDX_SUB）解出 3 个文件"
else
  bad "按序号串解出的文件数 = $(count_files "$S/by_index")，应为 3（见 $S/log_by_index.txt）"
fi
if [ "$(count_files "$S/by_glob")" = "3" ]; then
  ok "按通配（合集A/第一季/*）解出 3 个文件"
else
  bad "按通配解出的文件数 = $(count_files "$S/by_glob")，应为 3"
fi
if diff -r "$S/by_index" "$S/by_glob" >/dev/null 2>&1; then
  ok "两条路径的产物逐字节一致（树勾目录 == 通配）"
else
  bad "两条路径产物不一致：$(diff -r "$S/by_index" "$S/by_glob" 2>&1 | head -3)"
fi

# ── ② 只勾一个：产物正好一个，不多不少 ──
run "$S/one" "$IDX_ONE"
if [ "$(count_files "$S/one")" = "1" ]; then
  ok "只勾一个 → 产物只有 1 个文件（没把整包解出来）"
else
  bad "只勾一个却解出 $(count_files "$S/one") 个文件"
fi

# ── ③ 挑选不改变内容：与全解的同名文件 sha256 相同 ──
run "$S/all" "all"
if [ "$(count_files "$S/all")" = "6" ]; then
  ok "不传 select（全解）→ 6 个文件（正面对照）"
else
  bad "全解文件数 = $(count_files "$S/all")，应为 6"
fi
F_ONE="$(find "$S/one" -type f | head -1)"
F_ALL="$S/all/另一个/五.txt"
if [ -n "$F_ONE" ] && [ -f "$F_ALL" ]; then
  if [ "$(sha256sum "$F_ONE" | cut -d' ' -f1)" = "$(sha256sum "$F_ALL" | cut -d' ' -f1)" ]; then
    ok "挑出来的文件与全解版本 sha256 一致"
  else
    bad "挑出来的文件与全解版本内容不同"
  fi
else
  bad "找不到对比用的产物（$F_ONE / $F_ALL）"
fi

# ── ④ 空目录条目不该被当成文件解出（树里显示但不可勾）──
mkdir -p "$S/src2/空目录"
printf 'x\n' > "$S/src2/有文件.txt"
(cd "$S/src2" && zip -q -r "$S/空包.zip" .)
"$BIN" --no-smart-dir -o "$S/empty" "all" "$S/空包.zip" >/dev/null 2>&1
if [ -d "$S/empty/空目录" ] || [ -z "$(ls -A "$S/empty/空目录" 2>/dev/null)" ]; then
  ok "空目录条目：树里能显示、解出来是空目录（不是文件）"
else
  bad "空目录条目被解成了文件"
fi

echo
if [ "$FAIL" -eq 0 ]; then
  printf '结果：%d 通过 / 0 失败\n' "$PASS"
  rm -rf "$S"
  exit 0
else
  printf '结果：%d 通过 / %d 失败\n' "$PASS" "$FAIL"
  for f in "${FAILED[@]}"; do printf '  ✗ %s\n' "$f"; done
  echo "现场保留在 $S"
  exit 1
fi
