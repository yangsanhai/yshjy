#!/usr/bin/env python3
"""T16 探针 —— `--candidate <字节偏移>` 的命令行入口（S10 多候选）

引擎/协议早就能「解第 N 处」（界面「换一处解压」走的就是它），但命令行没法表达，
只能用 --serve 手工拼 —— 用户拿命令行复核时办不到。本探针只验命令行这一层。

断言（全部走真二进制、真文件）：
  1. 不给 --candidate → 解出引擎默认挑的那一处（chosen），行为没被改动
  2. `--candidate <另一处的偏移>` → 解出的文件来自**另一段**包（内容不同）
  3. `--candidate 999`（落在垃圾里）→ 退出码 NOT_ARCHIVE(3)，且**没有产物**
     （绝不能「找不到就偷偷解默认那处」——那等于用户点了第 2 处、程序解了第 1 处）
  4. `--candidate abc` → 退出码 USAGE(2)，提示里点名 --candidate 收不到数
  5. `--candidate` 配两个文件 → 退出码 USAGE(2)（它说的是「这一个文件里的第几处」）
  6. `--candidate` 的值不会被当成待解压的文件名（带值选项登记的老坑）

用法：python3 tests/candidate_cli.py [二进制路径]
"""
from __future__ import annotations

import json
import os
import random
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("YSHJY_BIN")
       or next((str(ROOT / "target" / rel) for rel in
                ("release/yshjy.exe", "release/yshjy", "debug/yshjy.exe", "debug/yshjy")
                if (ROOT / "target" / rel).exists()), ""))
if not BIN:
    print("❌ 找不到 yshjy 二进制（先 cargo build）")
    sys.exit(1)

USAGE, NOT_ARCHIVE = 2, 3

n_pass = n_fail = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✓ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ✗ {msg}")


def run(args, timeout=180):
    """跑一次真命令行，返回 (退出码, stdout, stderr)"""
    p = subprocess.run([BIN, *args], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


def scan_one(path):
    """走协议 scan 拿候选清单（只有它带 candidates[]）"""
    req = {"id": 1, "cmd": "scan", "paths": [str(path)]}
    p = subprocess.run([BIN, "--serve"], input=json.dumps(req) + "\n",
                       capture_output=True, text=True, timeout=180)
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        obj = json.loads(line)
        if obj.get("id") == 1:
            if not obj.get("ok"):
                print(f"❌ scan 失败：{obj}")
                sys.exit(1)
            items = obj["result"].get("items", [])
            if len(items) != 1:
                print(f"❌ scan 返回 {len(items)} 个 item，期望 1")
                sys.exit(1)
            return items[0]
    print(f"❌ scan 没回应\nstdout={p.stdout}\nstderr={p.stderr}")
    sys.exit(1)


def make_zip(dst: Path, name: str, payload: bytes):
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(name, payload)


def names_under(d: Path):
    if not d.exists():
        return set()
    return {p.name for p in d.rglob("*") if p.is_file()}


def main():
    tmp = Path(os.environ.get("TMPDIR", "/tmp")) / "t16-candidate-cli"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir(parents=True)
    rnd = random.Random(20261031)

    def junk(n):
        # 避开 'P' 开头，免得随机垃圾里撞出假 LFH 干扰扫描
        return bytes(rnd.choice(b"abcdefghijklmnoqrstuvwxyz0123456789") for _ in range(n))

    zip_a, zip_b = tmp / "A.zip", tmp / "B.zip"
    make_zip(zip_a, "第A处解出来的.txt", b"A" * 2048)
    make_zip(zip_b, "第B处解出来的.txt", b"B" * 2048)
    multi = tmp / "藏两个.mp4"
    with open(multi, "wb") as f:
        f.write(junk(3 << 20))
        f.write(zip_a.read_bytes())
        f.write(junk(3 << 20))
        f.write(zip_b.read_bytes())
        # 尾部必须 >16 MiB（layout::zip::DEFAULT_WINDOW），否则尾部窗口就直接找到了
        f.write(junk(20 << 20))

    it = scan_one(multi)
    cands = it.get("candidates") or []
    if len(cands) < 2:
        print(f"❌ 多候选样本只报出 {len(cands)} 条候选，探针无法继续：{cands}")
        return 1
    chosen = [c for c in cands if c.get("chosen")]
    其它 = [c for c in cands if not c.get("chosen")]
    if not chosen or not 其它:
        print(f"❌ 候选里没有「默认那条 / 非默认那条」：{cands}")
        return 1

    # ── 1. 不给 --candidate：还是解默认那条 ──
    print("1. 不给 --candidate → 解默认那条（老行为没变）")
    d1 = tmp / "默认"
    rc, out, err = run(["-o", str(d1), str(multi)])
    有的1 = names_under(d1)
    if rc == 0 and 有的1:
        ok(f"解出 {sorted(有的1)}（退出码 0）")
    else:
        bad(f"默认解压失败：rc={rc} stderr={err.strip()[:200]}")

    # ── 2. --candidate 指定另一处：解出来的必须是另一段 ──
    print("2. --candidate <另一处的偏移> → 解出另一段")
    off = 其它[0]["offset"]
    d2 = tmp / "指定"
    rc, out, err = run(["--candidate", str(off), "-o", str(d2), str(multi)])
    有的2 = names_under(d2)
    if rc == 0 and 有的2 and 有的1 and 有的2 != 有的1:
        ok(f"--candidate {off} 解出 {sorted(有的2)}，与默认 {sorted(有的1)} 不同")
    else:
        bad(f"--candidate {off} 没换到另一段：rc={rc} 默认={sorted(有的1)} 指定={sorted(有的2)} "
            f"stderr={err.strip()[:200]}")

    # ── 3. 非法偏移：报 NOT_ARCHIVE，且不产生产物 ──
    print("3. --candidate 999（落在垃圾里）→ 退出码 NOT_ARCHIVE，不许偷偷解默认那处")
    d3 = tmp / "非法偏移"
    rc, out, err = run(["--candidate", "999", "-o", str(d3), str(multi)])
    if rc == NOT_ARCHIVE and not names_under(d3):
        ok(f"退出码 {rc}（NOT_ARCHIVE），目标目录没产物")
    else:
        bad(f"期望 rc={NOT_ARCHIVE} 且无产物，实际 rc={rc} 产物={sorted(names_under(d3))} "
            f"stderr={err.strip()[:200]}")

    # ── 4. 非数字：USAGE 并且点名 --candidate ──
    print("4. --candidate abc → 退出码 USAGE，提示点名 --candidate")
    d4 = tmp / "非数字"
    rc, out, err = run(["--candidate", "abc", "-o", str(d4), str(multi)])
    msg = (out + err)
    if rc == USAGE and "candidate" in msg:
        ok(f"退出码 {rc}（USAGE），提示：{msg.strip().splitlines()[0][:80]}")
    else:
        bad(f"期望 rc={USAGE} 且提示含 candidate，实际 rc={rc} 输出={msg.strip()[:200]}")

    # ── 5. 配两个文件：拒绝（一个偏移不可能同时对上两个包）──
    print("5. --candidate 配两个文件 → 退出码 USAGE")
    d5 = tmp / "两个文件"
    rc, out, err = run(["--candidate", str(off), "-o", str(d5), str(multi), str(zip_a)])
    msg = (out + err)
    if rc == USAGE:
        ok(f"退出码 {rc}（USAGE），提示：{msg.strip().splitlines()[0][:80]}")
    else:
        bad(f"期望 rc={USAGE}，实际 rc={rc} 输出={msg.strip()[:200]}")

    # ── 6. 值不能被当成待解压的文件名（带值选项登记的老坑）──
    print("6. --candidate 的值不被当成输入文件")
    d6 = tmp / "值不当文件"
    rc, out, err = run(["--candidate", str(off), "-o", str(d6), str(multi)])
    msg = (out + err)
    if str(off) not in msg or "找不到" not in msg:
        ok("输出里没有把偏移当文件名的迹象")
    else:
        bad(f"偏移被当成输入文件了：{msg.strip()[:200]}")

    print()
    print(f"结果：{n_pass} 通过 / {n_fail} 失败")
    shutil.rmtree(tmp, ignore_errors=True)
    return 1 if n_fail else 0


if __name__ == "__main__":
    sys.exit(main())
