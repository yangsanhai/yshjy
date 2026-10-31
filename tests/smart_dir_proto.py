#!/usr/bin/env python3
"""探针：协议 `extract` 的 `smart_dir` 开关（T7 / E5）。

背景：`--no-smart-dir` 命令行一直有，但协议里**单条 extract** 的 `smart_dir` 被写死
`true`（只有 `batch` 有这个字段）。界面只走协议，所以这个能力在图形界面里表达不了 ——
用户在打包/解压页上没有任何办法改「要不要多套一层文件夹」。

⚠️ 必须**不给 dest**：给了 dest 两种模式都把产物直接铺在 dest 下（历史行为），
看不出区别；只有解到「包旁边」时才有得比。

三种情况各跑一次真进程：
  ① 不传            → 智能：包里只有单一顶层目录 → **不额外套层**
  ② smart_dir:false → 关掉智能：**永远多套一层「包名」文件夹**
  ③ smart_dir:true  → 与 ① 完全一致（显式 true 不能与默认不一致）

用法：python3 tests/smart_dir_proto.py <引擎路径>
（取二进制的顺序提醒：验新代码请显式传 `target/debug/yshjy`，
  `target/release/yshjy` 往往是上次发版留下的旧版 —— 那样验的是旧代码。）
"""
import json
import os
import subprocess
import sys
import tempfile
import zipfile

引擎 = sys.argv[1] if len(sys.argv) > 1 else "yshjy"


def 造包(目录):
    """包里只有一个顶层目录 —— 智能解压三规则的第 1 条就是冲它来的"""
    包 = os.path.join(目录, "样本.zip")
    with zipfile.ZipFile(包, "w") as z:
        z.writestr("顶层/甲.txt", "hello 杨三海\n")
        z.writestr("顶层/乙.txt", "world\n")
    return 包


def 跑(包, 参数):
    """起一个真 --serve，发一条 extract，读到 id=1 的响应为止"""
    p = subprocess.Popen(
        [引擎, "--serve"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
    )
    assert p.stdin is not None and p.stdout is not None
    try:
        p.stdin.write(
            json.dumps({"id": 1, "cmd": "extract", "path": 包, **参数},
                       ensure_ascii=False) + "\n"
        )
        p.stdin.flush()
        for line in p.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("id") == 1:
                return ev
        return None
    finally:
        p.stdin.close()
        try:
            p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            p.kill()


def 树(根):
    out = []
    for dp, _, fn in os.walk(根):
        for f in fn:
            out.append(os.path.relpath(os.path.join(dp, f), 根).replace(os.sep, "/"))
    return sorted(out)


失败 = []


def 场景(标题, 参数, 期望, 反期望):
    with tempfile.TemporaryDirectory(prefix="yshjy-smart-") as d:
        包 = 造包(d)
        响应 = 跑(包, 参数)
        if 响应 is None or not 响应.get("ok"):
            # 记住：先给实测命令与输出，再说结论
            失败.append(f"{标题}：extract 失败（响应={响应}）")
            return
        files = 树(d)
        命中 = [f for f in files if 期望 in f]
        误中 = [f for f in files if 反期望 in f]
        if not 命中:
            失败.append(f"{标题}：没找到「{期望}」，实测树={files}")
        elif 反期望 and 误中:
            失败.append(f"{标题}：不该出现「{反期望}」，实测树={files}")
        else:
            print(f"  ✅ {标题}：{files}")


print("① 不传 smart_dir（默认智能）")
场景("默认", {}, "顶层/甲.txt", "样本/顶层/甲.txt")

print("② smart_dir:false（永远套一层包名）")
场景("关掉智能", {"smart_dir": False}, "样本/顶层/甲.txt", "")

print("③ smart_dir:true（显式 true 必须与默认一致）")
场景("显式 true", {"smart_dir": True}, "顶层/甲.txt", "样本/顶层/甲.txt")

if 失败:
    print(f"\n❌ {len(失败)} 项失败：")
    for m in 失败:
        print("  -", m)
    sys.exit(1)
print("\n✅ 3/3 通过：smart_dir 在协议层真的生效（默认/关/显式开 三种都对）")
