#!/usr/bin/env bash
# C ABI 头文件 ↔ Rust 实现 对账（第二份真源漂移的看门狗）
#
# 由来（2026-10-29）：`ffi/include/yshjy.h` 是**手写**的，`ffi/src/lib.rs` 是唯一真源。
# 这和「man 页版本号停在旧版」「bash 补全漏登记 17 个选项」是**同一种病** ——
# 手抄的第二份真源没人对账，就会悄悄漂。本仓库已经在这类病上栽过两次。
#
# 断言：
#   1. 头文件声明的 `yshjy_*` 函数 == lib.rs 里 `pub extern "C" fn yshjy_*` 的实现（双向）
#   2. `YSHJY_ABI_VERSION` 宏 == lib.rs 的 `ABI_VERSION` 常量
#   3. `ffi/Cargo.toml` 的 version == 主 `Cargo.toml` 的 version
#      （实测发现它停在 0.9.20 而主线已 0.9.29 —— 版本号是「第六处」，规则 16 的五处清单漏了它）
#   4. 头文件里每个函数都带 `yshjy_` 前缀（C ABI 命名空间，别出现裸名字）
#
# 用法：bash tests/ffi-header-sync.sh
set -u
cd "$(dirname "$0")/.." || exit 1

n_pass=0
n_fail=0
ok()  { echo "  ✓ $1"; n_pass=$((n_pass+1)); }
bad() { echo "  ✗ $1"; n_fail=$((n_fail+1)); }

HDR="ffi/include/yshjy.h"
IMPL="ffi/src/lib.rs"
for f in "$HDR" "$IMPL" ffi/Cargo.toml Cargo.toml; do
    if [ ! -r "$f" ]; then
        echo "❌ 找不到 $f"
        exit 1
    fi
done

echo "── 1. 头文件 ↔ 实现的函数集合 ──"
python3 - "$HDR" "$IMPL" <<'PY'
import re, sys
from pathlib import Path

hdr_txt = Path(sys.argv[1]).read_text(encoding="utf-8")
impl_txt = Path(sys.argv[2]).read_text(encoding="utf-8")

impl = set(re.findall(r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(yshjy_\w+)', impl_txt))
# 头文件里 `yshjy_event_cb` 是 typedef（后面跟 `)`），不会命中；只抓真正的声明
hdr = set(re.findall(r'\b(yshjy_\w+)\s*\(', hdr_txt))

missing_in_hdr = impl - hdr      # 实现了但头文件没声明 → 宿主用不了
missing_in_impl = hdr - impl     # 头文件声明了但没实现 → 链接时炸

bad = 0
if missing_in_hdr:
    print(f"  ✗ 实现了但头文件没声明（宿主调不到）：{sorted(missing_in_hdr)}")
    bad += 1
if missing_in_impl:
    print(f"  ✗ 头文件声明了但没实现（链接会炸）：{sorted(missing_in_impl)}")
    bad += 1
if not bad:
    print(f"  ✓ {len(impl)} 个 extern \"C\" 函数两边一致")
sys.exit(1 if bad else 0)
PY
if [ $? -eq 0 ]; then n_pass=$((n_pass+1)); else n_fail=$((n_fail+1)); fi

echo "── 2. ABI 版本号 ──"
macro=$(grep -oE 'YSHJY_ABI_VERSION[[:space:]]+[0-9]+' "$HDR" | grep -oE '[0-9]+$' | head -1)
const=$(grep -oE 'ABI_VERSION:[[:space:]]*u32[[:space:]]*=[[:space:]]*[0-9]+' "$IMPL" | grep -oE '[0-9]+$' | head -1)
if [ -n "$macro" ] && [ "$macro" = "$const" ]; then
    ok "YSHJY_ABI_VERSION=${macro} 与 ABI_VERSION=${const} 一致"
else
    bad "ABI 版本不一致：头文件=${macro:-缺} 实现=${const:-缺}"
fi

echo "── 3. ffi/Cargo.toml 的版本号要跟主线一致 ──"
v_main=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
v_ffi=$(grep -m1 '^version' ffi/Cargo.toml | cut -d'"' -f2)
if [ "$v_main" = "$v_ffi" ]; then
    ok "ffi/Cargo.toml 与主线同为 ${v_main}"
else
    bad "版本漂移：ffi/Cargo.toml=${v_ffi} 主线=${v_main}（发版时一起改）"
fi

echo "── 4. 头文件里的函数都带 yshjy_ 前缀 ──"
python3 - "$HDR" <<'PY'
import re, sys
from pathlib import Path
lines = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
names = []
for ln in lines:
    # `typedef void (*yshjy_event_cb)(…)` 是函数指针类型定义，不是函数声明，跳过
    if ln.lstrip().startswith(("typedef", "#", "/*", "*", "//")):
        continue
    m = re.search(r'^[A-Za-z_][\w\s\*]*?\b([A-Za-z_]\w*)\s*\([^;]*\)\s*;', ln)
    if m:
        names.append(m.group(1))
naked = [n for n in names if not n.startswith("yshjy_")]
if naked:
    print(f"  ✗ 没有 C ABI 前缀的函数：{sorted(set(naked))}")
    sys.exit(1)
print(f"  ✓ {len(names)} 个声明全部带 yshjy_ 前缀")
PY
if [ $? -eq 0 ]; then n_pass=$((n_pass+1)); else n_fail=$((n_fail+1)); fi

echo
echo "── 结果：通过 $n_pass · 失败 $n_fail ──"
[ "$n_fail" -eq 0 ] || exit 1
