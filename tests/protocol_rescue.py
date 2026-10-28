#!/usr/bin/env python3
"""救援模式协议探针（2026-10-28）：界面「试着救出来」按钮背后的 `rescue` 命令

挡的是这几类事：
  1. 界面点了「救援」结果引擎不认识这个命令（UNKNOWN_COMMAND）—— 协议没接线
  2. 只重建清单就把文件写到盘上（用户点「先看看能救回啥」却被写了几百 MB）
  3. 救援结果落到「包旁边」而不是用户指定的目录
  4. 把「认不出压缩包」错报成「数据丢了」（用户会去重下好包）

样本是现造的：正常 zip，把中央目录签名 `PK\\x01\\x02` 全部打乱 —— 这就是
「索引坏了、数据还在」的典型样子，正是救援要处理的场面。

用法：
    python3 tests/protocol_rescue.py [path/to/yshjy]
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else os.environ.get(
    "YSHJY_BIN", "target/release/yshjy")

W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_rescue_"))

# 造样本：正常 zip → 打乱中央目录签名
SRC = W / "内容"
SRC.mkdir()
(SRC / "说明.txt").write_text("救援测试内容\n", encoding="utf-8")
(SRC / "数据.bin").write_bytes(b"R" * 8000)
PKG = W / "坏包.zip"
with zipfile.ZipFile(PKG, "w", zipfile.ZIP_DEFLATED) as z:
    for p in sorted(SRC.rglob("*")):
        if p.is_file():
            z.write(p, p.relative_to(SRC))

raw = bytearray(PKG.read_bytes())
签名数 = raw.count(b"PK\x01\x02")
i = 0
while True:
    j = raw.find(b"PK\x01\x02", i)
    if j < 0:
        break
    raw[j:j + 4] = b"XX\x01\x02"
    i = j + 4
PKG.write_bytes(bytes(raw))
assert 签名数 >= 1, "样本没造成（一个中央目录签名都没找到）"

# 正常连 list 都过不去，才说明「坏」得够真
proc0 = subprocess.run([BIN, "--list", str(PKG)], capture_output=True, text=True)
if proc0.returncode == 0:
    print(f"⚠ 样本的索引还是好的（--list 成功），救援用例会没有意义：{proc0.stdout[:120]}")

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


def 收(msg_id, 超时=180):
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


def 文件(d):
    return sorted(p.relative_to(d).as_posix() for p in pathlib.Path(d).rglob("*") if p.is_file())


发(cmd="hello")
h = 收(1)
断言("rescue" in (h.get("result", {}).get("features") or []),
     "hello.features 里有 rescue（否则界面按能力判断会把它当不存在）")

# ── ① list_only：只重建清单，一个字节都不落盘 ──
r = 收(发(cmd="rescue", path=str(PKG), list_only=True))
断言(r.get("ok"), f"list_only 成功（{r.get('error')}）")
res = r["result"]
断言(res.get("list_only") is True, "结果标明 list_only")
断言(res.get("entries_total") == 2, f"重建出 2 个条目（{res.get('entries_total')}）")
名字 = [e["path"] for e in res.get("rebuilt") or []]
断言(sorted(名字) == ["数据.bin", "说明.txt"], f"清单里两个文件都在（{名字}）")
断言(res.get("dest") in (None, ""), "list_only 不给 dest（不落盘）")
断言(文件(W) == ["坏包.zip"] or set(文件(W)) == {"坏包.zip", "内容/数据.bin", "内容/说明.txt"},
     f"磁盘上没多出救援产物（{文件(W)}）")

# ── ② 真救：落到指定 dest ──
out = W / "救出来"
r = 收(发(cmd="rescue", path=str(PKG), dest=str(out)))
断言(r.get("ok"), f"救援成功（{r.get('error')}）")
res = r["result"]
断言(res.get("outcome") == "Success", f"outcome=Success（{res.get('outcome')}）")
断言(res.get("entries_done") == 2 and res.get("entries_total") == 2,
     f"2/2 个条目救出来（{res.get('entries_done')}/{res.get('entries_total')}）")
断言(pathlib.Path(res["dest"]) == out, f"落到指定 dest（{res['dest']}）")
saved = 文件(out)
断言(saved == ["数据.bin", "说明.txt"], f"产物就是那两个文件（{saved}）")
断言((out / "说明.txt").read_text(encoding="utf-8") == "救援测试内容\n", "文本内容一字不差")
断言((out / "数据.bin").stat().st_size == 8000, "二进制大小一致（8000 字节）")
断言(not any(".rescue" in p for p in 文件(W)), "包旁边没留下 .rescue 副本")
断言(PKG.stat().st_size == len(raw), "原包一个字节没动（救援是只读的）")

# ── ③ 认不出的文件：必须报 NOT_ARCHIVE，不能报 DATA_LOST ──
bad = W / "随便.txt"
bad.write_text("这里面什么压缩包都没有\n", encoding="utf-8")
r = 收(发(cmd="rescue", path=str(bad)))
断言(not r.get("ok"), "非压缩包救援失败（符合预期）")
断言((r.get("error") or {}).get("code") == "NOT_ARCHIVE",
     f"错误码是 NOT_ARCHIVE 而不是 DATA_LOST（{(r.get('error') or {}).get('code')}）")

# ── ④ 不给 dest：落到包旁边 `<包名>.rescue`（与 CLI --rescue 一致）──
PKG2 = W / "第二个.zip"
PKG2.write_bytes(bytes(raw))
r = 收(发(cmd="rescue", path=str(PKG2)))
断言(r.get("ok"), f"不给 dest 也能救（{r.get('error')}）")
默认落点 = pathlib.Path(r["result"]["dest"])
断言(默认落点.name == "第二个.zip.rescue", f"默认落点叫 <包名>.rescue（{默认落点.name}）")
断言(文件(默认落点) == ["数据.bin", "说明.txt"], f"默认落点里也是这两个文件（{文件(默认落点)}）")

proc.stdin.close()
proc.wait(timeout=10)
print(f"\n全部通过。测试目录：{W}")
