#!/usr/bin/env python3
"""S10 多候选（T5）探针 —— 一个文件里塞着不止一份归档时，引擎要把它们都报出来

断言：
  1. 普通 zip → `candidates` 只有 1 条，且 `chosen` 为真（界面据此**不显示**选择界面）
  2. `[垃圾][ZIP-A][垃圾][ZIP-B][垃圾]` → `candidates` ≥ 2（慢路径扫描撞出来的）
  3. 每条候选的 `confidence` 只能是 high / medium / low 三档机器标识
  4. 候选里**恰有一条** `chosen`（界面默认勾它，永远不出现"没有默认"或"两个默认"）
  5. 非包（纯随机文件）→ `candidates` 为空数组，不是缺字段（外壳不用判 null）

用法：python3 tests/t5_candidates.py [二进制路径]
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

n_pass = n_fail = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✓ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ✗ {msg}")


def serve(req):
    """跑一条协议命令，返回 result（失败直接退出，别把错误当成空答案）"""
    p = subprocess.run(
        [BIN, "--serve"],
        input=json.dumps(req) + "\n",
        capture_output=True,
        text=True,
        timeout=180,
    )
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        obj = json.loads(line)
        if obj.get("id") == req["id"]:
            if not obj.get("ok"):
                print(f"❌ 协议调用失败：{obj}")
                sys.exit(1)
            return obj["result"]
    print(f"❌ 没有拿到 id={req['id']} 的回应\nstdout={p.stdout}\nstderr={p.stderr}")
    sys.exit(1)


def serve_allow_err(req):
    """跑一条命令，**允许失败**：成功返回 result，失败返回 None（用来验「该拒的要拒」）"""
    p = subprocess.run(
        [BIN, "--serve"],
        input=json.dumps(req) + "\n",
        capture_output=True,
        text=True,
        timeout=180,
    )
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        obj = json.loads(line)
        if obj.get("id") == req["id"]:
            return obj["result"] if obj.get("ok") else None
    print(f"❌ 没拿到 id={req['id']} 的回应\nstdout={p.stdout}\nstderr={p.stderr}")
    sys.exit(1)


def extract_to(path: Path, dest: Path, candidate=None):
    dest.mkdir(parents=True, exist_ok=True)
    req = {"id": 8, "cmd": "extract", "path": str(path), "dest": str(dest)}
    if candidate is not None:
        req["candidate"] = candidate
    return serve(req)


def scan_one(path):
    res = serve({"id": 1, "cmd": "scan", "paths": [str(path)]})
    items = res.get("items", [])
    if len(items) != 1:
        print(f"❌ scan 返回 {len(items)} 个 item，期望 1 个：{res}")
        sys.exit(1)
    return items[0]


def make_zip(dst: Path, name: str, payload: bytes):
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(name, payload)


def main():
    tmp = Path(os.environ.get("TMPDIR", "/tmp")) / "t5-candidates"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir(parents=True)

    rnd = random.Random(20261030)

    # ── 1. 普通 zip：单候选 ──
    print("1. 普通包 → 只有一条候选（界面不显示选择）")
    plain = tmp / "普通.zip"
    make_zip(plain, "a.txt", b"hello" * 100)
    it = scan_one(plain)
    cands = it.get("candidates")
    if cands is None:
        bad("普通包没有 candidates 字段")
    elif len(cands) == 1:
        ok(f"普通包 candidates = 1（{cands[0]['format']} / {cands[0]['confidence']}）")
    else:
        bad(f"普通包 candidates = {len(cands)}，期望 1：{cands}")
    if cands and cands[0].get("chosen") is True:
        ok("普通包那一条 chosen = true")
    else:
        bad(f"普通包那条没标 chosen：{cands}")

    # ── 2. 多候选：垃圾 + ZIP-A + 垃圾 + ZIP-B + 垃圾 ──
    print("2. [垃圾][ZIP-A][垃圾][ZIP-B][垃圾] → 两处都要报出来")
    zip_a = tmp / "A.zip"
    zip_b = tmp / "B.zip"
    make_zip(zip_a, "A.txt", b"A" * 2048)
    make_zip(zip_b, "B.txt", b"B" * 2048)
    multi = tmp / "藏两个.mp4"

    def junk(n):
        # 故意避开 'P' 开头，免得随机垃圾里撞出假 LFH 干扰断言
        return bytes(rnd.choice(b"abcdefghijklmnoqrstuvwxyz0123456789") for _ in range(n))

    with open(multi, "wb") as f:
        f.write(junk(3 << 20))
        f.write(zip_a.read_bytes())
        f.write(junk(3 << 20))
        f.write(zip_b.read_bytes())
        # 尾部垫 >16 MiB：`layout::zip::DEFAULT_WINDOW` 是 16 MiB，只有超出它
        # 才会退化成全文件扫描（真实情形：包里藏着包、后面还跟着整部视频）。
        f.write(junk(20 << 20))

    it = scan_one(multi)
    cands = it.get("candidates") or []
    if len(cands) >= 2:
        ok(f"多候选样本 candidates = {len(cands)}："
           + "、".join(f"{c['format']}@{c['offset']}/{c['confidence']}" for c in cands))
    else:
        bad(f"多候选样本只报出 {len(cands)} 条（期望 ≥2）：{cands}；kind={it.get('kind')}")
    print(f"     （仅供参考：kind={it.get('kind')} archive={it.get('archive')} "
          f"looks_like={it.get('looks_like')}）")

    # ── 3. 置信度取值 ──
    print("3. confidence 只能是三档机器标识")
    allowed = {"high", "medium", "low"}
    got = {c.get("confidence") for c in cands}
    if got <= allowed:
        ok(f"取值合法（{sorted(got)}）")
    else:
        bad(f"出现了约定外的置信度：{got - allowed}")

    # ── 4. 恰有一条 chosen ──
    print("4. 候选里恰有一条 chosen")
    n_chosen = sum(1 for c in cands if c.get("chosen") is True)
    if n_chosen == 1:
        ok("恰有一条 chosen（界面默认勾它）")
    else:
        bad(f"chosen 有 {n_chosen} 条，期望恰好 1 条：{[c.get('chosen') for c in cands]}")

    # ── 5. 非包 → 空数组 ──
    print("5. 非包（纯随机文件）→ candidates 是空数组")
    noise = tmp / "乱七八糟.bin"
    noise.write_bytes(junk(64 << 10))
    it = scan_one(noise)
    c = it.get("candidates")
    if c == []:
        ok("非包 candidates = []（不是 null，外壳不用判空）")
    else:
        bad(f"非包 candidates = {c!r}，期望 []")

    # ── 6. 指定候选解压（「都试试」的引擎底座）──
    print("6. candidate 指定第几处 → 解出来的就是那一段")
    if len(cands) < 2:
        bad("候选不足两条，没法验「解指定的那一处」")
    else:
        非选中 = [c for c in cands if not c.get("chosen")]
        选中 = [c for c in cands if c.get("chosen")]
        if not 非选中 or not 选中:
            bad(f"候选里没找到「选中的那条」和「没选中的那条」：{cands}")
        else:
            # ① 不指定 → 引擎自己挑（应当是 chosen 那条）
            d1 = tmp / "解默认"
            extract_to(multi, d1)
            有的1 = {p.name for p in d1.rglob("*") if p.is_file()}
            # ② 指定第一条（非 chosen）→ 必须解出另一份内容
            d2 = tmp / "解第一处"
            extract_to(multi, d2, candidate=非选中[0]["offset"])
            有的2 = {p.name for p in d2.rglob("*") if p.is_file()}
            if 有的1 and 有的2 and 有的1 != 有的2:
                ok(f"默认解出 {sorted(有的1)}，指定第 1 处解出 {sorted(有的2)}——确实是两段不同的包")
            else:
                bad(f"指定候选没换到另一段：默认={sorted(有的1)} 指定={sorted(有的2)}")
            # ③ 非法 candidate → 必须报错，不许静默回退成"自己挑一个"
            r = serve_allow_err({"id": 9, "cmd": "extract", "path": str(multi),
                                 "dest": str(tmp / "解非法"), "candidate": 999})
            if r is None:
                ok("非法 candidate 被拒（没有假装成功）")
            else:
                bad(f"非法 candidate 居然通过了：{r}")

    print()
    print(f"结果：{n_pass} 通过 / {n_fail} 失败")
    shutil.rmtree(tmp, ignore_errors=True)
    return 1 if n_fail else 0


if __name__ == "__main__":
    sys.exit(main())
