#!/usr/bin/env python3
"""压缩级别（level）协议 + CLI 回归 —— 第 23 批 P23-4。

用法：
    python3 tests/protocol_level.py [path/to/yshjy]

断言：
 1. 协议 pack 不带 level → 结果里 level=6（引擎默认，界面据此显示「标准」）
 2. level=0（仅存储）产出体积 > level=9（最小体积），且 level=9 的包能正常解开
 3. level=10 必须报 BAD_PARAM，不静默当默认
 4. CLI `--level 9` 生效；`--level 10` 退出码 2（用法错误）
"""
import json
import pathlib
import random
import subprocess
import sys
import tempfile
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_level_"))

SRC = W / "内容"
SRC.mkdir()
random.seed(3)
# 半可压缩数据（重复文本 + 随机），才能看出级别差异
(SRC / "大.txt").write_text(("杨三海解压测试文本 hello world 12345\n" * 4000)
                            + bytes(random.getrandbits(8) for _ in range(20000)).hex(),
                            encoding="utf-8")

proc = subprocess.Popen([BIN, "--serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, text=True, bufsize=1)
_id = 0


def 发(**kw):
    global _id
    _id += 1
    proc.stdin.write(json.dumps({"id": _id, **kw}, ensure_ascii=False) + "\n")
    proc.stdin.flush()
    return _id


def 收(mid, 超时=180):
    import time
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


# 造一个 mp4 外壳（pack 伪装模式要求 cover；用 tail 布局省事：随便给个文件）
外壳 = W / "壳.mp4"
外壳.write_bytes(b"\x00\x00\x00\x18ftypisom" + b"\x00" * 4000)

# ── 1. 默认级别 ──
o1 = W / "默认.mp4"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(o1), cover=str(外壳)))
断言(r["ok"], f"默认打包成功：{r.get('error')}")
断言(r["result"]["level"] == 6, f"默认 level=6（实际 {r['result']['level']}）")

# ── 2. level 0 vs 9 ──
o0 = W / "存储.mp4"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(o0), cover=str(外壳), level=0))
断言(r["ok"], f"level=0 打包成功：{r.get('error')}")
o9 = W / "最小.mp4"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(o9), cover=str(外壳), level=9))
断言(r["ok"], f"level=9 打包成功：{r.get('error')}")
断言(o0.stat().st_size > o9.stat().st_size,
   f"仅存储({o0.stat().st_size} B) 比最小体积({o9.stat().st_size} B) 大")

# level=9 的包要能解开（别为了省体积写出坏包）
d9 = W / "解9"
r = 收(发(cmd="extract", path=str(o9), dest=str(d9), cover=False))
断言(r["ok"], f"level=9 的包能解开：{r.get('error')}")
断言((d9 / "内容" / "大.txt").exists() or (d9 / "大.txt").exists(),
   f"解出大.txt（实际 {[p.name for p in d9.rglob('*') if p.is_file()]}）")

# ── 3. 越界必须报错 ──
o10 = W / "越界.mp4"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(o10), cover=str(外壳), level=10))
断言(not r["ok"], "level=10 被拒绝")
断言(r["error"]["code"] == "BAD_PARAM", f"错误码 BAD_PARAM（实际 {r['error']['code']}）")
断言(not o10.exists(), "越界参数没有产出文件")

proc.stdin.close()
proc.wait(timeout=10)

# ── 4. CLI ──
cli9 = W / "cli9.mp4"
p = subprocess.run([BIN, "ys", str(SRC), "-o", str(cli9), "--cover", str(外壳),
                    "--level", "9"], capture_output=True, text=True, timeout=180)
断言(p.returncode == 0, f"CLI --level 9 退出码 0（实际 {p.returncode}）\n{p.stdout[-500:]}{p.stderr[-500:]}")
断言(cli9.exists() and cli9.stat().st_size > 0, "CLI 产物非空")

p = subprocess.run([BIN, "ys", str(SRC), "-o", str(W / "cli10.mp4"), "--cover", str(外壳),
                    "--level", "10"], capture_output=True, text=True, timeout=60)
断言(p.returncode == 2, f"CLI --level 10 退出码 2（实际 {p.returncode}）")
断言("0-9" in (p.stdout + p.stderr), f"报错里说清范围：{(p.stdout + p.stderr).strip()[:120]}")

print("\n全部通过（level 协议 + CLI 共 8 组断言）")
