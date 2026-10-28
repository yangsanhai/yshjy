#!/usr/bin/env bash
# 进度条探针（2026-10-28）：单任务等价 / -j 2 两行独立 / TERM=dumb 不沉默
#
# 由来：`-j 2` 时两行进度会互相覆盖，改用 indicatif 的 MultiProgress；
# 但 indicatif 在 `TERM=dumb` 下会**静默退化成 hidden**（`draw_target.rs` 的 `is_dumb()`），
# 一点进度都不画也不报错 —— 这条探针就是防止那种「沉默」再出现。
#
# 用法：bash tests/progress_multi.sh [yshjy 二进制路径]
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
import os, pty, select, shutil, subprocess, sys, time, zipfile
from pathlib import Path

BIN = sys.argv[1]
# ⚠️ 三个坑都踩过，样本必须放**机械盘**（项目内 target/），不能放 /tmp：
#   ① /tmp 是 tmpfs（内存盘）：造两个 700MB 包会把它吃到 97%、占掉 9.4G 内存；
#   ② 内存盘上解压等于纯内存拷贝 —— release 版解 200MB×2 只要 **0.26 秒**，
#      一次进度绘制都赶不上，探针会误报「没有进度」（debug 版慢才侥幸通过）；
#   ③ 样本数据必须是**随机**的：全零数据压缩比 230:1 会触发 zip 炸弹防护
#      （`safety.zip_bomb_ratio`），yshjy 会正确拒绝 —— 这不是 bug，是防护生效。
work = Path.cwd() / "target" / "test-progress"
work.mkdir(parents=True, exist_ok=True)

SIZE = 150 * 1024 * 1024
样本 = [work / f"{n}.zip" for n in ("包A", "包B")]
if not all(p.exists() and p.stat().st_size > 0 for p in 样本):
    for p in 样本:
        with zipfile.ZipFile(p, "w", zipfile.ZIP_DEFLATED, compresslevel=1) as zf:
            zf.writestr(f"{p.stem}-数据.bin", os.urandom(SIZE))

def run_pty(args, term, wait=120):
    master, slave = pty.openpty()
    env = dict(os.environ)
    env["TERM"] = term
    env.pop("YSHJY_DEBUG_PROGRESS", None)
    p = subprocess.Popen([BIN] + args, stdin=slave, stdout=slave, stderr=slave,
                         env=env, close_fds=True)
    os.close(slave)
    buf = b""
    end = time.time() + wait
    while time.time() < end:
        r, _, _ = select.select([master], [], [], 0.2)
        if r:
            try: c = os.read(master, 65536)
            except OSError: break
            if not c: break
            buf += c
        if p.poll() is not None:
            break
    try: p.wait(timeout=5)
    except subprocess.TimeoutExpired: p.kill()
    os.close(master)
    return p.returncode, buf.decode("utf-8", "replace")

def frames_of(text):
    return [l for l in text.replace("\r", "\n").split("\n")
            if "[█" in l or "[#" in l or "%" in l]

n_pass = n_fail = 0
def ok(m):
    global n_pass; n_pass += 1; print(f"  ✓ {m}")
def bad(m):
    global n_fail; n_fail += 1; print(f"  ✗ {m}")

# ── ① 单任务：输出与改前等价（有进度帧、含文件名）──
code, text = run_pty([str(work/"包A.zip"), "-o", str(work/"out1")], "xterm-256color")
fr = frames_of(text)
if code == 0 and fr and any("包A" in l for l in fr):
    ok(f"单任务有进度帧（{len(fr)} 帧，含文件名）")
else:
    bad(f"单任务没进度：退出码 {code}，帧 {len(fr)}")

# ── ② -j 2 且 TERM 正常：两个任务各自有行（不再互相覆盖）──
code, text = run_pty(["-j", "2", str(work/"包A.zip"), str(work/"包B.zip"),
                      "-o", str(work/"out2")], "xterm-256color")
fr = frames_of(text)
a = sum(1 for l in fr if "包A" in l)
b = sum(1 for l in fr if "包B" in l)
if code == 0 and a > 0 and b > 0:
    ok(f"-j 2 两个任务各有进度行（包A {a} 帧 / 包B {b} 帧）")
else:
    bad(f"-j 2 两行没画出来：退出码 {code}，包A {a} 帧 / 包B {b} 帧")

# ── ③ TERM=dumb：必须退回单行，不能一片沉默 ──
code, text = run_pty(["-j", "2", str(work/"包A.zip"), str(work/"包B.zip"),
                      "-o", str(work/"out3")], "dumb")
fr = frames_of(text)
if code == 0 and len(fr) > 0:
    ok(f"TERM=dumb 退回单行手绘（{len(fr)} 帧），不是沉默")
else:
    bad(f"TERM=dumb 下完全没有进度（indicatif 静默退化成 hidden 那个坑）")

# ── ④ 非 TTY（管道）：一行进度都不该画 ──
r = subprocess.run([BIN, "-j", "2", str(work/"包A.zip"), str(work/"包B.zip"),
                    "-o", str(work/"out4")], capture_output=True, text=True)
markers = [m for m in ("[█", "[#", "\r") if m in (r.stdout + r.stderr)]
if not markers:
    ok("管道模式一行进度都不画（不污染日志）")
else:
    bad(f"管道模式里出现了进度字符：{markers}")

# 只清输出，样本留着给下次跑（造 150MB 随机包要好几秒，能省就省）
for d in work.glob("out*"):
    shutil.rmtree(d, ignore_errors=True)
print(f"\n  通过 {n_pass} · 失败 {n_fail}")
sys.exit(1 if n_fail else 0)
PY
