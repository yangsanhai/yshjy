#!/usr/bin/env python3
"""第21批组A-界面：协议实测（普通压缩 / 伪装打包 / 封面独立勾选）"""
import json
import os
import shutil
import subprocess
import sys

BIN = os.path.expanduser("~/hermes/yshjy/target/release/yshjy")
WORK = os.path.expanduser("~/.hermes/cache/scratch/yshjy_probe_plain")
shutil.rmtree(WORK, ignore_errors=True)
os.makedirs(WORK)
src = os.path.join(WORK, "src")
os.makedirs(src)
open(os.path.join(src, "a.txt"), "w").write("hello plain")
open(os.path.join(src, "b.bin"), "wb").write(b"\x07" * 5000)

p = subprocess.Popen([BIN, "--serve"], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, text=True, bufsize=1)
nid = 0
events = []


def call(cmd, **kw):
    global nid
    nid += 1
    msg = {"id": nid, "cmd": cmd, **kw}
    p.stdin.write(json.dumps(msg, ensure_ascii=False) + "\n")
    p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line:
            raise RuntimeError("引擎提前退出")
        v = json.loads(line)
        if v.get("type") == "event":
            events.append(v)
            continue
        if v.get("id") == nid:
            return v


def check(name, ok, info=""):
    print(("PASS " if ok else "FAIL ") + name + (("  " + info) if info else ""))
    return ok


fails = 0
h = call("hello")["result"]
features = h.get("features", [])
fails += not check("features 含 answer/skip", {"answer", "skip"} <= set(features), str(features))
fails += not check("event_kinds 含 ASK_USER", "ASK_USER" in h.get("event_kinds", []))

# ① 普通压缩
plain = os.path.join(WORK, "plain.zip")
r = call("pack", inputs=[src], out=plain, mode="plain")["result"]
head = open(plain, "rb").read(4)
fails += not check("普通压缩产物开头是 PK", head == b"PK\x03\x04", repr(head))
fails += not check("普通压缩 cover_size=0", r.get("cover_size") == 0, str(r.get("cover_size")))
fails += not check("普通压缩 mode=plain", r.get("mode") == "plain", str(r.get("mode")))
fails += not check("普通压缩没有说明书", r.get("readme") in (None, ""), str(r.get("readme")))
ld = call("list", path=plain)["result"]
fails += not check("普通压缩能列出内容", len(ld.get("entries", [])) == 2, str(len(ld.get("entries", []))))
fails += not check("普通压缩没有 cover 字段", "cover" not in ld, str(ld.get("cover")))

# ② 伪装打包（默认仍是伪装，不传 mode 也要老行为）
fake = os.path.join(WORK, "fake.mp4")
r2 = call("pack", inputs=[src], out=fake)["result"]
head2 = open(fake, "rb").read(8)
fails += not check("不传 mode 仍是伪装（MP4 头）", head2[4:8] == b"ftyp", repr(head2))
fails += not check("伪装 mode=disguise", r2.get("mode") == "disguise", str(r2.get("mode")))
ld2 = call("list", path=fake)["result"]
cov = ld2.get("cover")
fails += not check("伪装包 list 带 cover", isinstance(cov, dict) and cov.get("size", 0) > 0, str(cov))

# ③ 只提取封面（extract:false）
out_cover_only = os.path.join(WORK, "cover_only")
r3 = call("extract", path=fake, dest=out_cover_only, extract=False)["result"]
fails += not check("只提封面时 entries_done=0", r3.get("entries_done") == 0, str(r3.get("entries_done")))
fails += not check("只提封面拿得到 cover", isinstance(r3.get("cover"), dict), str(r3.get("cover")))
has_files = os.path.isdir(out_cover_only) and any(os.scandir(out_cover_only))
fails += not check("只提封面不建内容目录", not has_files, str(out_cover_only))

# ④ 只解内容（cover:false）
out_body = os.path.join(WORK, "body_only")
r4 = call("extract", path=fake, dest=out_body, cover=False)["result"]
fails += not check("只解内容时 cover 为 null", r4.get("cover") is None, str(r4.get("cover")))
fails += not check("只解内容解出 2 个文件", r4.get("entries_done") == 2, str(r4.get("entries_done")))

# ⑤ 声明的地址：select 是字符串（与 CLI 的 --select 同格式），非空必须报 UNSUPPORTED
r5 = call("extract", path=fake, dest=os.path.join(WORK, "x"), select="1")
fails += not check("select 仍明确报 UNSUPPORTED", r5.get("error", {}).get("code") == "UNSUPPORTED", str(r5))

p.stdin.close()
p.wait(timeout=10)
print(f"\n结果：{'全部通过' if fails == 0 else str(fails) + ' 项失败'}")
sys.exit(1 if fails else 0)
