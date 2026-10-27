#!/usr/bin/env bash
# RAR 原子落盘探针 —— 「解到一半被硬杀，会不会留下顶着正式名的半截文件」
# （阶段 1 梯队四第 12 项）
#
# 病根：RAR 是七条落盘路径里**最后一条**直接写正式名的（其它六条早就走 `PartFile`）。
# 解到一半被 `kill -9` / 断电 / 磁盘满时，磁盘上会留下**顶着正式文件名**的半截文件，
# 外观与好文件完全一样；重试又因「同名改名」策略绕开它，长出 `f3 (1).bin` —— 坏文件永远修不好。
#
# 这个探针盯四件事（缺一条都不算修好）：
#   ① 中断后：**没有任何「正式名 + 内容不对」的文件**（核心；改之前必然违反）
#   ② 中断后：只留 `.yshjy-part`，正式文件名根本没出现过
#   ③ 重试一次：文件全齐、内容与参照逐字节一致、**0 个 `(1)` 副本**、0 个 part 残留
#   ④ 自愈：手工删掉一个已解文件，重跑要能补回来
#
# 用法：bash tests/rar_atomic.sh [输入 MB，默认 150；想更稳可给 300]
# 依赖：rar（写侧，仅用于造样本）—— 没有就 `sudo apt install -y rar`

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MB=${1:-150}
BIN=${BIN:-target/debug/yshjy}
[ -x "$BIN" ] || { echo "先编译：cargo build（找不到 $BIN）" >&2; exit 2; }
command -v rar >/dev/null || {
  echo "需要 rar 来造样本：sudo apt install -y rar" >&2
  exit 2
}
command -v python3 >/dev/null || { echo "需要 python3" >&2; exit 2; }

# 二进制必须比源码新 —— 改完代码只跑 `cargo test` 是**不会**重建 target/debug/yshjy 的，
# 探针会拿旧二进制跑出旧症状（实测踩过：差点把"修复无效"当成结论，其实是没编译）
if [ -n "$(find src Cargo.toml -newer "$BIN" -print -quit 2>/dev/null)" ]; then
  echo "⚠️  源码比 $BIN 新 —— 先 cargo build，否则测的是旧行为" >&2
  exit 2
fi

T=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_raratomic.XXXXXX")
PASS=0
FAIL=0
FAILED=()

ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

echo "临时目录：$T"
echo "造样本：${MB} MB / 10 个文件（rar -m0 存储模式）"

# ── 造输入与样本 ──
mkdir -p "$T/输入"
each_mb=$((MB / 10))
[ "$each_mb" -lt 1 ] && each_mb=1
for i in $(seq 1 10); do
  head -c "${each_mb}M" /dev/urandom > "$T/输入/f$i.bin"
done
( cd "$T" && rar a -m0 -idq 包.rar 输入/ ) || { echo "造样本失败" >&2; exit 2; }
echo "样本：$(du -h "$T/包.rar" | cut -f1)"

# ── 参照清单（hash + 文件名，忽略层级）──
( cd "$T/输入" && for f in ./*.bin; do
    printf '%s %s %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "${f#./}" "$(stat -c%s "$f")"
  done | sort -k2 ) > "$T/参照.txt"

# 检查一个解压目录：返回 0 = 没问题，1 = 有半截正式名/内容不符
# 参数：目录 [要不要输出"文件齐不齐"]
check_dir() {
  python3 - "$1" "$T/参照.txt" "$2" <<'PY'
import hashlib, os, sys

目录, 参照路径, 严格模式 = sys.argv[1], sys.argv[2], sys.argv[3]

参照 = {}
参照大小 = {}
for line in open(参照路径, encoding="utf-8"):
    h, name, size = line.split()
    name = name.strip()
    参照[name] = h
    参照大小[name] = int(size)

实际 = {}
坏文件 = []
for 根, _, 文件们 in os.walk(目录):
    for 名 in 文件们:
        if 名.endswith(".yshjy-part"):
            continue
        全 = os.path.join(根, 名)
        if 严格模式 == "宽松":
            # 中断现场只要判「是不是半截」——半截的特征就是大小对不上，
            # 不必把几 GB 哈希一遍（机械盘上这一步能吃掉十几秒）
            实际[名] = os.path.getsize(全)
            if 名 in 参照大小 and 实际[名] != 参照大小[名]:
                坏文件.append(f"{名}({实际[名]}/{参照大小[名]})")
            continue
        h = hashlib.sha256()
        with open(全, "rb") as f:
            for 块 in iter(lambda: f.read(1 << 20), b""):
                h.update(块)
        实际[名] = h.hexdigest()
        if 名 in 参照 and 实际[名] != 参照[名]:
            坏文件.append(名)

问题 = []
if 坏文件:
    问题.append(f"这些文件顶着正式名但内容不对（半截文件）：{sorted(坏文件)}")
if 严格模式 == "严格":
    缺 = sorted(set(参照) - set(实际))
    多 = sorted(set(实际) - set(参照))
    if 缺:
        问题.append(f"缺文件：{缺}")
    if 多:
        问题.append(f"多出文件（副本/垃圾）：{多}")

if 问题:
    print("；".join(问题))
    sys.exit(1)
print(f"正式名文件 {len(实际)} 个，内容都对")
PY
}

count_suffix() { # count_suffix() 路径 后缀 → 数量
  find "$1" -maxdepth 3 -name "*$2" 2>/dev/null | wc -l
}

# ── ① 先量一次完整解要多久，中断点取 1/3（保证杀在写盘阶段）──
T0=$(date +%s%N)
"$BIN" -o "$T/参照解" "$T/包.rar" >/dev/null 2>&1
T1=$(date +%s%N)
full_ms=$(( (T1 - T0) / 1000000 ))
echo "完整解一次：${full_ms} ms"
if msg=$(check_dir "$T/参照解" 严格); then
  ok "完整解压内容与参照逐字节一致"
else
  bad "完整解压就出问题了：$msg"
fi
if [ "$(count_suffix "$T/参照解" '.yshjy-part')" -eq 0 ]; then
  ok "完整解压零 part 残留"
else
  bad "完整解压后还有 part 残留"
fi

# ── ② 中断（SIGKILL，硬杀）──
kill_s=$(awk -v ms="$full_ms" 'BEGIN{ half=ms/3000; if (half<0.15) half=0.15; printf "%.2f", half }')
echo "硬杀中断点：${kill_s} 秒（完整解的 1/3）"
timeout -s KILL "$kill_s" "$BIN" -o "$T/断" "$T/包.rar" >/dev/null 2>&1
echo "中断后 yshjy 退出码=$?（137 = 被 SIGKILL，符合预期）"

n_part=$(count_suffix "$T/断" '.yshjy-part')
n_real=$(find "$T/断" -maxdepth 3 -type f ! -name '*.yshjy-part' 2>/dev/null | wc -l)
echo "中断现场：part $n_part 个，正式名文件 $n_real 个"

if msg=$(check_dir "$T/断" 宽松); then
  ok "中断后没有任何「顶着正式名的半截文件」"
else
  bad "中断后留下半截正式名：$msg"
fi
# 这条是「修好了」的直接证据：以前杀在这里，n_real必然 > 0 且内容不对
if [ "$n_real" -eq 0 ]; then
  ok "中断后正式文件名一个都没出现（正确的原子写入行为）"
elif [ "$n_part" -gt 0 ]; then
  ok "中断后只留 part（正式名文件虽存在但内容完整，说明是条目边界处被杀）"
else
  bad "中断后既没 part 也没正式名，没法判断（中断点太早？加大 MB 重试）"
fi

# ── ③ 重试一次必须全齐、无副本 ──
"$BIN" -o "$T/断" "$T/包.rar" >/dev/null 2>&1
if msg=$(check_dir "$T/断" 严格); then
  ok "中断后重试：文件全齐、内容全对、零副本"
else
  bad "中断后重试不干净：$msg"
fi
if [ "$(count_suffix "$T/断" '.yshjy-part')" -eq 0 ]; then
  ok "重试完成后零 part 残留"
else
  bad "重试完成后还有 part 残留"
fi

# ── ④ 自愈：删掉一个已解文件，重跑要能补回来 ──
#
# 这里**只**断言「删掉的那个回来了」，不要求零副本：
# 目录里已有同名文件时，默认 `Rename` 策略会把它们复制成 `f2 (1).bin` —— 那是「重名策略」
# 的正常行为（想覆盖就把配置里的 `on_conflict` 改成 `replace`），不是本项的缺陷。
# 注意区分两种重跑：**中断后**重跑（正式名一个都没有）→ 零副本，实测过；
# **完整解完之后**再解一遍 → 9 个副本，属于用户主动重复操作。
first_one=$(cd "$T/断" && find . -name 'f1.bin' -type f | head -1)
if [ -n "$first_one" ]; then
  rm -f "$T/断/$first_one"
  "$BIN" -o "$T/断" "$T/包.rar" >/dev/null 2>&1
  if [ -n "$(find "$T/断" -name 'f1.bin' -type f | head -1)" ]; then
    ok "删掉的文件重跑后补回来了（自愈）"
  else
    bad "删文件后没自愈：f1.bin 没回来"
  fi
  n_dup=$(find "$T/断" -name '* (*).bin' | wc -l)
  echo "  ℹ 附带现象：产生 $n_dup 个 (1) 副本 —— 默认 Rename 策略的正常行为（想覆盖请把配置 on_conflict 改成 replace）"
else
  bad "找不到 f1.bin，自愈测不了"
fi

echo
echo "──────────────────────────────"
printf 'RAR 原子落盘探针：\033[32m%d 通过\033[0m' "$PASS"
[ "$FAIL" -gt 0 ] && printf ' / \033[31m%d 失败\033[0m' "$FAIL"
echo
if [ "$FAIL" -gt 0 ]; then
  echo "失败项："
  for f in "${FAILED[@]}"; do echo "  - $f"; done
  echo "现场留着排查：$T"
  exit 1
fi
rm -rf "$T"
echo "临时目录已清理"
exit 0
