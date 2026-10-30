#!/usr/bin/env bash
# 移动设备落盘检验探针（2026-10-30，用户拍板的方案）
#
# 为什么要这条探针
# ────────────────
# 2026-10-29 真机实测：解 300 MB 到 U 盘，屏幕报「✅ 解开（用时 0.3 秒）」，
# 此刻内核里 Dirty 307 MB / Writeback 0 —— 拔盘后一个文件不剩。
# 用户 2026-10-30 定的方案：**默认关**的开关，打开后只在移动设备上等真落盘，
# 并要求给进度条（不让人干等）。这条探针守的就是这个方案的四条边界：
#
#   ① 本机硬盘 + 开关关  → **绝不**发 FLUSH 事件（一秒都不多等）
#   ② 本机硬盘 + 开关开  → **仍然绝不**发（开关不是万能开关）
#   ③ 移动设备 + 开关关  → 不发 FLUSH，但**必须**发 MOVABLE_NOT_VERIFIED 诊断
#                          （开关关着的人照样会「看到 ✅ 就拔」）
#   ④ 移动设备 + 开关开  → 发 FLUSH_FINISHED，结局 DONE；解压用时**不含**等待
#
# 没有可用的移动设备时：明说「跳过」，退出码 0 —— **不许假装通过**。
#
# 用法：bash tests/flush_movable.sh [yshjy 二进制路径]
set -u
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
    for c in target/release/yshjy target/debug/yshjy; do
        [ -x "$c" ] && BIN="$c" && break
    done
fi
[ -n "$BIN" ] || { echo "❌ 找不到 yshjy 二进制"; exit 1; }
if [ -n "$(find src -name '*.rs' -newer "$BIN" -print -quit)" ]; then
    echo "❌ $BIN 比源码旧，先 cargo build"
    exit 1
fi

python3 - "$BIN" <<'PY'
import json, os, subprocess, sys, time, zipfile
from pathlib import Path

BIN = sys.argv[1]
SIZE = 12 * 1024 * 1024  # 12 MB：够跨过页缓存，又不至于让慢盘等太久

# ── 造样本（放项目内 target/，别放 /tmp：tmpfs 会骗速度，见 progress_multi.sh）──
work = Path.cwd() / "target" / "test-flush"
work.mkdir(parents=True, exist_ok=True)
sample = work / "落盘样本.zip"
if not sample.exists():
    with zipfile.ZipFile(sample, "w", zipfile.ZIP_STORED) as zf:
        zf.writestr("数据.bin", os.urandom(SIZE))

cfg_off = work / "off.toml"
cfg_off.write_text('[safety]\nflush_movable = "off"\n', encoding="utf-8")
cfg_on = work / "on.toml"
cfg_on.write_text('[safety]\nflush_movable = "on"\n', encoding="utf-8")


def removable_mounts():
    """可写的移动设备挂载点（判据与引擎 src/flush.rs 一致：usb 总线段 或 removable=1）"""
    seen, out = set(), []
    for line in open("/proc/self/mountinfo", encoding="utf-8"):
        f = line.split()
        if len(f) < 5:
            continue
        majmin, mp = f[2], f[4].replace("\\040", " ")
        if mp in seen:
            continue
        seen.add(mp)
        real = ""
        try:
            real = os.path.realpath(f"/sys/dev/block/{majmin}")
        except OSError:
            pass
        usb = "/usb" in real
        rm = "0"
        if real:
            for cand in (os.path.join(os.path.dirname(real), "removable"),
                         os.path.join(real, "removable")):
                if os.path.exists(cand):
                    rm = open(cand).read().strip()
                    break
        if (usb or rm == "1") and os.access(mp, os.W_OK):
            out.append(mp)
    return out


def run(cfg, dest):
    """跑一次解压，返回 (退出码, [(kind, data), ...], 耗时秒)"""
    t0 = time.time()
    p = subprocess.run(
        [BIN, "--config", str(cfg), str(sample), "-o", str(dest), "--json"],
        capture_output=True, text=True, timeout=900,
    )
    # ⚠️ --json 吐的是 serde 默认的**外部标签**格式：{"FlushFinished": {...}}
    # （GUI 走的是 --serve 的 {"kind":..,"data":..}，两套别搞混 —— 这里踩过一次，
    #  探针自己解析错，误报「引擎没发事件」）
    evs = []
    for ln in p.stdout.splitlines():
        ln = ln.strip()
        if not ln.startswith("{"):
            continue
        try:
            j = json.loads(ln)
        except json.JSONDecodeError:
            continue
        if not isinstance(j, dict) or len(j) != 1:
            continue  # 最后一行是汇总（cancelled/engine/jobs…），不是事件
        k, v = next(iter(j.items()))
        if isinstance(v, dict):
            evs.append((k, v))
    return p.returncode, evs, time.time() - t0


def kinds(evs):
    return [k for k, _ in evs]


fails, ran = [], []

# ── ①② 本机硬盘：开关关 / 开关开，都不许有 FLUSH ──
for tag, cfg in (("关", cfg_off), ("开", cfg_on)):
    dest = work / f"本地-{tag}"
    rc, evs, secs = run(cfg, dest)
    ks = kinds(evs)
    n = sum(1 for k in ks if k.startswith("Flush"))
    ran.append(f"本机硬盘+开关{tag}")
    if rc != 0:
        fails.append(f"本机硬盘+开关{tag}：解压失败（退出码 {rc}）")
    elif n:
        fails.append(f"本机硬盘+开关{tag}：不该出现 {n} 条 FLUSH 事件（本机盘一秒都不该等）")
    else:
        print(f"✅ 本机硬盘 + 开关{tag}：0 条 FLUSH 事件（{secs:.1f} 秒，没白等）")

# ── 移动设备 ──
mounts = removable_mounts()
if not mounts:
    print("⏭  跳过 ③④：没找到可写的移动设备（插上 U 盘再跑这条）")
    print(f"\n本机部分 {len(ran) - len(fails)}/{len(ran)} 通过")
    sys.exit(1 if fails else 0)

mp = mounts[0]
stamp = time.strftime("%H%M%S")
print(f"👉 移动设备：{mp}")

# ③ 开关关：不许等，但必须有诊断提醒
dest = Path(mp) / f"yshjy-探针-关-{stamp}"
rc, evs, secs = run(cfg_off, dest)
ks = kinds(evs)
n_flush = sum(1 for k in ks if k.startswith("Flush"))
has_diag = any(
    k == "Diagnostic" and (d.get("code") or "").upper().replace("_", "") == "MOVABLENOTVERIFIED"
    for k, d in evs
)
ran.append("移动设备+开关关")
if n_flush:
    fails.append(f"移动设备+开关关：不该有 FLUSH 事件（开关关着就不该等），实际 {n_flush} 条")
elif not has_diag:
    fails.append("移动设备+开关关：缺 MOVABLE_NOT_VERIFIED 诊断（开关关着也得提醒先安全弹出）")
else:
    print(f"✅ 移动设备 + 开关关：没等（{secs:.1f} 秒），但发了「拔盘前先安全弹出」诊断")
print(f"   测试目录：{dest}")

# ④ 开关开：必须等完再报成功，且解压用时不含等待
dest = Path(mp) / f"yshjy-探针-开-{stamp}"
rc, evs, secs = run(cfg_on, dest)
ks = kinds(evs)
fin = [(k, d) for k, d in evs if k == "FlushFinished"]
ran.append("移动设备+开关开")
if not fin:
    fails.append("移动设备+开关开：一条 FLUSH_FINISHED 都没有（开关开了却没检验）")
else:
    outcome = (fin[-1][1].get("outcome") or "").upper()
    waited = fin[-1][1].get("millis", 0) or 0
    if outcome != "DONE":
        fails.append(f"移动设备+开关开：结局是 {outcome}，期望 DONE")
    else:
        print(f"✅ 移动设备 + 开关开：检验通过，等 {waited / 1000:.1f} 秒才报成功")

    # ⑤ 屏幕上那个「用时」必须**不含**等待写盘的时间
    #    历史 bug：屏幕上写「用时 0.3 秒」，其实数据全在缓存里 —— 假话。
    #    现在「解压用时」和「检验用时」必须分开说，两个数都不能糊弄。
    import re
    dest2 = Path(mp) / f"yshjy-探针-用时-{stamp}"
    p2 = subprocess.run(
        [BIN, "--config", str(cfg_on), str(sample), "-o", str(dest2)],
        capture_output=True, text=True, timeout=900,
    )
    txt = p2.stdout + p2.stderr
    m_use = re.search(r"用时\s*([0-9.]+)\s*秒", txt)
    m_chk = re.search(r"检验\s*([0-9.]+)\s*秒", txt)
    if not (m_use and m_chk):
        fails.append(f"屏幕输出里没同时出现「用时」和「检验」两个数：{txt.strip()[:200]}")
    else:
        use, chk = float(m_use.group(1)), float(m_chk.group(1))
        if chk >= 0.2 and use >= chk:
            fails.append(f"「用时 {use} 秒」把等待写盘的 {chk} 秒也算进去了（两个数必须分开）")
        else:
            print(f"   ✅ 屏幕两个数分开：解压用时 {use} 秒 / 检验 {chk} 秒")
print(f"   测试目录：{dest}")

print()
if fails:
    for f in fails:
        print(f"❌ {f}")
    sys.exit(1)
print(f"OK {len(ran)}/{len(ran)} 通过")
PY
