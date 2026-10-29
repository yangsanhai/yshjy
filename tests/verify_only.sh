#!/usr/bin/env bash
# `--test`（只测不写）验收：完整解码 + 校验，但一个字节都不落盘
#
# 由来（2026-10-29）：`--test` 是「查包完不完整」的动作（7-Zip 的 `t`、Ark 的
# `SupportsTesting` 是同一个能力）。难点不在解码，而在**什么都不留下** ——
# 只要有一条落盘路径没接上开关，用户就会在「测一下」之后发现多出一堆文件。
#
# 断言：
#   1. 好包 + `--test`：退出码 0，且**输出目录根本没被创建**
#   2. 好包 + `--test` + `-o` 指定新目录：同样零产物、零目录
#   3. 数据损坏的包 + `--test`：退出码非 0（校验真的在跑，不是走个过场）
#   4. `--test` 与 `--trash-source` 互斥：退出码 2（不许「测完删原包」）
#   5. 只测模式不碰原包：源文件还在
#   6. 正面对照：同一个包不加 `--test` 能正常解出 3 个文件（证明样本和路径都是好的）
#   7. `--json` 模式下也零产物（脚本场景）
#
# 用法：bash tests/verify_only.sh [二进制路径]
set -u
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
    for c in target/release/yshjy target/debug/yshjy; do
        [ -x "$c" ] && { BIN="$c"; break; }
    done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "❌ 找不到可用的 yshjy 二进制（先 cargo build --release）"
    exit 1
fi
case "$BIN" in /*) ;; *) BIN="$(pwd)/$BIN" ;; esac

n_pass=0
n_fail=0
ok()  { echo "  ✓ $1"; n_pass=$((n_pass+1)); }
bad() { echo "  ✗ $1"; n_fail=$((n_fail+1)); }

D="${TMPDIR:-/tmp}/yshjy_verify_$$"
mkdir -p "$D/src"
cd "$D" || exit 1

# ── 造样本：一个正常的 3 文件 zip，一份改坏了数据的副本 ──
python3 - "$D" <<'PY'
import sys, zipfile, pathlib, shutil
d = pathlib.Path(sys.argv[1])
good = d / "src" / "好包.zip"
with zipfile.ZipFile(good, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("a.txt", "hello 杨三海" * 100)
    z.writestr("子目录/b.bin", bytes(range(256)) * 40)
    z.writestr("c.txt", "0123456789" * 500)

# 坏包：翻转数据区里一个字节 → CRC 对不上（不改目录表，包结构仍然合法）
data = bytearray(good.read_bytes())
pos = data.find(b"PK\x03\x04")
# 跳过 local header（30 字节 + 文件名 + 扩展名）找到第一个条目的数据
import struct
nlen, elen = struct.unpack_from("<HH", data, pos + 26)
start = pos + 30 + nlen + elen
data[start + 10] ^= 0xFF          # 保证落在压缩数据里（前 10 字节留给流头）
bad = d / "src" / "坏包.zip"
bad.write_bytes(bytes(data))

# 第二个格式：tar.gz —— 一次覆盖 tar 与「单流（gzip）」两条落盘路径
import tarfile, io
tgz = d / "src" / "样本.tar.gz"
with tarfile.open(tgz, "w:gz") as t:
    for name, payload in (("x/一.txt", b"alpha" * 200), ("x/y/二.bin", bytes(range(200)) * 20)):
        info = tarfile.TarInfo(name)
        info.size = len(payload)
        t.addfile(info, io.BytesIO(payload))
print(f"样本：好包 {good.stat().st_size} B / 坏包 {bad.stat().st_size} B / tar.gz {tgz.stat().st_size} B")
PY
if [ ! -f "$D/src/好包.zip" ] || [ ! -f "$D/src/坏包.zip" ]; then
    echo "❌ 样本没造出来"
    exit 1
fi

echo "── 1. 好包 + --test：退出码 0 且不建输出目录 ──"
mkdir -p "$D/run1"
cp "$D/src/好包.zip" "$D/run1/"
out=$(cd "$D/run1" && "$BIN" --test 好包.zip 2>&1); rc=$?
extra=$(cd "$D/run1" && ls -A | grep -v '^好包\.zip$' | wc -l)
if [ "$rc" = 0 ] && [ "$extra" = 0 ]; then
    ok "退出码 0，目录里除了原包什么都没多出来"
else
    bad "退出码=$rc 目录里多出 $extra 项（应为 0）"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 2. 好包 + --test -o 新目录：目录不该被创建 ──"
out=$("$BIN" --test -o "$D/run2/产物" "$D/src/好包.zip" 2>&1); rc=$?
if [ "$rc" = 0 ] && [ ! -e "$D/run2/产物" ]; then
    ok "退出码 0，输出目录没被创建"
else
    bad "退出码=$rc，输出目录存在=$( [ -e "$D/run2/产物" ] && echo 是 || echo 否 )"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 3. 坏包 + --test：校验必须真的报错 ──"
out=$("$BIN" --test -o "$D/run3" "$D/src/坏包.zip" 2>&1); rc=$?
if [ "$rc" != 0 ]; then
    ok "坏包退出码非 0（$rc）—— 校验不是走过场"
else
    bad "坏包居然退出码 0：说明 --test 没在真校验"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 4. --test 与 --trash-source 互斥 ──"
out=$("$BIN" --test --trash-source -o "$D/run4" "$D/src/好包.zip" 2>&1); rc=$?
if [ "$rc" = 2 ]; then
    ok "退出码 2（用法错误），拒绝「测完动原包」"
else
    bad "退出码=$rc（应为 2）"
    printf '%s\n' "$out" | sed 's/^/      /' | head -6
fi

echo "── 5. 只测模式不碰原包 ──"
if [ -f "$D/src/好包.zip" ] && [ -f "$D/src/坏包.zip" ]; then
    ok "两个源包都还在（没被移动/删除）"
else
    bad "源包不见了 —— 只测模式动了源文件"
fi

echo "── 6. 正面对照：不加 --test 能正常解出 3 个文件 ──"
out=$("$BIN" -o "$D/run6" "$D/src/好包.zip" 2>&1); rc=$?
n=$(find "$D/run6" -type f 2>/dev/null | wc -l)
if [ "$rc" = 0 ] && [ "$n" -ge 3 ]; then
    ok "正常解压退出码 0，解出 $n 个文件"
else
    bad "正常解压退出码=$rc 文件数=$n（样本或默认路径有问题）"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 7. --json 模式也零产物 ──"
mkdir -p "$D/run7"
cp "$D/src/好包.zip" "$D/run7/"
out=$(cd "$D/run7" && "$BIN" --test --json 好包.zip 2>&1); rc=$?
extra=$(cd "$D/run7" && ls -A | grep -v '^好包\.zip$' | wc -l)
json_ok=$(printf '%s\n' "$out" | python3 -c '
import json, sys
n = 0
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        json.loads(line)
        n += 1
    except Exception:
        pass
print(1 if n else 0)
')
if [ "$rc" = 0 ] && [ "$extra" = 0 ] && [ "$json_ok" = 1 ]; then
    ok "--json 事件流正常且目录零产物"
else
    bad "rc=$rc 多出=$extra 有JSON=$json_ok"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 8. tar.gz：tar + 单流两条落盘路径也只测不写 ──"
mkdir -p "$D/run8"
cp "$D/src/样本.tar.gz" "$D/run8/"
out=$(cd "$D/run8" && "$BIN" --test 样本.tar.gz 2>&1); rc=$?
extra=$(cd "$D/run8" && ls -A | grep -v '^样本\.tar\.gz$' | wc -l)
out2=$(cd "$D/run8" && "$BIN" -o "$D/run8/正常" 样本.tar.gz 2>&1); rc2=$?
n2=$(find "$D/run8/正常" -type f 2>/dev/null | wc -l)
if [ "$rc" = 0 ] && [ "$extra" = 0 ] && [ "$rc2" = 0 ] && [ "$n2" = 2 ]; then
    ok "只测零产物；同一份包正常解出 $n2 个文件（对照成立）"
else
    bad "只测 rc=$rc 多出=$extra ／ 正常解 rc=$rc2 文件=$n2"
    printf '%s\n' "$out" | sed 's/^/      /' | head -6
fi

echo
echo "── 结果：通过 $n_pass · 失败 $n_fail ──"
[ "$n_fail" -eq 0 ] || exit 1
