#!/usr/bin/env python3
"""探针：协议 `report` 的 `out_dir`（诊断包落哪个目录）—— T7 / E6。

背景：`report.out_dir` 引擎里一直有，界面却只传 `path` → 引擎退回
**进程当前目录**。GUI 从哪启动就落在哪（`/` 或 `~`），用户回头根本找不着，
而诊断包正是「要拿去发给别人」的那份文件。E6 改成界面一律传「下载」，
但这条协议行为此前只有 widget 测试守着（验界面传参），引擎那侧没人管。

三次真进程（每次都把 `XDG_CONFIG_HOME` / `XDG_STATE_HOME` 指到临时目录，
**绝不碰用户真实配置与真实崩溃记录**）：
  ① 给 out_dir      → 产物落在那儿、cwd 干净、`result.path` 指过去
  ② 不给 out_dir    → 落进程 cwd（历史行为，别被顺手改成「按包旁边」）
  ③ 给不存在的目录  → ok:false 且两处都没产物（不能悄悄落到别处，
                       否则用户以为生成了，去那个目录里找不到）

⚠️ 响应里的路径在 `result.path`（不是 `data`）—— 先打一次原始响应再改断言，
别照另一条通道的形状猜。

用法：python3 tests/report_outdir_proto.py <引擎路径>
（取二进制的顺序：验新代码请显式传 `target/debug/yshjy`；
  `target/release/yshjy` 往往是上次发版留下的旧版。）
"""
import glob
import json
import os
import subprocess
import sys
import tempfile
import zipfile

引擎 = sys.argv[1] if len(sys.argv) > 1 else "yshjy"
# 本探针会把子进程的 cwd 换到临时目录 → 引擎路径必须先转成绝对路径，
# 否则 `target/debug/yshjy` 这种相对路径在新 cwd 里根本不存在。
if os.sep in 引擎:
    引擎 = os.path.abspath(引擎)

失败 = []


def 断言(条件, 说明):
    if 条件:
        print(f"  ✅ {说明}")
    else:
        print(f"  ❌ {说明}")
        失败.append(说明)


def 跑一条(命令, cwd, 环境):
    p = subprocess.Popen(
        [引擎, "--serve"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
        cwd=cwd,
        env=环境,
    )
    assert p.stdin is not None and p.stdout is not None
    try:
        p.stdin.write(json.dumps(命令, ensure_ascii=False) + "\n")
        p.stdin.flush()
        for line in p.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                响应 = json.loads(line)
            except json.JSONDecodeError:
                continue
            if 响应.get("id") == 命令["id"]:
                return 响应
        return {}
    finally:
        p.stdin.close()
        p.wait(timeout=15)


def 清空目录里(目录):
    for f in glob.glob(os.path.join(目录, "yshjy-report-*.zip")):
        os.remove(f)


def 目录里的包(目录):
    return [os.path.basename(p) for p in glob.glob(os.path.join(目录, "yshjy-report-*.zip"))]


def main():
    根 = tempfile.mkdtemp(prefix="yshjy_report_outdir_")
    配置 = os.path.join(根, "cfg")
    cwd目录 = os.path.join(根, "cwd")
    输出目录 = os.path.join(根, "输出")
    for d in (配置, cwd目录, 输出目录):
        os.makedirs(d)
    隔离 = dict(
        os.environ,
        XDG_CONFIG_HOME=配置,
        XDG_STATE_HOME=os.path.join(根, "state"),
    )

    # 目标包：一个正常的 ZIP，够引擎识别就行（诊断包不读内容）
    包 = os.path.join(根, "样本.zip")
    with zipfile.ZipFile(包, "w") as z:
        z.writestr("a.txt", "hi\n")

    # ① 给 out_dir
    清空目录里(cwd目录)
    响应 = 跑一条({"id": 1, "cmd": "report", "path": 包, "out_dir": 输出目录}, cwd目录, 隔离)
    断言(响应.get("ok") is True, f"带 out_dir：成功（{响应.get('error')}）")
    落地 = 目录里的包(输出目录)
    断言(len(落地) == 1, f"带 out_dir：产物落在这个目录里（{落地}）")
    断言(目录里的包(cwd目录) == [], "带 out_dir：cwd 里没有多余产物")
    路径 = (响应.get("result") or {}).get("path")
    断言(
        isinstance(路径, str) and os.path.dirname(路径) == 输出目录,
        f"带 out_dir：result.path 指回同一个目录（{路径}）",
    )
    断言(
        isinstance(路径, str) and os.path.exists(路径),
        "带 out_dir：result.path 那个文件真的存在",
    )

    # ② 不给 out_dir —— 历史行为：落进程 cwd
    清空目录里(cwd目录)
    响应 = 跑一条({"id": 1, "cmd": "report", "path": 包}, cwd目录, 隔离)
    断言(响应.get("ok") is True, f"不给 out_dir：成功（{响应.get('error')}）")
    断言(len(目录里的包(cwd目录)) == 1, "不给 out_dir：落在进程 cwd（老行为，界面靠传参绕开它）")

    # ③ 给一个不存在的目录 —— 必须报错，不能悄悄落到别处
    清空目录里(cwd目录)
    不存在 = os.path.join(根, "没有这个目录")
    响应 = 跑一条({"id": 1, "cmd": "report", "path": 包, "out_dir": 不存在}, cwd目录, 隔离)
    断言(响应.get("ok") is False, "目录不存在：明确报错（不是假装成功）")
    断言(目录里的包(cwd目录) == [], "目录不存在：cwd 里也没偷偷生成一份")

    print()
    if 失败:
        print(f"❌ {len(失败)} 项失败")
        return 1
    print("✅ 全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
