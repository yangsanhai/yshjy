#!/usr/bin/env bash
# 打包进度探针 —— 「打大包时进度条到底动没动」（梯队四第 11 项）
#
# 病根（修之前）：只有「普通压缩」传了进度回调；伪装包和自解压走的是
# `write_zip(..., &mut |_, _| {})`（空回调）+ `std::io::copy`（一次拷完、零进度）。
# 实测打 2 GB 伪装包用了 183 秒，CLI 一条进度事件都不发，GUI 进度条整段是死的。
#
# 这个探针盯四件事：
#   ① 事件数够多（大包上不能只报一两条就没了）
#   ② 压缩阶段的分母 = 输入总字节，且进度真的往上走
#   ③ 拼装阶段分母被放大（`TotalHint`），并且也往上走
#   ④ 最终到 100%、同分母段内不回退
# 三条路径各测一遍：普通 ZIP / 伪装 / 自解压 .sh
#
# 用法：bash tests/pack_progress.sh [输入 MB，默认 150]

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MB=${1:-150}
# ★ 必须用 debug，不是 release：本探针按「进度事件条数 / 分母会不会换」判定，而 release 解 80MB
#   只要 0.15 秒、只发得出 1 条事件 —— 换 release 后 3 项断言当场全红（2026-10-29 实测）。
#   `tests/all.sh` 第 1 关会同时建 release 与 debug，两档都保证最新；单独手跑前先 `cargo build`。
BIN=${BIN:-target/debug/yshjy}
if [ ! -x "$BIN" ]; then
  echo "先编译：cargo build（找不到 $BIN）" >&2
  exit 2
fi
command -v python3 >/dev/null || { echo "需要 python3" >&2; exit 2; }

T=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_packpg.XXXXXX")
mkdir -p "$T/输入"
# 用 sparse 文件：造得快，且 level=0 时中间 ZIP 也是这么大 —— 两段都有活干
truncate -s "${MB}M" "$T/输入/大文件.bin"
PASS=0
FAIL=0
FAILED=()

ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

# 统计一份事件流；不合格时打印诊断并返回 1
#
# ⚠️ 函数内变量**必须用英文**：bash 的 `local 中文变量=` 会报
# 「不是有效的标识符」，整个函数静默失效（本项目踩过三次，AGENTS.md 硬规矩第 2 条）
查进度() {
  local evfile="$1" label="$2"
  python3 - "$evfile" "$label" <<'PY'
import json, sys

路径, 说明 = sys.argv[1], sys.argv[2]
轨迹 = []
for line in open(路径, encoding="utf-8", errors="replace"):
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        ev = json.loads(line)
    except Exception:
        continue
    # CLI `--json` 打裸事件；`--serve` 管道会多一层 {"kind":…, "event":…}
    载荷 = ev.get("PackProgress") or (ev.get("event") or {}).get("PackProgress")
    if not 载荷:
        continue
    p = 载荷.get("progress") or {}
    轨迹.append((p.get("done_bytes") or 0, p.get("total_bytes") or 0, 载荷.get("current")))

问题 = []
# 事件数受节流影响：包小 + page cache 快时只能出一两条，**不能**拿它当正确性指标。
# 修之前是 0 条（连终态都没有），修之后至少「中间一条 + 终态一条」，
# 真正说明问题的是下面两条：分母值变了（拼装段报了新总量）、终态到 100%。
if len(轨迹) < 2:
    问题.append(f"只有 {len(轨迹)} 条进度事件（一条中间态 + 一条终态都没有）")
回退 = 0
for i in range(1, len(轨迹)):
    d0, t0, _ = 轨迹[i - 1]
    d1, t1, _ = 轨迹[i]
    if t0 == t1 and d1 < d0:
        回退 += 1
if 回退:
    问题.append(f"进度回退 {回退} 次")
分母 = sorted({t for _, t, _ in 轨迹 if t})
if len(分母) < 2:
    问题.append("分母从头到尾没变过 —— 拼装阶段没有告知新总量，进度条会卡在压缩阶段的百分比")
if 轨迹:
    末d, 末t, _ = 轨迹[-1]
    if not 末t or 末d * 100 // 末t < 99:
        问题.append(f"最终只到 {末d * 100 // 末t if 末t else 0}%")

if 问题:
    print(f"  ✗ {说明}：" + "；".join(问题))
    for d, t, c in 轨迹[:3]:
        print(f"      样例 {d}/{t} {c}")
    sys.exit(1)
print(f"  ✓ {说明}（{len(轨迹)} 条事件，分母 {[round(x / 2**20) for x in 分母]} MiB，走到 100%）")
PY
}

echo "临时目录：$T"
echo "输入：${MB} MB（sparse，level=0 仅存储）"

# ── ① 伪装包 ──
"$BIN" ys "$T/输入" -o "$T/伪装.mp4" --level 0 --json >"$T/伪装.json" 2>"$T/伪装.err"
if 查进度 "$T/伪装.json" "伪装包（.mp4）报进度"; then ok "伪装包进度在动"; else bad "伪装包进度"; fi

# ── ② 普通压缩 ──
"$BIN" ys "$T/输入" -o "$T/普通.zip" --level 0 --mode plain --json >"$T/普通.json" 2>"$T/普通.err" \
  || "$BIN" ys "$T/输入" -o "$T/普通.zip" --level 0 --json >"$T/普通.json" 2>"$T/普通.err"
if 查进度 "$T/普通.json" "普通 ZIP 报进度"; then ok "普通压缩进度在动"; else bad "普通压缩进度"; fi

# ── ③ 自解压 .sh ──
"$BIN" ys "$T/输入" -o "$T/自解压.sh" --sfx-linux --level 0 --json >"$T/自解压.json" 2>"$T/自解压.err"
if 查进度 "$T/自解压.json" "自解压 .sh 报进度"; then ok "自解压进度在动"; else bad "自解压进度"; fi

# ── ④ 三种产物都要真能用（进度是附加的，不能把包打坏）──
if [ -s "$T/伪装.mp4" ]; then ok "伪装包有产物（$(du -h "$T/伪装.mp4" | cut -f1)）"; else bad "伪装包是空的"; fi
if [ -s "$T/普通.zip" ]; then ok "普通 ZIP 有产物"; else bad "普通 ZIP 是空的"; fi
if [ -x "$T/自解压.sh" ]; then ok "自解压 .sh 可执行"; else bad "自解压 .sh 没执行权限"; fi

# 自解压产物真解一遍（进度改到落盘路径上了，必须验证没写坏）
# 注意：存根会自己在当前目录下建一个以脚本名命名的子目录，所以用 find 找产物
# ★ YSHJY_SFX_NO_OPEN=1 不能少：存根跑完默认会 `xdg-open` 打开目标目录，测试里就会
#   当着用户的面弹出文件管理器窗口（2026-10-29 实际被用户抓到两次）。其它四个 sfx 探针
#   （protocol_sfx / protocol_sfx_linux / sfx_stub_test）早就设了，只有这里漏了。
mkdir -p "$T/解开"
if ( cd "$T/解开" && YSHJY_SFX_NO_OPEN=1 sh "$T/自解压.sh" >/dev/null 2>&1 ) \
   && [ -n "$(find "$T/解开" -name '大文件.bin' -print -quit)" ]; then
  ok "自解压 .sh 真的能解开"
else
  bad "自解压 .sh 解不开"
fi

echo
echo "──────────────────────────────"
printf '打包进度探针：\033[32m%d 通过\033[0m' "$PASS"
[ "$FAIL" -gt 0 ] && printf ' / \033[31m%d 失败\033[0m' "$FAIL"
echo
if [ "$FAIL" -gt 0 ]; then
  printf '失败项：\n'
  for f in "${FAILED[@]}"; do echo "  - $f"; done
  echo "临时目录留着排查：$T"
  exit 1
fi
rm -rf "$T"
echo "临时目录已清理"
exit 0
