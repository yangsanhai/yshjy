#!/usr/bin/env bash
# --pick 全屏勾选探针（2026-10-28）
#
# 验收标准（docs/CLI-GUI-PLAN.md §2）：
#   ① 勾选结果与 --select "2,5,9" 逐字节一致
#   ② 管道 / 非交互终端里跑不进全屏，给出降级提示
#   ③ 中文文件名不错行
#   ④ 退出后终端复原（stty -a 与进入前一致）
#
# 为什么用 python 开 pty：dialoguer 要真终端才肯画界面 ——
# 普通子进程的 stdin 不是 TTY，测不到真实按键路径。
#
# 用法：bash tests/pick.sh [yshjy 二进制路径]
set -u
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
    for c in target/release/yshjy target/debug/yshjy; do
        [ -x "$c" ] && BIN="$c" && break
    done
fi
[ -n "$BIN" ] || { echo "❌ 找不到 yshjy 二进制"; exit 1; }
# 源码比二进制新就拒跑（这个坑踩过：拿旧二进制测出「修复无效」的假象）
if [ -n "$(find src -name '*.rs' -newer "$BIN" -print -quit)" ]; then
    echo "❌ $BIN 比源码旧，先 cargo build"
    exit 1
fi

python3 - "$BIN" <<'PY'
import os, pty, select, shutil, subprocess, sys, time, zipfile
from pathlib import Path

BIN = sys.argv[1]
work = Path("/tmp/yshjy_pick_test")
shutil.rmtree(work, ignore_errors=True)
work.mkdir(parents=True)

# 造样本：3 个文件，中文名（对应验收③）
src = work / "src"
(src / "遊戲數據").mkdir(parents=True)
(src / "文档").mkdir()
(src / "文档" / "说明.txt").write_text("说明")
(src / "文档" / "大文件.bin").write_bytes(b"A" * 200_000)
(src / "遊戲數據" / "存档.sav").write_bytes(b"B" * 1000)
zip_path = work / "中文包.zip"
with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
    for p in sorted(src.rglob("*")):
        if p.is_file():
            z.write(p, p.relative_to(src))

n_pass = n_fail = 0
def ok(m): 
    global n_pass; n_pass += 1; print(f"  ✓ {m}")
def bad(m):
    global n_fail; n_fail += 1; print(f"  ✗ {m}")

# ── ① 非 TTY 必须拒绝（验收②）──
r = subprocess.run([BIN, "--pick", str(zip_path)], input="", capture_output=True, text=True)
msg = r.stdout + r.stderr
if r.returncode == 2 and "--select" in msg:
    ok("管道里拒绝全屏并提示改用 --select（退出码 2）")
else:
    bad(f"非 TTY 没拒绝：退出码 {r.returncode}，输出 {msg.strip()[:80]}")

# ── ② pty 里勾第 1、3 项，与 --select 对比（验收①③）──
def run_pick(keys, out_dir):
    master, slave = pty.openpty()
    p = subprocess.Popen([BIN, "--pick", str(zip_path), "-o", str(out_dir)],
                         stdin=slave, stdout=slave, stderr=slave, close_fds=True)
    os.close(slave)
    time.sleep(0.9)
    buf = b""
    def drain(t=0.3):
        nonlocal buf
        end = time.time() + t
        while time.time() < end:
            rready, _, _ = select.select([master], [], [], 0.1)
            if rready:
                try: buf += os.read(master, 65536)
                except OSError: return
    drain()
    for k in keys:
        os.write(master, k); time.sleep(0.18); drain(0.12)
    try: p.wait(timeout=30)
    except subprocess.TimeoutExpired: p.kill()
    drain(0.3)
    os.close(master)
    return p.returncode, buf.decode("utf-8", "replace")

out_pick = work / "out_pick"
code, screen = run_pick([b" ", b"\x1b[B", b"\x1b[B", b" ", b"\r"], out_pick)
if code == 0:
    ok(f"pty 里勾选完成（退出码 0）")
else:
    bad(f"pty 勾选失败：退出码 {code}\n{screen[-300:]}")

# 界面里中文要完整出现（验收③）
if "中文包" in screen and "文档" in screen and "遊戲數據" in screen:
    ok("勾选界面里中文文件名完整、不错行")
else:
    bad("勾选界面里中文缺失（可能是宽度算错）")

out_sel = work / "out_sel"
subprocess.run([BIN, "--select", "1,3", "-o", str(out_sel), str(zip_path)], capture_output=True)

def listing(d):
    return sorted(p.relative_to(d).as_posix() for p in d.rglob("*") if p.is_file())

f1, f2 = listing(out_pick), listing(out_sel)
if f1 and f1 == f2:
    ok(f"--pick 与 --select \"1,3\" 解出的文件一致（{len(f1)} 个）")
else:
    bad(f"两者不一致：--pick {f1} vs --select {f2}")

same = f1 == f2 and all(
    (out_pick / f).read_bytes() == (out_sel / f).read_bytes() for f in f1)
if same:
    ok("内容逐字节一致")
else:
    bad("内容有差异")

# ── ③ Esc 取消：不产出文件、退出码 0（验收④的前半）──
out_cancel = work / "out_cancel"
code2, _ = run_pick([b"\x1b"], out_cancel)
if code2 == 0 and not (out_cancel.exists() and any(out_cancel.rglob("*"))):
    ok("Esc 取消：退出码 0、一个文件都没解")
else:
    bad(f"取消行为不对：退出码 {code2}，产物 {listing(out_cancel) if out_cancel.exists() else '无目录'}")

print(f"\n  通过 {n_pass} · 失败 {n_fail}")
sys.exit(1 if n_fail else 0)
PY
