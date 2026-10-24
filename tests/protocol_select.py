#!/usr/bin/env python3
"""文件级挑选（select）协议回归 —— 第 23 批 P23-1。

用法：
    python3 tests/protocol_select.py [path/to/yshjy]

覆盖：
 1. 不传 select → 全解（「只加不删」：老前端行为不能变）
 2. select 传序号数组 [index] → 只解出那一个（序号 = list 里的 index，从 1 起）
 3. select 传表达式 "*.txt" → 只解 txt
 4. select 传非法表达式 → BAD_PARAM（不静默全解）
 5. batch 也认 select
"""
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_select_proto_"))

# 造一个三种文件的包；用伪随机数据避免撞压缩比防护
SRC = W / "内容"
SRC.mkdir()
import random

random.seed(7)
(SRC / "a.txt").write_bytes(b"AAA" + bytes(random.getrandbits(8) for _ in range(200)))
(SRC / "b.txt").write_bytes(b"BBB" + bytes(random.getrandbits(8) for _ in range(200)))
(SRC / "c.bin").write_bytes(b"CCC" + bytes(random.getrandbits(8) for _ in range(400)))
BAO = W / "包.zip"
with zipfile.ZipFile(BAO, "w", zipfile.ZIP_DEFLATED) as z:
    for f in sorted(SRC.iterdir()):
        z.write(f, f.name)

proc = subprocess.Popen([BIN, "--serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, text=True, bufsize=1)
_id = 0


def 发(**kw):
    global _id
    _id += 1
    proc.stdin.write(json.dumps({"id": _id, **kw}, ensure_ascii=False) + "\n")
    proc.stdin.flush()
    return _id


def 收(mid, 超时=120):
    截止 = time.time() + 超时
    while time.time() < 截止:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("引擎把管道关了")
        v = json.loads(line)
        if v.get("type") == "event":
            continue
        if v.get("id") == mid:
            return v
    raise TimeoutError(f"等 id={mid} 超时")


def 断言(c, m):
    if not c:
        print(f"❌ {m}")
        sys.exit(1)
    print(f"✅ {m}")


def 产物(dest):
    d = pathlib.Path(dest)
    return sorted(p.name for p in d.rglob("*") if p.is_file()) if d.exists() else []


# ── 0. hello / list 拿 index ──
r = 收(发(cmd="list", path=str(BAO)))
断言(r["ok"], "list 正常返回")
entries = r["result"]["entries"]
断言(len(entries) == 3, f"列表 3 个条目（实际 {len(entries)}）")
idx = {e["path"].split("/")[-1].split("\\")[-1]: e["index"] for e in entries}


# ── 1. 不传 select：全解 ──
d1 = W / "全解"
r = 收(发(cmd="extract", path=str(BAO), dest=str(d1)))
断言(r["ok"], f"不传 select 解压成功：{r.get('error')}")
断言(产物(d1) == ["a.txt", "b.txt", "c.bin"], f"全解出 3 个（实际 {产物(d1)}）")


# ── 2. 序号数组：只解一个 ──
d2 = W / "只一个"
r = 收(发(cmd="extract", path=str(BAO), dest=str(d2), select=[idx["c.bin"]]))
断言(r["ok"], f"select 序号数组解压成功：{r.get('error')}")
断言(产物(d2) == ["c.bin"], f"只解出 c.bin（实际 {产物(d2)}）")


# ── 3. 表达式：只解 txt ──
d3 = W / "只要txt"
r = 收(发(cmd="extract", path=str(BAO), dest=str(d3), select="*.txt"))
断言(r["ok"], f"select 表达式解压成功：{r.get('error')}")
断言(产物(d3) == ["a.txt", "b.txt"], f"只解出两个 txt（实际 {产物(d3)}）")


# ── 4. 非法表达式必须报错 ──
d4 = W / "非法"
r = 收(发(cmd="extract", path=str(BAO), dest=str(d4), select="瞎写的表达式"))
断言(not r["ok"], "非法 select 被拒绝（不静默全解）")
断言(r["error"]["code"] == "BAD_PARAM", f"错误码 BAD_PARAM（实际 {r['error']['code']}）")
断言(not d4.exists() or 产物(d4) == [], "非法参数没有产出任何文件")


# ── 5. 批量也认 select ──
d5 = W / "批量"
r = 收(发(cmd="batch", paths=[str(BAO)], dest=str(d5), select="*.bin"))
断言(r["ok"], f"batch 带 select 成功：{r.get('error')}")
断言(产物(d5) == ["c.bin"], f"batch 只解出 c.bin（实际 {产物(d5)}）")


# ── 6. required：跳过目录与平台垃圾（这里没有垃圾，应解出 3 个文件） ──
d6 = W / "必需"
r = 收(发(cmd="extract", path=str(BAO), dest=str(d6), select="required"))
断言(r["ok"], f"select=required 成功：{r.get('error')}")
断言(len(产物(d6)) == 3, f"required 解出 3 个（实际 {产物(d6)}）")

proc.stdin.close()
proc.wait(timeout=10)
print("\n全部通过（select 协议 6 组断言）")
