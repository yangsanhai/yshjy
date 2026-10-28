#!/usr/bin/env python3
"""「只取封面」协议探针（2026-10-28）

验收标准（docs/TODO.md §界面侧第一条）：
  拖入伪装包 → 只取封面 → 落地一个能正常播放的原版视频，**不解压内层**。

这个探针挡的是两类真问题：
  1. **落点不受控**（本次修掉的 bug）：`-o` / 协议 `dest` 被忽略，封面副本撒在源包旁边 ——
     只读目录直接失败，可写目录越跑越多 `封面 (35).mp4`（实测在 tests/data 里堆了 36 个）。
  2. **「只取封面」偷偷解压**：只要 `extract:false` 还写出内层文件，用户勾选项就是假的。

用法：
    python3 tests/protocol_cover_only.py [path/to/yshjy]
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

# all.sh 用 `env YSHJY_BIN=…` 传路径，其它探针也这个约定
BIN = sys.argv[1] if len(sys.argv) > 1 else os.environ.get(
    "YSHJY_BIN", "target/release/yshjy")
ROOT = pathlib.Path(__file__).resolve().parent.parent
SAMPLE = ROOT / "tests" / "data" / "A1_steg_modeA_plain.mp4"
if not SAMPLE.exists():
    print(f"❌ 缺样本 {SAMPLE}（python3 tests/make_samples.py 生成）")
    sys.exit(1)

W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_coveronly_"))
# 样本复制到临时目录再动：探针绝不能往 tests/data 里落东西
SRC_DIR = W / "源包"
SRC_DIR.mkdir()
PKG = SRC_DIR / "游戏.mp4"
shutil.copyfile(SAMPLE, PKG)

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


def 目录里的文件(d):
    return sorted(p.relative_to(d).as_posix() for p in pathlib.Path(d).rglob("*") if p.is_file())


发(cmd="hello")
收(1)

# ── ① 只取封面：落在 dest 里、内层一个文件都不解 ──
out1 = W / "只要封面"
r = 收(发(cmd="extract", path=str(PKG), dest=str(out1), cover=True, extract=False))
断言(r.get("ok"), f"cover:true + extract:false 成功（{r.get('error')}）")
res = r["result"]
断言(res.get("outcome") == "Success", f"结果 outcome=Success（{res.get('outcome')}）")
断言(res.get("entries_done") == 0, "entries_done = 0（没解任何条目）")

cover = res.get("cover") or {}
断言(bool(cover.get("path")), "结果里带回封面路径（界面靠它显示「视频已分离」）")
cover_path = pathlib.Path(cover.get("path", ""))
断言(cover_path.exists(), f"封面文件真的落地了：{cover_path}")
断言(cover_path.parent == out1, f"封面落在 -o / dest 目录里（实际 {cover_path.parent}）")
断言(cover_path.stat().st_size == cover.get("size"),
     f"封面大小与结果一致（{cover_path.stat().st_size}）")
断言(cover_path.read_bytes()[4:8] == b"ftyp", "封面是合法 MP4（ftyp 头，能播）")

文件 = 目录里的文件(out1)
断言(文件 == [cover_path.name], f"dest 里只有封面一个文件，内层没解（{文件}）")

# 源包旁边不许有副本 —— 老 bug 的现场
断言(目录里的文件(SRC_DIR) == ["游戏.mp4"],
     f"源包目录里只有原包、没有副本（{目录里的文件(SRC_DIR)}）")

# ── ② 反证：extract:true + cover:false 走完整解压，不该有封面 ──
out2 = W / "解压"
r = 收(发(cmd="extract", path=str(PKG), dest=str(out2), cover=False, extract=True))
断言(r.get("ok"), f"cover:false + extract:true 成功（{r.get('error')}）")
res2 = r["result"]
断言(res2.get("entries_done", 0) > 0, f"解出了条目（{res2.get('entries_done')} 个）")
断言(not res2.get("cover"), "没要封面时结果里不带 cover 字段")
断言(not any("封面" in f for f in 目录里的文件(out2)),
     f"解压产物里没有封面文件（{目录里的文件(out2)[:3]}…）")

# ── ③ 老行为保持：不给 dest 时仍落在包旁边（别为了修 bug 把默认改掉）──
r = 收(发(cmd="extract", path=str(PKG), cover=True, extract=False))
断言(r.get("ok"), f"不给 dest 也能成功（{r.get('error')}）")
旁边 = [f for f in 目录里的文件(SRC_DIR) if "封面" in f]
断言(len(旁边) == 1, f"不给 dest 时封面落在包旁边（{旁边}）")
断言(not (SRC_DIR / "游戏.mp4.yshjy-part").exists(), "没留下临时文件")

# ── ④ 两次都到同一个 dest：第二次改名，之前那份不动 ──
r = 收(发(cmd="extract", path=str(PKG), dest=str(out1), cover=True, extract=False))
断言(r.get("ok"), "重复取封面不报错（自动改名）")
名字 = 目录里的文件(out1)
断言(len(名字) == 2, f"同名不覆盖，出现第二份（{名字}）")
断言(any("(1)" in n for n in 名字), "第二份带 (1) 后缀")
断言(cover_path.exists(), "第一份还在（没被冲掉）")

# ── ⑤ 解压 + 取封面一起来：封面在 dest 根，内层在子目录 ──
out3 = W / "都要"
r = 收(发(cmd="extract", path=str(PKG), dest=str(out3), cover=True, extract=True))
断言(r.get("ok"), f"cover + extract 同时要成功（{r.get('error')}）")
res3 = r["result"]
断言(bool(res3.get("cover")), "同时要时也带回封面")
断言(pathlib.Path(res3["cover"]["path"]).parent == out3,
     f"封面在 dest 根目录（{pathlib.Path(res3['cover']['path']).parent}）")
# 内层产物与封面共存于同一目录（实测行为：显式给 dest 时直接铺在 dest 下，
# 不再套一层包名子目录 —— 探针别按直觉写成 startswith("包名/")）
文件3 = 目录里的文件(out3)
断言(any("/" in f for f in 文件3), f"内层文件解在 dest 下（{文件3[:3]}…）")
断言(pathlib.Path(res3["cover"]["path"]).name in 文件3,
     "封面与内层产物共存同一个 dest，互不覆盖")

proc.stdin.close()
proc.wait(timeout=10)
print(f"\n全部通过。测试目录：{W}")
