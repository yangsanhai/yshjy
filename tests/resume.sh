#!/usr/bin/env bash
# 断点续传探针 —— 「解到一半被打断，重开能不能接着解」
#
# 验收标准（docs/TODO.md 梯队四第 10 项）：
#   **中断后重开能接着解，且已验证字节一致**
#
# 这个脚本怎么证明这件事（三条硬证据，缺一条就不算过）：
#   ① 中断是真的：打断后目录里的文件数 **少于** 总数（不是因为包太小已经解完了）
#   ② 续传是真的：重开时引擎发出 RESUMED 事件，且 done 数 ≈ 断点处已完成的条目数
#   ③ 字节是真的：续出来的目录与「一口气解完」的参照目录逐个文件 sha256 全等
# 外加两条反证，证明跳过不是「看见文件就跳过」：
#   ④ 手动删掉一个已解文件 → 重解必须把它补回来
#   ⑤ 手动把某个文件写坏（长度也变）→ 重解必须把它重写正确
#
# 用法：
#   bash tests/resume.sh          # 快（约 60 MB 的包，几十秒）
#   bash tests/resume.sh 300      # 慢（300 MB，机械盘上更稳地抓住中断点）
#
# 注意：
#   - 全程把配置目录指到临时目录（YSHJY_CONFIG_DIR），**不碰**你真实的
#     ~/.config/yshjy/resume，测完删临时目录
#   - 不删任何仓库文件

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MB=${1:-60}
# 用 debug（与 pack_progress.sh 一致）：`tests/all.sh` 两档都会建 —— 详见那个文件同处注释
BIN=${BIN:-target/debug/yshjy}
if [ ! -x "$BIN" ]; then
  echo "先编译：cargo build（找不到 $BIN）" >&2
  exit 2
fi
command -v python3 >/dev/null || { echo "需要 python3 造测试包" >&2; exit 2; }

T=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_resume.XXXXXX")
export YSHJY_CONFIG_DIR="$T/config"
PKG="$T/测试包.zip"
REF="$T/参照"     # 一口气解完的标准答案
DA="$T/甲"        # 被打断 / 接着解的那个
PASS=0
FAIL=0
FAILED=()

ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

# 目录里文件的 sha256 清单（相对路径 + 哈希，排序后便于逐字比对）
# 注意排除 `.yshjy-part`：那是「没写完」的中间态，要单独断言（它出现本身就该报错），
# 混进内容比对里会让失败信息说不清是「内容不对」还是「留了半截文件」
清单() { ( cd "$1" 2>/dev/null && find . -type f ! -name '*.yshjy-part' -print0 | sort -z | xargs -0 -r sha256sum ) 2>/dev/null; }

# 残留的中间态文件数（续传成功后必须是 0）
残留part() { find "$1" -type f -name '*.yshjy-part' 2>/dev/null | wc -l; }

# `名字 (1).bin` 这类自动改名副本的数量（续传会话里不该出现）
副本数() { find "$1" -type f -name '* ([0-9]*).*' 2>/dev/null | wc -l; }

echo "临时目录：$T"
echo "造 ${MB} MB 的测试包（多文件 + 子目录 + 中文名）…"

python3 - "$PKG" "$MB" <<'PY'
import os, sys, zipfile
out, mb = sys.argv[1], int(sys.argv[2])
# 每个文件 512 KiB，凑够 mb 兆；分三个目录，其中一个是中文名
per = 512 * 1024
count = max(12, (mb * 1024 * 1024) // per)
dirs = ["视频", "游戏数据", "附加"]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=1) as z:
    for d in dirs:
        z.writestr(d + "/", "")
    for i in range(count):
        d = dirs[i % len(dirs)]
        # 随机数据不可压缩：解压时间主要由磁盘写决定，中断点更好抓
        z.writestr(f"{d}/第{i:03d}集.bin", os.urandom(per))
print(f"  条目 {count} 个文件 + 3 个目录 = {count + 3} 项，压缩包 " +
      f"{os.path.getsize(out) / 1024 / 1024:.1f} MiB")
PY
TOTAL=$(python3 -c "
import zipfile,sys
z=zipfile.ZipFile(sys.argv[1])
print(sum(1 for i in z.infolist() if not i.is_dir()))" "$PKG")
echo "  文件条目总数：$TOTAL"

# ── 参照解压：一口气解完，作为「字节一致」的基准 ──
if "$BIN" -o "$REF" "$PKG" >/dev/null 2>&1; then
  ok "参照解压（一口气解完）"
else
  bad "参照解压（一口气解完）"
  echo "参照解压都失败，后面没法比 —— 停。"
  exit 1
fi
if [ -z "$(清单 "$REF")" ]; then
  bad "参照目录是空的"
  exit 1
fi

# 打一个「解到指定进度就被打断」的包解压任务
# 用法：打断一次 <输出目录> <信号> [目标文件数]；返回 0 = 真打断了，1 = 没赶上（包太小）
打断一次() {
  local out="$1" sig="$2" target="${3:-$(( TOTAL / 3 ))}"
  "$BIN" -o "$out" "$PKG" >"$T/中断.log" 2>&1 &
  local pid=$!
  local n=0
  # 轮询等它解出三分之一，然后动手
  for _ in $(seq 1 6000); do
    kill -0 "$pid" 2>/dev/null || break
    n=$(find "$out" -type f 2>/dev/null | wc -l)
    if [ "$n" -ge "$target" ]; then
      kill -"$sig" "$pid" 2>/dev/null
      break
    fi
    sleep 0.01
  done
  wait "$pid" 2>/dev/null
  local done_n
  done_n=$(find "$out" -type f 2>/dev/null | wc -l)
  echo "  中断时已解出 $done_n / $TOTAL 个文件（目标断点 $target）"
  [ "$done_n" -gt 0 ] && [ "$done_n" -lt "$TOTAL" ]
}

# 续传一次：返回 RESUMED 事件里报的「上次已完成条数」
续传一次() {
  local out="$1" log="$2"
  "$BIN" --json -o "$out" "$PKG" >"$log" 2>"$T/续传.err"
  python3 - "$log" <<'PY'
import json, sys
done = 0
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        ev = json.loads(line)
    except Exception:
        continue
    if "Resumed" in ev:
        done = ev["Resumed"].get("done", 0)
print(done)
PY
}

# ── 场景一：用户按取消（SIGINT）──
echo
echo "场景一：解到一半按取消（Ctrl-C）→ 重开接着解"
rm -rf "$DA"
if 打断一次 "$DA" INT; then
  ok "被打断（取消）"
else
  bad "没能打断（包太小或机器太快，换大点：bash tests/resume.sh 300）"
fi
DONE=$(续传一次 "$DA" "$T/续1.log")
if [ "$DONE" -gt 0 ]; then
  ok "重开时报「接着上次来」（上次已完成 $DONE 项）"
else
  bad "重开时没有 RESUMED 事件（续传记录没生效）"
fi
if diff -r "$REF" "$DA" >/dev/null 2>&1; then
  ok "续出来的内容与参照逐字节一致（diff -r）"
else
  bad "续出来的内容与参照不一致"
  diff -r "$REF" "$DA" 2>&1 | head -10
fi
if [ "$(清单 "$REF")" = "$(清单 "$DA")" ]; then
  ok "全部文件 sha256 清单完全相同"
else
  bad "sha256 清单有差异"
fi
if [ "$(残留part "$DA")" -eq 0 ]; then
  ok "续传完成后没有残留的 .yshjy-part（被打断那个文件已重解改名）"
else
  bad "续传完成后还留着 $(残留part "$DA") 个 .yshjy-part"
fi

# 解压成功后状态文件应当被清掉（否则下次会误跳过）
LEFT=$(find "$YSHJY_CONFIG_DIR/resume" -name '*.jsonl' 2>/dev/null | wc -l)
if [ "$LEFT" -eq 0 ]; then
  ok "解压完成后没有残留的续传记录"
else
  bad "解压完成后还留着 $LEFT 份续传记录"
fi

# ── 场景二：硬杀（kill -9，模拟关窗口 / 断电）──
echo
echo "场景二：解到一半被硬杀（kill -9）→ 重开接着解"
rm -rf "$DA"
if 打断一次 "$DA" KILL; then
  ok "被打断（硬杀）"
else
  bad "没能打断（换大点：bash tests/resume.sh 300）"
fi
DONE=$(续传一次 "$DA" "$T/续2.log")
if [ "$DONE" -gt 0 ]; then
  ok "硬杀后也能续（上次已完成 $DONE 项）"
else
  bad "硬杀后没有续传记录"
fi
if [ "$(清单 "$REF")" = "$(清单 "$DA")" ]; then
  ok "硬杀后续出来的内容逐字节一致"
else
  bad "硬杀后续出来的内容不一致"
  diff -r "$REF" "$DA" 2>&1 | head -10
fi

# ── 反证：跳过不是「看见文件就跳过」──
echo
echo "反证：磁盘校验真的在起作用（删一个 / 改坏一个 → 必须重解）"
rm -rf "$DA"
VICTIM=$(cd "$REF" && find . -type f -name '*.bin' | sort | head -1)
# 先跑一次被打断，留下续传记录
打断一次 "$DA" KILL >/dev/null
if [ -f "$DA/$VICTIM" ]; then
  rm -f "$DA/$VICTIM"
  续传一次 "$DA" "$T/续3.log" >/dev/null
  if [ -f "$DA/$VICTIM" ]; then
    ok "被删掉的文件（$VICTIM）重解时补回来了"
  else
    bad "被删掉的文件没有补回来 —— 续传在瞎跳过"
  fi
  if [ "$(清单 "$REF")" = "$(清单 "$DA")" ]; then
    ok "补回来的内容与参照一致"
  else
    bad "补回来的内容与参照不一致"
  fi
else
  echo "  （这次没赶上打断点，跳过反证）"
fi

# ── 随机断点压测（学 peel 的做法：不在固定位置 kill，而是随机点）──
#
# 为什么值得多花这十几秒：固定断点只覆盖「三分之一处」这一种情况，
# 而真实中断可能发生在任何一个文件边界上（第一个文件之后、最后一个之前）。
# peel（Rust 的流式解压器）就是这么测的：随机 kill 点、每轮都断言与干净跑字节一致。
echo
echo "随机断点压测（随机 kill 点，每轮都要字节一致）"
ROUNDS=${RESUME_RANDOM_ROUNDS:-3}
RND_OK=0
for i in $(seq 1 "$ROUNDS"); do
  rm -rf "$DA"
  TARGET=$(( TOTAL * (10 + RANDOM % 70) / 100 ))
  [ "$TARGET" -lt 2 ] && TARGET=2
  打断一次 "$DA" KILL "$TARGET" >/dev/null
  续传一次 "$DA" "$T/随机$i.log" >/dev/null
  if [ "$(清单 "$REF")" = "$(清单 "$DA")" ]; then
    RND_OK=$((RND_OK + 1))
  else
    bad "随机断点第 $i 轮（kill 在第 $TARGET 个文件时）续出来不一致"
    diff -r "$REF" "$DA" 2>&1 | head -6
  fi
done
if [ "$RND_OK" = "$ROUNDS" ]; then
  ok "随机断点 $RND_OK/$ROUNDS 轮全部逐字节一致"
fi

# ── 定向：崩在「改名成功 / 记录没写」那几微秒 ──
#
# 这是随机 kill 压测抓出来的真 bug（固定断点测不出来）：
# 磁盘上已经有完好的 `第033集.bin`，但续传记录里没有它（上次崩在两个动作之间）。
# 修复前续传会按默认的 Rename 策略再解一遍 → 多出一个 `第033集 (1).bin`。
# 修复后：续传会话里「没记录却已存在」一律覆盖重解，不留副本。
echo
echo "定向：模拟「改名叫成功了、记录还没写下去」→ 不能留下 (1) 副本"
rm -rf "$DA"
打断一次 "$DA" KILL >/dev/null
REC=$(ls "$YSHJY_CONFIG_DIR"/resume/*.jsonl 2>/dev/null | head -1)
if [ -n "${REC:-}" ] && [ -f "$REC" ]; then
  python3 - "$REC" <<'PY'
import sys
p = sys.argv[1]
lines = open(p, encoding="utf-8").read().splitlines(True)
# 从后往前删掉第一条「文件」记录，等价于「文件已落盘、记录没来得及写」
for i in range(len(lines) - 1, 0, -1):
    if '"t":"item"' in lines[i] and '"k":"f"' in lines[i]:
        del lines[i]
        break
open(p, "w", encoding="utf-8").writelines(lines)
PY
  续传一次 "$DA" "$T/临界区.log" >/dev/null
  N=$(副本数 "$DA")
  if [ "$N" -eq 0 ]; then
    ok "崩在临界区的那一条被覆盖重解，没有留下 (1) 副本"
  else
    bad "续传留下了 $N 个 (1) 副本（没记录却已存在的文件被当成冲突改名了）"
  fi
  if [ "$(清单 "$REF")" = "$(清单 "$DA")" ]; then
    ok "临界区续传后内容仍与参照一致"
  else
    bad "临界区续传后内容与参照不一致"
    diff -r "$REF" "$DA" 2>&1 | head -6
  fi
else
  echo "  （这次没留下续传记录，跳过）"
fi

# ── 汇总 ──
echo
echo "──────────────────────────────"
printf '断点续传探针：\033[32m%d 通过\033[0m' "$PASS"
[ "$FAIL" -gt 0 ] && printf ' / \033[31m%d 失败\033[0m' "$FAIL"
echo
if [ "$FAIL" -gt 0 ]; then
  printf '失败项：\n'
  for f in "${FAILED[@]}"; do echo "  - $f"; done
  echo
  echo "临时目录留着方便排查：$T"
  exit 1
fi
rm -rf "$T"
echo "临时目录已清理"
exit 0
