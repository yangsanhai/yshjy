#!/usr/bin/env python3
"""伪装文件（外壳）回归测试 —— 协议 pack 的 cover 参数。

用法：
    python3 tests/protocol_pack_cover.py [path/to/yshjy]

覆盖这几条（都是用户 2026-10-18 提的）：
  1. cover 传任意类型的文件 → 产物开头就是外壳本身（这里用最小 PDF 验）
  2. 外壳不是 MP4 → 自动降级「尾部追加」，结果里 layout=TailAppend
  3. 产物还能被自己 list / extract（数据没写坏）
  4. 伪装模式不传 cover → BAD_PARAM（不再偷偷用内置素材）
  5. 普通压缩传了 cover → 忽略，产物仍是标准 ZIP
  6. 输出路径 = 外壳路径 → BAD_PARAM（不覆盖用户的外壳）
"""
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_cover_"))

# 最小可用 PDF 当外壳
SHELL = W / "说明.pdf"
SHELL.write_bytes(
    b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n"
)

SRC = W / "内容"
SRC.mkdir()
(SRC / "笔记.txt").write_text("被藏起来的内容\n", encoding="utf-8")

proc = subprocess.Popen(
    [BIN, "--serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
    bufsize=1,
)
_id = 0


def 发(**kw):
    global _id
    _id += 1
    proc.stdin.write(json.dumps({"id": _id, **kw}, ensure_ascii=False) + "\n")
    proc.stdin.flush()
    return _id


def 收(msg_id, 超时=120):
    截止 = time.time() + 超时
    while time.time() < 截止:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("引擎把管道关了")
        v = json.loads(line)
        if v.get("type") == "event":
            continue
        if v.get("id") == msg_id:
            return v
    raise TimeoutError(f"等 id={msg_id} 超时")


def 断言(cond, msg):
    if not cond:
        print(f"❌ {msg}")
        sys.exit(1)
    print(f"✅ {msg}")


发(cmd="hello")
收(1)

# 1+2+3：任意文件当外壳
out = W / "寄出去.pdf"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(out), mode="disguise",
         cover=str(SHELL), checksum=False, readme=False))
断言(r.get("ok"), f"pack 成功（{r.get('error')}）")
res = r["result"]
断言(out.read_bytes()[:5] == b"%PDF-", "产物开头就是外壳 PDF（%PDF-）")
断言(res.get("layout") == "TailAppend", f"非 MP4 外壳自动用尾部追加（{res.get('layout')}）")
断言(res.get("cover") == str(SHELL), "结果里带回外壳路径")
断言(out.stat().st_size > SHELL.stat().st_size, "产物比外壳大（压缩数据追加在后面）")

r = 收(发(cmd="list", path=str(out)))
断言(r.get("ok"), "产物能被 list 定位")
断言(any(e["path"].endswith("笔记.txt") for e in r["result"]["entries"]),
     "清单里能看到藏进去的文件")

dest = W / "解出来"
r = 收(发(cmd="extract", path=str(out), dest=str(dest), cover=False))
断言(r.get("ok"), "产物能被 extract 还原")
断言((dest / "内容" / "笔记.txt").read_text(encoding="utf-8") == "被藏起来的内容\n",
     "解出来的内容一字不差")

# 4：伪装模式必须选外壳
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "没外壳.mp4"), mode="disguise"))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM",
     f"不传 cover 报 BAD_PARAM（{r.get('error', {}).get('code')}）")

# 5：普通压缩忽略 cover
plain = W / "普通.zip"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(plain), mode="plain",
         cover=str(SHELL), checksum=False, readme=False))
断言(r.get("ok") and zipfile.is_zipfile(plain), "普通压缩仍是标准 ZIP（cover 被忽略）")

# 6：输出不能覆盖外壳
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(SHELL), mode="disguise",
         cover=str(SHELL)))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM",
     "输出 = 外壳时报 BAD_PARAM（不覆盖用户文件）")

# 7：MP4 外壳仍走 free 原子（视频能播，数据藏在原子里）
mp4_shell = pathlib.Path("assets/cover/mini.mp4")
if mp4_shell.exists():
    out_mp4 = W / "能播的.mp4"
    r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(out_mp4), mode="disguise",
             cover=str(mp4_shell), checksum=False, readme=False))
    断言(r.get("ok") and r["result"].get("layout") == "FreeAtom",
         "MP4 外壳走 free 原子布局")
    断言(out_mp4.read_bytes()[4:8] == b"ftyp", "产物仍是合法 MP4 头")

proc.stdin.close()
proc.wait(timeout=10)
print(f"\n全部通过。测试目录：{W}")
