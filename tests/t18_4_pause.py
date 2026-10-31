#!/usr/bin/env python3
"""探针：队列「全部暂停 / 继续」（T18-4，2026-11-01 做）

背景：界面以前只有「移出队列」（单条），没有整体暂停。用户要的是**真暂停**：
停了还能接着跑，不是「全部取消」。引擎这边挂在两个地方 ——
`ctx::entry_done`（每解完一个文件，所有格式共用）和 `batch` 循环的任务之间。

★ 这个探针要防的假绿（都是真见过的那几种）：
  · 「收到 pause 回执」就当成功了 —— 回执只代表**请求送到**，引擎要等当前
    这个文件解完才停。所以必须等 `PAUSED` 事件，并且**看数字不再增长**。
  · 「文件数不动」可能是它已经跑完了（任务太快）—— 所以对照组必须先证明
    这个包确实够跑一会儿，且暂停发生在中途。
  · 「暂停后能继续」不代表没写坏 —— 所以最终按每个文件的 sha256 与
    不暂停那次逐一对齐。

断言（全部真二进制、真 --serve、真文件）：
  1. `hello` 里认识 `pause` / `resume` 命令，事件种类里有 `PAUSED`
  2. 对照组（不暂停）：batch 正常跑完，没被挂在半路
  3. 暂停回执 `delivered=true`
  4. 收到 `PAUSED{paused:true}`
  5. PAUSED 之后 1 秒内 `ENTRY_FINISHED` **一条都不再增长**（真停住了）
  6. resume 后收到 `PAUSED{paused:false}`，且文件数接着增长
  7. 最终产物与不暂停那次**逐文件 sha256 一致**（暂停不丢数据、不写坏）
  8. 暂停中再点取消：必须能退出（结局 cancelled），**不能卡在条件变量上**

用法：python3 tests/t18_4_pause.py [二进制路径]
（验新代码请显式传 target/debug/yshjy；target/release 往往还是上次发版的旧版）
"""
from __future__ import annotations

import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("YSHJY_BIN")
       or next((str(ROOT / "target" / rel) for rel in
                ("debug/yshjy.exe", "debug/yshjy", "release/yshjy.exe", "release/yshjy")
                if (ROOT / "target" / rel).exists()), ""))
if not BIN:
    print("❌ 找不到 yshjy 二进制（先 cargo build）")
    sys.exit(1)
BIN = os.path.abspath(BIN)

n_pass = n_fail = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✅ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ❌ {msg}")


def 断言(条件, msg):
    (ok if 条件 else bad)(msg)
    return 条件


# ───────────────────────── 样本 ─────────────────────────

W = tempfile.mkdtemp(prefix="yshjy_pause.", dir=str(ROOT / "target"))
# 300 个 300 KB 的文件：条目**多**才好暂停（挂起点是「每个条目解完」），
# 而且目标放在 target/（真实磁盘）而不是 /tmp —— /tmp 是 tmpfs 内存盘，
# 87 MB 写进去只要 0.3 秒，暂停根本来不及落在中途（第一版就栽在这）。
N_FILES = 300
FILE_SIZE = 300 * 1024
包 = os.path.join(W, "many.zip")
内容表 = {}
with zipfile.ZipFile(包, "w", zipfile.ZIP_STORED) as z:
    for i in range(N_FILES):
        data = os.urandom(FILE_SIZE)
        名字 = f"f{i:03d}.bin"
        内容表[名字] = hashlib.sha256(data).hexdigest()
        z.writestr(名字, data)


def 目录指纹(根):
    """目录里每个文件的 sha256（用来证明「暂停不写坏」）"""
    结果 = {}
    for dirpath, _, files in os.walk(根):
        for f in files:
            p = os.path.join(dirpath, f)
            rel = os.path.relpath(p, 根)
            h = hashlib.sha256()
            with open(p, "rb") as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b""):
                    h.update(chunk)
            结果[rel] = h.hexdigest()
    return 结果


# ───────────────────── 一条 --serve 交互壳 ─────────────────────

class 会话:
    def __init__(self):
        self.p = subprocess.Popen(
            [BIN, "--serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.行 = queue.Queue()
        self.全部事件 = []
        threading.Thread(target=self._读, daemon=True).start()
        self.下一个id = 1

    def _读(self):
        assert self.p.stdout is not None
        for line in self.p.stdout:
            self.行.put(line)

    def 发(self, obj):
        self.下一个id += 1
        obj = dict(obj)
        obj.setdefault("id", self.下一个id)
        assert self.p.stdin is not None
        self.p.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
        self.p.stdin.flush()
        return obj["id"]

    def 取行(self, 超时=15.0):
        try:
            return self.行.get(timeout=超时)
        except queue.Empty:
            return None

    def 等响应(self, req_id, 超时=15.0):
        """读到指定 id 的响应；路上的事件收进 self.全部事件"""
        截止 = time.time() + 超时
        while time.time() < 截止:
            line = self.取行(截止 - time.time())
            if line is None:
                return None
            line = line.strip()
            if not line:
                continue
            try:
                o = json.loads(line)
            except json.JSONDecodeError:
                continue
            if o.get("id") == req_id and "ok" in o:
                return o
            self.全部事件.append(o)
        return None

    def 等事件(self, 判定, 超时=30.0):
        """一直读到某条事件满足判定为止（返回它）；路上收到的进 全部事件"""
        截止 = time.time() + 超时
        while time.time() < 截止:
            line = self.取行(截止 - time.time())
            if line is None:
                return None
            line = line.strip()
            if not line:
                continue
            try:
                o = json.loads(line)
            except json.JSONDecodeError:
                continue
            self.全部事件.append(o)
            if 判定(o):
                return o
        return None

    def 关闭(self):
        try:
            self.p.kill()
        except Exception:
            pass


def 事件名(o):
    # 真实事件行形如 {"kind":"ENTRY_FINISHED","type":"event","event":{"EntryFinished":{...}}}
    # —— 认 `kind`（大写），不要拿 `event` 里那个 CamelCase 键名去比
    if o.get("kind"):
        return o["kind"]
    ev = o.get("event")
    if isinstance(ev, dict) and ev:
        return next(iter(ev.keys()))
    return None


def 事件体(o):
    ev = o.get("event")
    if isinstance(ev, dict) and ev:
        内 = next(iter(ev.values()))
        if isinstance(内, dict):
            return 内
    return {}


def 数(会话对象, 名字):
    return sum(1 for e in 会话对象.全部事件 if 事件名(e) == 名字)


print(f"引擎：{BIN}")
print(f"样本：{N_FILES} 个文件 × {FILE_SIZE // 1024} KB = {N_FILES * FILE_SIZE // 1024 // 1024} MB")

try:
    # ── ① hello 里有没有 pause/resume ──
    s = 会话()
    你好 = s.等响应(s.发({"cmd": "hello"}))
    命令们 = (你好 or {}).get("result", {}).get("features") or []
    种类们 = (你好 or {}).get("result", {}).get("event_kinds") or []
    断言("pause" in 命令们 and "resume" in 命令们, "hello 认识 pause / resume 命令")
    断言("PAUSED" in 种类们, "hello 的 event_kinds 里有 PAUSED")
    s.关闭()

    # ── ② 对照组：不暂停也不能被卡住 ──
    对照片 = os.path.join(W, "out_ref")
    s = 会话()
    s.等响应(s.发({"cmd": "hello"}))
    t0 = time.time()
    rid = s.发({"cmd": "batch", "paths": [包], "dest": 对照片})
    结果 = s.等响应(rid, 超时=120)
    用时 = time.time() - t0
    s.关闭()
    断言(结果 is not None, f"对照组 batch 跑完了（{用时:.1f} 秒）")
    断言(数(s, "PAUSED") == 0, "没点暂停时不该冒出 PAUSED 事件")
    参考指纹 = 目录指纹(对照片)
    断言(len(参考指纹) == N_FILES, f"对照组产物齐全（{len(参考指纹)} / {N_FILES} 个文件）")
    # 智能解压会建一层目录（many/...），所以按「文件名」比内容，不按完整相对路径
    参考按名 = {os.path.basename(k): v for k, v in 参考指纹.items()}
    断言(参考按名 == 内容表, "对照组产物内容与源一致（sha256 逐个对上）")
    # 对照组跑多快不重要（page cache 让 87 MB 也能 0.3 秒写完），
    # 真正证明「暂停落在中途」的是下面那条 `0 < 停点 < N_FILES`
    print(f"     （对照组耗时 {用时:.2f} 秒，仅供参考）")

    # ── ③~⑦ 暂停 → 真停住 → 继续 → 跑完且不写坏 ──
    暂停图 = os.path.join(W, "out_pause")
    s = 会话()
    s.等响应(s.发({"cmd": "hello"}))
    rid = s.发({"cmd": "batch", "paths": [包], "dest": 暂停图})

    # 第一个条目解完就点暂停 —— 这时后面还有 299 个没解
    第一 = s.等事件(lambda o: 事件名(o) == "ENTRY_FINISHED")
    断言(第一 is not None, "等到第一个 ENTRY_FINISHED（任务确实开跑了）")
    回执 = s.等响应(s.发({"cmd": "pause"}), 超时=10)
    结果体 = (回执 or {}).get("result") or {}
    断言(结果体.get("delivered") is True, f"pause 回执 delivered=true（{结果体}）")

    已停 = s.等事件(lambda o: 事件名(o) == "PAUSED" and 事件体(o).get("paused") is True, 超时=60)
    断言(已停 is not None, "收到 PAUSED{paused:true}（引擎真的停住了）")

    停点 = 数(s, "ENTRY_FINISHED")
    time.sleep(1.2)
    # 这 1.2 秒里可能有残留事件在队列里，读干净再数
    while True:
        line = s.取行(0.2)
        if line is None:
            break
        try:
            s.全部事件.append(json.loads(line))
        except Exception:
            pass
    停点后 = 数(s, "ENTRY_FINISHED")
    断言(停点后 == 停点, f"暂停期间文件数不再增长（{停点} → {停点后}）")
    断言(0 < 停点 < N_FILES, f"暂停确实发生在中途（已解 {停点} / {N_FILES}），不是已经跑完")

    s.等响应(s.发({"cmd": "resume"}), 超时=10)
    已续 = s.等事件(lambda o: 事件名(o) == "PAUSED" and 事件体(o).get("paused") is False, 超时=10)
    断言(已续 is not None, "收到 PAUSED{paused:false}（恢复被确认）")

    结果 = s.等响应(rid, 超时=180)
    s.关闭()
    断言(结果 is not None, "继续之后 batch 正常跑完")
    暂停指纹 = 目录指纹(暂停图)
    断言(暂停指纹 == 参考指纹,
         f"暂停过的产物与不暂停逐文件一致（{len(暂停指纹)} 个文件）")
    断言(数(s, "ENTRY_FINISHED") >= N_FILES, "最终解完的条目数不少于文件数")

    # ── ⑧ 暂停中取消：必须能退出，不能卡死 ──
    取消图 = os.path.join(W, "out_cancel")
    s = 会话()
    s.等响应(s.发({"cmd": "hello"}))
    rid = s.发({"cmd": "batch", "paths": [包], "dest": 取消图})
    s.等事件(lambda o: 事件名(o) == "ENTRY_FINISHED")
    s.等响应(s.发({"cmd": "pause"}), 超时=10)
    s.等事件(lambda o: 事件名(o) == "PAUSED" and 事件体(o).get("paused") is True, 超时=60)
    t0 = time.time()
    s.等响应(s.发({"cmd": "cancel"}), 超时=10)
    结果 = s.等响应(rid, 超时=20)
    退出用时 = time.time() - t0
    s.关闭()
    断言(结果 is not None,
         f"暂停中点取消能退出（{退出用时:.1f} 秒，没卡在条件变量上）")
    结局 = [j.get("outcome") for j in ((结果 or {}).get("result") or {}).get("jobs") or []]
    断言(any(x == "cancelled" for x in 结局) or 结局 == [],
         f"结局是 cancelled 而不是假装成功（{结局}）")

finally:
    shutil.rmtree(W, ignore_errors=True)

print()
print(f"通过 {n_pass} / 失败 {n_fail}")
sys.exit(0 if n_fail == 0 else 1)
