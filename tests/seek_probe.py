#!/usr/bin/env python3
"""seek 三档的协议契约（第三期 C）。

由来（2026-10-29）：手机上的存储句柄不一定能 seek。`docs/ARCHITECTURE.md` F1 的方案是
三档 —— ① 直接 fd ② 块缓存 ③ 兜底先复制（要空间预检），并要求「选文件后立刻探测」。

本轮**只做探测 + 协议字段**：真到 `copy_first` 时复制那份必须在 Android 上验，
本机没有安卓环境，所以这里只断言「探到了、字段对、老前端不受影响」，
不冒充「落地已完成」。

断言：
  1. `hello.caps.seek_probe` 为真（界面据此决定要不要探测/提示）
  2. `scan` 的每个候选都带 `seek` 字段，值只能是三档机器标识之一
  3. 普通文件 → `full`（桌面常态，绝大多数用户走这档）
  4. 字符设备（`/dev/null`）→ `copy_first`（流不能回退，重读只能靠复制）
  5. （FIFO 不在这里测：`scan` 会对候选做 `job::identify`，那一步会 open FIFO 而阻塞。
      FIFO 的判定由 `src/seek.rs` 的单测覆盖 —— 那里只调探测函数，不打开文件。）
  6. `seek_label` 给界面用，但**不参与**机器判断（不许拿中文当状态机）
  7. 老前端兼容：`hello.caps` 里原有的 preview/selective/resume/background 都还在

用法：python3 tests/seek_probe.py [二进制路径]
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "target" / "release" / "yshjy")
if not Path(BIN).exists():
    BIN = str(ROOT / "target" / "debug" / "yshjy")
if not Path(BIN).exists():
    print("❌ 找不到 yshjy 二进制（先 cargo build --release）")
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
        timeout=60,
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


ALLOWED = {"full", "block_cache", "copy_first"}

# ── 1. 能力位 ──
hello = serve({"id": 1, "cmd": "hello"})
caps = hello.get("caps", {})
if caps.get("seek_probe") is True:
    ok("hello.caps.seek_probe = true")
else:
    bad(f"hello.caps.seek_probe 缺失或不为真：{caps.get('seek_probe')}")

# ── 7. 老前端兼容（只加不删）──
missing = [k for k in ("preview", "selective", "resume", "background") if k not in caps]
if not missing:
    ok("hello.caps 里原有能力位都还在（只加不删）")
else:
    bad(f"原有能力位丢了：{missing}")

# ── 2/3. 普通文件 ──
tmp = Path(tempfile.mkdtemp(prefix="yshjy_seekprobe_"))
try:
    normal = tmp / "普通.zip"
    normal.write_bytes(b"PK\x03\x04" + b"\x00" * 64)
    targets = [str(normal), "/dev/null"]

    res = serve({"id": 2, "cmd": "scan", "paths": targets})
    items = {i["path"]: i for i in res.get("items", [])}

    if all("seek" in i for i in items.values()):
        ok("scan 的每个候选都带 seek 字段")
    else:
        bad("有候选没带 seek 字段")

    bad_values = {i.get("seek") for i in items.values()} - ALLOWED
    if not bad_values:
        ok(f"seek 取值都在约定三档内（{sorted({i.get('seek') for i in items.values()})}）")
    else:
        bad(f"出现了约定外的 seek 值：{bad_values}")

    got = items.get(str(normal), {}).get("seek")
    if got == "full":
        ok("普通文件 → full（桌面常态）")
    else:
        bad(f"普通文件应判 full，实际 {got}")

    got = items.get("/dev/null", {}).get("seek")
    if got == "copy_first":
        ok("字符设备 → copy_first（流不能回退）")
    else:
        bad(f"/dev/null 应判 copy_first，实际 {got}")


    # ── 6. label 只是给人的，不许当状态机 ──
    labels = [i.get("seek_label") for i in items.values()]
    if all(isinstance(x, str) and x for x in labels):
        ok("每个候选都带非空的 seek_label（给界面显示用）")
    else:
        bad("seek_label 缺失")
finally:
    shutil.rmtree(tmp, ignore_errors=True)

print()
print(f"  通过 {n_pass} / 失败 {n_fail}")
sys.exit(0 if n_fail == 0 else 1)
