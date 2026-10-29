#!/usr/bin/env bash
# 干扰数据 / 假签名（`--decoy`）验收 —— 第三期 B
#
# 由来（2026-10-29）：`--decoy` 往伪装包里撒噪声段、可选夹假特征码，让扫描器
# 分不清哪段才是真压缩包。风险全在「撒错地方」：噪声一旦落进 MP4 盒子结构，
# 视频就播不了；落进压缩流内部，包就解不开。
#
# 所以这个探针**不只测能不能打包**，重点是「噪声确实在压缩数据之外」：
#   1. 不带 --decoy 能正常打包（对照）
#   2. --decoy 4 --decoy-signatures 打包成功
#   3. 两次打包逐字节一致（可复现：随机量只由输入路径派生，不看系统时间）
#   4. ZIP 起点 > 噪声总量（噪声全在压缩数据之前，没插进去）
#   5. 7-Zip 能列内容（实测：假签名后必须跟 30 个零，否则 7z 会随机性列不出）
#   6. unzip 能列内容
#   7. `file` 仍认成 MP4（不是 OpenPGP / data）
#   8. `--test` 通过（完整解码 + 校验）
#   9. 解出的内容与不带干扰的版本**逐文件 sha256 完全一致**
#  10. 产物里能数到假特征码（签名开关真的生效）
#  11. L1 尾部追加布局同样通过
#  12. 错误路径：--decoy 65 / --decoy-size 2M / --sfx 互斥 → 退出码 2
#
# 反向验证（2026-10-29 做过）：把引擎里 FreeAtom 分支的「写噪声」挪到拷贝 ZIP **之后**
# （即噪声落进压缩数据之后），第 2 条与第 4 条当场变红（12 通过 / 2 失败），改回全绿。
#
# 用法：bash tests/decoy.sh [二进制路径]
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

D="${TMPDIR:-/tmp}/yshjy_decoy_$$"
mkdir -p "$D/src"
cd "$D" || exit 1
trap 'rm -rf "$D"' EXIT

# ── 造样本：一个目录（两个文件，1 个略大，压缩后体积才有意义）──
head -c 300000 /dev/urandom > src/随机.bin
printf 'hello 杨三海 干扰数据\n' > src/说明.txt

# ── 1. 对照组：不带干扰 ──
if "$BIN" ys src -o plain.mp4 --quiet >/dev/null 2>&1; then
    ok "不带 --decoy 能打包（对照）"
else
    bad "不带 --decoy 打包失败"
fi

# ── 2. 带干扰 + 假签名 ──
if "$BIN" ys src -o d1.mp4 --decoy 4 --decoy-signatures --quiet >/dev/null 2>&1; then
    ok "--decoy 4 --decoy-signatures 打包成功"
else
    bad "--decoy 4 --decoy-signatures 打包失败"
fi
"$BIN" ys src -o d2.mp4 --decoy 4 --decoy-signatures --quiet >/dev/null 2>&1

# ── 3. 可复现 ──
if [ "$(sha256sum < d1.mp4)" = "$(sha256sum < d2.mp4)" ]; then
    ok "两次打包逐字节一致（可复现）"
else
    bad "两次打包不一致（随机量不该受系统时间影响）"
fi

# ── 4. 噪声在压缩数据之外 ──
python3 - "$D" <<'PY'
import sys, pathlib, struct
d = pathlib.Path(sys.argv[1])
raw = (d / "d1.mp4").read_bytes()
# 找最后一个 EOCD（ZIP 的唯一可靠锚点），再按 central directory 反推起点
eocd = raw.rfind(b"PK\x05\x06")
assert eocd > 0, "产物里找不到 EOCD"
cd_size, cd_off = struct.unpack_from("<II", raw, eocd + 12)
base = eocd - cd_size - cd_off
# 噪声总量 = 4 段 × 16 KiB（默认 --decoy-size）
noise = 4 * 16 * 1024
print(f"    ZIP 起点 = {base} 字节，噪声总量 = {noise} 字节，封面 = 1688 字节")
sys.exit(0 if base > noise else 1)
PY
if [ $? -eq 0 ]; then
    ok "ZIP 起点在全部干扰数据之后（噪声没插进压缩数据）"
else
    bad "ZIP 起点落在干扰数据之内 —— 噪声写错位置了"
fi

# ── 5/6. 第三方工具还能列内容 ──
if command -v 7z >/dev/null 2>&1; then
    if 7z l d1.mp4 2>/dev/null | grep -q '随机.bin'; then
        ok "7-Zip 能列出内容"
    else
        bad "7-Zip 列不出内容（假签名后的填充是不是被改回随机了？）"
    fi
else
    echo "  - 7z 没装，跳过"
fi
if command -v unzip >/dev/null 2>&1; then
    if unzip -l d1.mp4 2>/dev/null | grep -q '随机.bin'; then
        ok "unzip 能列出内容"
    else
        bad "unzip 列不出内容"
    fi
else
    echo "  - unzip 没装，跳过"
fi

# ── 7. 仍然看起来是媒体 ──
if command -v file >/dev/null 2>&1; then
    f=$(file -b d1.mp4)
    case "$f" in
        *ISO\ Media*|*MP4*) ok "file 认成 MP4（$f）" ;;
        *) bad "file 认不出是 MP4：$f" ;;
    esac
else
    echo "  - file 没装，跳过"
fi

# ── 8. 只测不写能通过 ──
if "$BIN" --test d1.mp4 --quiet >/dev/null 2>&1; then
    ok "--test 通过（完整解码 + 校验）"
else
    bad "--test 失败"
fi

# ── 9. 解出的内容与对照组逐文件一致 ──
rm -rf o1 o2
"$BIN" plain.mp4 -o o1 --quiet >/dev/null 2>&1
"$BIN" d1.mp4 -o o2 --quiet >/dev/null 2>&1
h1=$(cd o1 2>/dev/null && find . -type f | sort | xargs -r sha256sum)
h2=$(cd o2 2>/dev/null && find . -type f | sort | xargs -r sha256sum)
if [ -n "$h1" ] && [ "$h1" = "$h2" ]; then
    ok "解出的内容与不带干扰的版本逐文件 sha256 一致"
else
    bad "解出的内容不一致（噪声污染了压缩数据）"
fi

# ── 10. 假特征码确实在里面 ──
hits=$(python3 - d1.mp4 <<'PY'
import sys
d = open(sys.argv[1], "rb").read()
sigs = [b"Rar!", b"PK\x03\x04", b"\x1f\x8b\x08", b"BZh", b"7z\xbc\xaf\x27\x1c"]
# 减去真 ZIP 自带的 PK\x03\x04（local header 至少 2 处：目录头 + 2 个条目）
print(sum(d.count(s) for s in sigs))
PY
)
if [ "$hits" -gt 3 ]; then
    ok "产物里数到 $hits 处特征码（含假签名）"
else
    bad "产物里只有 $hits 处特征码，假签名可能没写进去"
fi

# ── 11. L1 尾部追加布局 ──
if "$BIN" ys src -o l1.mp4 --tail --decoy 4 --decoy-signatures --quiet >/dev/null 2>&1 \
   && "$BIN" --test l1.mp4 --quiet >/dev/null 2>&1; then
    ok "L1 尾部追加 + 干扰数据同样通过 --test"
else
    bad "L1 + 干扰数据失败"
fi

# ── 12. 错误路径 ──
"$BIN" ys src -o bad1.mp4 --decoy 65 >/dev/null 2>&1
[ $? -eq 2 ] && ok "--decoy 65 被拒（退出码 2）" || bad "--decoy 65 没被拒"
"$BIN" ys src -o bad2.mp4 --decoy 4 --decoy-size 2M >/dev/null 2>&1
[ $? -eq 2 ] && ok "--decoy-size 2M 被拒（退出码 2）" || bad "--decoy-size 2M 没被拒"
"$BIN" ys src -o bad3.sh --sfx-linux --decoy 4 >/dev/null 2>&1
[ $? -eq 2 ] && ok "--sfx 与 --decoy 互斥（退出码 2）" || bad "--sfx 与 --decoy 没报错"

echo
echo "  通过 $n_pass / 失败 $n_fail"
[ "$n_fail" -eq 0 ]
