#!/usr/bin/env python3
"""协议冒烟测试 —— 也是「前端该怎么接」的可执行参考实现。

用法：
    python3 tests/protocol_smoke.py                    # 默认用 target/debug/yshjy
    python3 tests/protocol_smoke.py path/to/yshjy      # 指定二进制（发布包里的也行）

它做的事：启动 `yshjy --serve`，通过 stdout/stdin 说话，
把 hello / list / extract / pack 四条命令各跑一遍，并检查事件有没有来。

前端（Flutter / Tauri / 网页）只要照这个循环写，就能驱动整个引擎。
"""
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import zipfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/debug/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_proto_"))

# ── 造一个带中文名的 zip ──
z = W / "试用.zip"
with zipfile.ZipFile(z, "w") as f:
    f.write(__file__, "脚本.py")
    f.writestr("空.txt", "hello 协议")

proc = subprocess.Popen(
    [BIN, "--serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
    bufsize=1,
)


def 发(obj):
    proc.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
    proc.stdin.flush()


def 收(msg_id, 超时=30):
    """读到这个 id 的响应为止；路上收到的事件原样收集返回。"""
    事件 = []
    截止 = time.time() + 超时
    while time.time() < 截止:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("引擎把管道关了")
        v = json.loads(line)
        if v.get("type") == "event":
            事件.append(v)
            continue
        if v.get("id") == msg_id:
            return v, 事件
    raise TimeoutError(f"等 id={msg_id} 超时（已经收到 {len(事件)} 条事件）")


def 断言(条件, 说明):
    if not 条件:
        raise AssertionError(说明)
    print(f"  ✓ {说明}")


print("== 1. 握手 ==")
发({"id": 1, "cmd": "hello"})
r, _ = 收(1)
断言(r["ok"], "hello 返回 ok")
断言(r["result"]["protocol"] == 1, "协议版本是 1")
断言("list" in r["result"]["features"], "能力表里有 list")
print(f"    引擎 {r['result']['engine']}，事件类型 {len(r['result']['event_kinds'])} 种")

print("== 2. 列内容 ==")
发({"id": 2, "cmd": "list", "path": str(z)})
r, 事件 = 收(2)
断言(r["ok"], "list 返回 ok")
断言(len(r["result"]["entries"]) == 2, "列出 2 个条目")
断言(any(e["path"].endswith("空.txt") for e in r["result"]["entries"]), "中文名没乱码")
断言(any(e["path"].endswith("脚本.py") for e in r["result"]["entries"]), "路径正确")

print("== 3. 解压（并检查事件）==")
发({"id": 3, "cmd": "extract", "path": str(z), "dest": str(W / "out")})
r, 事件 = 收(3, 60)
断言(r["ok"], f"extract 返回 ok（{r.get('error')}）")
断言((W / "out").exists(), "输出目录建出来了")
找到 = list((W / "out").rglob("空.txt"))
断言(len(找到) == 1, "文件真的解出来了")
种类 = [e["kind"] for e in 事件]
断言("ENTRY_FINISHED" in 种类, "收到了条目完成事件")
断言("JOB_STARTED" in 种类 or "STAGE_STARTED" in 种类, "收到了阶段/任务事件")

print("== 4. 打包成 PNG 伪装图 ==")
发({"id": 4, "cmd": "pack", "inputs": [str(W / "out")], "out": str(W / "包.png")})
r, _ = 收(4, 60)
断言(r["ok"], "pack 返回 ok")
断言(r["result"]["layout"] == "TailAppend", "图片封面自动用尾部追加布局")
头 = (W / "包.png").read_bytes()[:8]
断言(头 == b"\x89PNG\r\n\x1a\n", "产物是真 PNG（不是 MP4 头冒充）")

print("== 5. 把伪装图再列一次 ==")
发({"id": 5, "cmd": "list", "path": str(W / "包.png")})
r, _ = 收(5)
断言(r["ok"], "伪装图能被我们认出来")
断言(len(r["result"]["entries"]) >= 1, "里面的条目列得出来")

print("== 6. 错误处理：不存在的文件 ==")
发({"id": 6, "cmd": "list", "path": str(W / "根本没有这个文件.mp4")})
r, _ = 收(6)
断言(r["ok"] is False, "返回失败而不是崩掉")
码 = r["error"]["code"]
断言(码.isupper() and "_" in 码 or 码.isupper(), f"错误码是稳定标识（{码}）")

print("== 7. 未知命令 ==")
发({"id": 7, "cmd": "fly-to-moon"})
r, _ = 收(7)
断言(r["ok"] is False and r["error"]["code"] == "UNKNOWN_COMMAND", "未知命令有明确错误码")

proc.stdin.close()
proc.wait(timeout=10)
print("\n全部通过 ✅（协议 v1）")
print(f"临时目录：{W}")
