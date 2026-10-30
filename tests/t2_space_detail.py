#!/usr/bin/env python3
"""探针：空间不足时协议错误对象必须带 need / available。

这是 2026-10-30 真机撞出来的坑：引擎侧 `JobOutcome.error_detail` 有了，
但 api 层无条件重新 identify，而空间不足时 identify 是成功的 →
走兜底分支，detail 全丢，界面只能说一句「清一点空间再试」。

用法：python3 t2_space_detail.py <引擎路径> <3GB包的路径>
"""
import json
import os
import subprocess
import sys

引擎 = sys.argv[1]
包 = sys.argv[2] if len(sys.argv) > 2 else ""

# 这条探针要一个「解压后约 3 GB、压缩后几 MB」的包，现造要十几秒 ——
# 默认跳过，设 YSHJY_SPACE_PROBE_PKG=<包路径> 才跑（与 --slow 项同样的口径，别假装跑过）
if not 包 or 包 == "/dev/null" or not os.path.exists(包):
    print("⏭ 跳过：没给 3 GB 测试包（参见 tests/all.sh 顶部说明）")
    sys.exit(0)

p = subprocess.Popen(
    [引擎, "--serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
    encoding="utf-8",
)

p.stdin.write(json.dumps({"id": 1, "cmd": "extract", "path": 包}, ensure_ascii=False) + "\n")
p.stdin.flush()

错误 = None
事件里 = None
for line in p.stdout:
    line = line.strip()
    if not line:
        continue
    ev = json.loads(line)
    # --serve 走平铺形状：{"kind":"SPACE_SHORTAGE","data":{...}}
    # （`--json` 才是 {"事件名":{...}} —— 两条通道形状不同，别照另一条写）
    if ev.get("kind") == "SPACE_SHORTAGE":
        事件里 = ev  # 整行留着：不同通道/版本把字段放哪不一定，别假设 data 里
    if ev.get("id") == 1 and ev.get("ok") is False:
        错误 = ev.get("error") or {}
        break

p.stdin.close()
try:
    p.wait(timeout=10)
except subprocess.TimeoutExpired:
    p.kill()

失败 = []
if 错误 is None:
    失败.append("没收到带 id=1 的失败响应")
else:
    if 错误.get("code") != "NOT_ENOUGH_SPACE":
        失败.append(f"错误码不对：{错误.get('code')}")
    detail = 错误.get("detail") or {}
    if "need" not in detail:
        失败.append(f"detail 里没有 need（实际：{detail}）")
    if "available" not in detail:
        失败.append(f"detail 里没有 available（实际：{detail}）")

if 事件里 is None:
    失败.append("没收到 SpaceShortage 事件")
else:
    平 = json.dumps(事件里, ensure_ascii=False)
    if '"needed"' not in 平 or '"available"' not in 平:
        失败.append(f"事件里的数字不全：{平}")

if 失败:
    print("❌ 失败：")
    for f in 失败:
        print("   -", f)
    print("原始错误：", 错误)
    sys.exit(1)

print(f"✅ 错误 detail = {错误.get('detail')}")
print(f"✅ 事件 = {事件里}")
print("✅ 三数所需的两个数字，两条路都通")
