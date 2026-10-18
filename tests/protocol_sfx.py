#!/usr/bin/env python3
"""自解压（SFX）协议回归测试 —— 第 23 批。

用法：
    python3 tests/protocol_sfx.py [path/to/yshjy]

注意：开发机上 `assets/sfx/yshjy-sfx.exe` 可能是 Linux 版存根
（`YSHJY_ALLOW_NON_PE_STUB=1` 构建），所以这里不硬编码 MZ，
而是跟存根文件逐字节比开头；正式发布前必须换成真正的 Windows PE。
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
STUB = pathlib.Path("assets/sfx/yshjy-sfx.exe").read_bytes()
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_sfx_proto_"))

SRC = W / "内容"
SRC.mkdir()
(SRC / "说明.txt").write_text("自解压测试\n", encoding="utf-8")

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


发(cmd="hello")
收(1)

# 1. 正常生成
out = W / "自解压包.exe"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(out), mode="disguise",
         sfx=True, checksum=False, readme=False))
断言(r.get("ok"), f"pack sfx 成功（{r.get('error')}）")
断言(r["result"].get("sfx") is True, "结果里 sfx=true")
断言(out.read_bytes()[:len(STUB[:4])] == STUB[:4], "产物开头就是存根")
断言(out.stat().st_size > len(STUB), "产物 = 存根 + 压缩数据")

# 2. 产物能自己解开
# 正式存根是 Windows PE（MZ）→ 用 wine 跑；开发机的 Linux 版存根直接跑
if STUB[:2] == b"MZ":
    runner = ["wine"]
    env = dict(os.environ, WINEDEBUG="-all",
               YSHJY_SFX_SILENT="1", YSHJY_SFX_NO_OPEN="1")
else:
    out.chmod(0o755)
    runner = []
    env = dict(os.environ, YSHJY_SFX_SILENT="1", YSHJY_SFX_NO_OPEN="1")
r2 = subprocess.run(runner + [str(out)], cwd=str(W), env=env,
                    capture_output=True, text=True, timeout=180)
目标 = W / "自解压包" / "内容" / "说明.txt"
断言(目标.read_text(encoding="utf-8") == "自解压测试\n", "双击（直接运行）能解出内容")

# 3. 互斥项都明确报错
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "带密码.exe"), mode="disguise",
         sfx=True, password="123"))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM", "sfx + 解压码 → BAD_PARAM")

r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "分卷.exe"), mode="disguise",
         sfx=True, split=1024 * 1024))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM", "sfx + 分卷 → BAD_PARAM")

r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "错后缀.mp4"), mode="disguise",
         sfx=True))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM", "sfx + 非 .exe → BAD_PARAM")

r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "普通.zip"), mode="plain", sfx=True))
断言(not r.get("ok") and r["error"]["code"] == "BAD_PARAM", "普通压缩 + sfx → BAD_PARAM")

proc.stdin.close()
proc.wait(timeout=10)
print(f"\n全部通过。测试目录：{W}")
