#!/usr/bin/env python3
"""`--serve` 一次性喂命令不丢结果（D16 / P23-7）—— 第 23 批。

用法：
    python3 tests/protocol_serve_eof.py [path/to/yshjy]

修之前的症状：`printf '{"id":1,"cmd":"extract",…}' | yshjy --serve`
—— stdin EOF 先到，主循环直接 break，正在跑的任务结果一个字都没输出。

断言：
 1. 一次性喂一条 extract 后立刻 EOF，结果仍然出现，且 outcome=Success
 2. 产物真的解出来了（不是只回了个 Success）
 3. 一次性喂两条（第二条必然 BUSY，协议规定一次一个任务），第一条结果不丢
"""
import json
import pathlib
import subprocess
import sys
import tempfile
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_serve_eof_"))

SRC = W / "内容"
SRC.mkdir()
import random

random.seed(11)
# 造 120 个小文件：让解压至少跑几百毫秒，EOF 才会「抢在」结果前面
for i in range(120):
    (SRC / f"f{i:03d}.bin").write_bytes(bytes(random.getrandbits(8) for _ in range(2000)))
BAO = W / "包.zip"
with zipfile.ZipFile(BAO, "w", zipfile.ZIP_DEFLATED) as z:
    for f in sorted(SRC.iterdir()):
        z.write(f, f.name)


def 断言(c, m):
    if not c:
        print(f"❌ {m}")
        sys.exit(1)
    print(f"✅ {m}")


def 行们(out):
    结果 = []
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            结果.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return 结果


# ── 1. 单条命令 + 立刻 EOF ──
d1 = W / "解出"
cmd = json.dumps({"id": 1, "cmd": "extract", "path": str(BAO), "dest": str(d1)},
                 ensure_ascii=False) + "\n"
p = subprocess.run([BIN, "--serve"], input=cmd, capture_output=True, text=True, timeout=180)
vs = 行们(p.stdout)
结果行 = [v for v in vs if v.get("type") != "event"]
断言(结果行, f"EOF 后仍然收到了任务结果（stdout 行数 {len(vs)}）")
断言(结果行[0]["id"] == 1 and 结果行[0]["ok"], f"id=1 成功：{结果行[0]}")
断言(结果行[0]["result"]["outcome"] == "Success", "outcome=Success")
n = len(list(d1.rglob("*.bin"))) if d1.exists() else 0
断言(n == 120, f"真的解出 120 个文件（实际 {n}）")

# ── 2. 两条命令一次性喂：第二条 BUSY 可以，第一条结果不能丢 ──
d2 = W / "解出2"
两条 = (json.dumps({"id": 10, "cmd": "extract", "path": str(BAO), "dest": str(d2)},
                   ensure_ascii=False) + "\n"
      + json.dumps({"id": 11, "cmd": "list", "path": str(BAO)}, ensure_ascii=False) + "\n")
p = subprocess.run([BIN, "--serve"], input=两条, capture_output=True, text=True, timeout=180)
vs = 行们(p.stdout)
结果行 = {v.get("id"): v for v in vs if v.get("type") != "event"}
断言(10 in 结果行, "第一条（extract）结果没丢")
断言(结果行[10].get("ok"), f"第一条成功：{结果行[10]}")
本 = [v for v in vs if v.get("type") != "event" and v.get("id") == 11]
if 本:
    断言(本[0]["error"]["code"] == "BUSY", f"第二条按约定回 BUSY（实际 {本[0].get('error')}）")
    print("   （第二条 BUSY 是设计：协议一次只跑一个任务，脚本要逐条等响应）")
else:
    print("   （第二条没回响应 —— 允许，但脚本应逐条等待）")

print("\n全部通过（serve EOF 不丢结果 2 组断言）")
