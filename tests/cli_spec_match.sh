#!/usr/bin/env bash
# CLI 选项三方对账：src/cli_spec.rs 的 OPTS 表 ↔ 三份补全脚本 ↔ --help 文案
#
# 由来（2026-10-28）：同一份选项清单曾经手抄在三处，实测已经漂了 ——
# `--import-passwords` 会取值却没登记进参数表，手写补全只算了 18 个长选项
# （实际 35 个）。这条探针就是为了让"漂移"当场变红，而不是等用户撞上。
#
# 断言：
#   1. 补全脚本与表一致（生成器 --check 模式，改表不改补全 → 红）
#   2. 补全文件里出现的每个长选项都在表里（没有孤儿）
#   3. 表里每个对外长选项都出现在补全文件里（没有漏登记）
#   4. `--help` 里写到的长选项都必须真实存在（不许出现假选项）
#   5. 三份补全的语法都能过解析（bash -n / zsh -n）
#
# 用法：bash tests/cli_spec_match.sh
set -u
cd "$(dirname "$0")/.." || exit 1

n_pass=0
n_fail=0
ok()  { echo "  ✓ $1"; n_pass=$((n_pass+1)); }
bad() { echo "  ✗ $1"; n_fail=$((n_fail+1)); }

# ── 找一个支持 --dump-cli-spec 的二进制（release 优先） ──
BIN=""
for cand in target/release/yshjy target/release/yshjy.exe target/debug/yshjy target/debug/yshjy.exe; do
    [ -x "$cand" ] || continue
    if "$cand" --dump-cli-spec >/dev/null 2>&1; then BIN="$cand"; break; fi
done
if [ -z "$BIN" ]; then
    echo "❌ 找不到支持 --dump-cli-spec 的二进制（先 cargo build）"
    exit 1
fi
echo "── 用 $BIN 读表 ──"

echo "── 1. 补全脚本是否与表同步 ──"
if python3 scripts/gen-cli-assets.py --check >/tmp/cli_spec_check.log 2>&1; then
    ok "补全脚本与表一致"
else
    bad "补全脚本与表不一致（跑 python3 scripts/gen-cli-assets.py）"
    sed 's/^/     /' /tmp/cli_spec_check.log | head -5
fi

echo "── 2/3. 补全文件 ↔ 表的选项集合 ──"
python3 - "$BIN" <<'PY'
import json, re, subprocess, sys
from pathlib import Path

bin_path = sys.argv[1]
spec = json.loads(subprocess.run([bin_path, "--dump-cli-spec"],
                                 capture_output=True, text=True).stdout)
table = {o["long"] for o in spec["options"] if not o["internal"]}
files = ["scripts/completions/yshjy.bash", "scripts/completions/_yshjy",
         "scripts/completions/yshjy.fish"]

bad = 0
for f in files:
    txt = Path(f).read_text(encoding="utf-8")
    # bash / zsh 写成 `--dest`；fish 写成 `-l dest`（不带横线），所以两套提取
    in_file = set(re.findall(r"(?<![\w-])--[a-z][a-z0-9-]+", txt))
    in_file |= {"--" + m for m in re.findall(r"-l\s+([a-z][a-z0-9-]+)", txt)}
    extra = in_file - table          # 补全里有、表里没有 → 孤儿
    missing = table - in_file        # 表里有、补全里没有 → 漏登记
    if extra:
        print(f"  ✗ {f}: 出现表里没有的选项 {sorted(extra)}")
        bad += 1
    elif missing:
        print(f"  ✗ {f}: 漏了 {len(missing)} 个选项 {sorted(missing)}")
        bad += 1
    else:
        print(f"  ✓ {f}: {len(table)} 个对外选项全在")
sys.exit(1 if bad else 0)
PY
if [ $? -eq 0 ]; then n_pass=$((n_pass+1)); else n_fail=$((n_fail+1)); fi

echo "── 4. --help 里不许出现假选项 ──"
python3 - "$BIN" <<'PY'
import json, re, subprocess, sys
bin_path = sys.argv[1]
spec = json.loads(subprocess.run([bin_path, "--dump-cli-spec"],
                                 capture_output=True, text=True).stdout)
all_long = {o["long"] for o in spec["options"]}
help_txt = subprocess.run([bin_path, "--help"], capture_output=True, text=True).stdout
in_help = set(re.findall(r"(?<![\w-])--[a-z][a-z0-9-]+", help_txt))
ghost = in_help - all_long
if ghost:
    print(f"  ✗ --help 里写了根本不存在（或没登记）的选项：{sorted(ghost)}")
    sys.exit(1)
print(f"  ✓ --help 里的 {len(in_help)} 个长选项都是真的")
PY
if [ $? -eq 0 ]; then n_pass=$((n_pass+1)); else n_fail=$((n_fail+1)); fi

echo "── 4b. man 页 ↔ 表的选项集合 ──"
python3 - "$BIN" <<'PY'
import json, re, subprocess, sys
from pathlib import Path
bin_path = sys.argv[1]
spec = json.loads(subprocess.run([bin_path, "--dump-cli-spec"],
                                 capture_output=True, text=True).stdout)
table = {o["long"] for o in spec["options"] if not o["internal"]}
all_long = {o["long"] for o in spec["options"]}
txt = Path("docs/yshjy.1").read_text(encoding="utf-8")
in_man = set(re.findall(r"(?<![\w-])--[a-z][a-z0-9-]+", txt))
missing = table - in_man          # man 页没写到的对外选项
extra = in_man - all_long         # man 页写了表里没有的（假选项）
if missing:
    print(f"  ✗ man 页漏了 {len(missing)} 个选项：{sorted(missing)}")
    sys.exit(1)
if extra:
    print(f"  ✗ man 页里有表里不存在的选项：{sorted(extra)}")
    sys.exit(1)
print(f"  ✓ man 页 {len(table)} 个对外选项全在")
PY
if [ $? -eq 0 ]; then n_pass=$((n_pass+1)); else n_fail=$((n_fail+1)); fi

# man 页版本号必须与 Cargo.toml 一致（历史上落后过三个版本）
V_CARGO=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
if grep -q "yshjy ${V_CARGO}" docs/yshjy.1; then
    ok "man 页版本号与 Cargo.toml 一致（${V_CARGO}）"
else
    bad "man 页版本号与 Cargo.toml 不一致（应为 ${V_CARGO}）"
fi

echo "── 5. 补全脚本语法 ──"
if command -v bash >/dev/null && bash -n scripts/completions/yshjy.bash 2>/dev/null; then
    ok "bash 补全语法通过（bash -n）"
else
    bad "bash 补全语法错误"
fi
if command -v zsh >/dev/null; then
    if zsh -n scripts/completions/_yshjy 2>/dev/null; then
        ok "zsh 补全语法通过（zsh -n）"
    else
        bad "zsh 补全语法错误"
    fi
else
    echo "  – 跳过 zsh 语法（本机没装 zsh）"
fi

echo
echo "── 结果：通过 $n_pass · 失败 $n_fail ──"
[ "$n_fail" -eq 0 ] || exit 1
