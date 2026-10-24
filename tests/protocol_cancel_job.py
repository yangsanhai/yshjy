#!/usr/bin/env python3
"""协议探针：批量解压途中单独取消某一行（D5 / N4）

验的是真链路：`--serve` 跑起来 → 发 `batch` → 第一项正在跑时发
`{"cmd":"cancel","job":2}` → 结果里第 2 项必须是 `cancelled`，
而第 1、3 项照跑成功（**不是整批停**）。

用法：YSHJY_BIN=target/release/yshjy python3 tests/protocol_cancel_job.py
"""
import json
import os
import subprocess
import sys
import tempfile
import zipfile

BIN = os.environ.get("YSHJY_BIN", "target/release/yshjy")
if not os.path.exists(BIN):
    print(f"找不到引擎：{BIN}（先 cargo build --release）")
    sys.exit(2)

W = tempfile.mkdtemp(prefix="yshjy_canceljob.")
# 80 MiB × 3：够大，第一项跑的时候我们来得及发 cancel
包们 = []
for i in (1, 2, 3):
    p = os.path.join(W, f"pack{i}.zip")
    with zipfile.ZipFile(p, "w", zipfile.ZIP_STORED) as z:
        z.writestr("a.bin", os.urandom(80 * 1024 * 1024))
    包们.append(p)

proc = subprocess.Popen(
    [BIN, "--serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
    bufsize=1,
)


def send(obj):
    proc.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
    proc.stdin.flush()


def read_until(req_id, 事件收=None):
    """读到指定 id 的响应为止；路上碰到的事件顺手交给回调"""
    while True:
        line = proc.stdout.readline()
        if not line:
            print("!! 引擎提前退出了")
            sys.exit(1)
        line = line.strip()
        if not line:
            continue
        try:
            o = json.loads(line)
        except json.JSONDecodeError:
            continue
        if o.get("id") == req_id and ("ok" in o):
            return o
        if 事件收 is not None:
            事件收(o)


try:
    send({"cmd": "hello", "id": 1})
    你好 = read_until(1)
    if not 你好.get("ok"):
        print(f"!! hello 失败：{你好}")
        sys.exit(1)

    def 取job(o):
        """事件行形如 {"kind":"JOB_STARTED","event":{"JobStarted":{"job":1,...}}}"""
        ev = o.get("event")
        if isinstance(ev, dict) and ev:
            内 = next(iter(ev.values()))
            if isinstance(内, dict):
                return 内.get("job")
        return None

    已开始 = []

    def 收事件(o):
        j = 取job(o)
        if j is not None:
            已开始.append(j)

    send({"cmd": "batch", "paths": 包们, "dest": os.path.join(W, "out"), "id": 2})
    # 等第一项真的开始跑了，再取消第 2 项 —— 太早发就成「还没开始就跳过」了
    while not 已开始:
        line = proc.stdout.readline()
        if not line:
            print("!! 等 JOB_STARTED 时引擎退出了")
            sys.exit(1)
        try:
            o = json.loads(line)
        except json.JSONDecodeError:
            continue
        收事件(o)

    send({"cmd": "cancel", "job": 2, "id": 3})
    取消回执 = read_until(3)
    结果 = read_until(2)

    jobs = (结果.get("result") or {}).get("jobs") or []
    状态 = {j["id"]: j["outcome"] for j in jobs}
    print(f"任务开始顺序：{已开始}")
    print(f"取消回执：{取消回执.get('result')}")
    print(f"各项结局：{状态}")
    print(f"汇总：{结果.get('result', {}).get('cancelled')} 个取消")

    问题 = []
    if 状态.get(2) != "cancelled":
        问题.append(f"第 2 项应是 cancelled，实际 {状态.get(2)}")
    if 状态.get(1) != "ok":
        问题.append(f"第 1 项应照跑成功，实际 {状态.get(1)}")
    if 状态.get(3) != "ok":
        问题.append(f"第 3 项应照跑成功，实际 {状态.get(3)}")

    if 问题:
        print("\n✗ 失败：")
        for p in 问题:
            print(f"   · {p}")
        sys.exit(1)
    print("\n✓ 通过：只取消了第 2 项，第 1、3 项照跑成功")
finally:
    try:
        proc.stdin.close()
    except Exception:
        pass
    proc.wait(timeout=10)
