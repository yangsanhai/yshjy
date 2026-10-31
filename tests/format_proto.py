#!/usr/bin/env python3
"""探针：协议 `pack` 的 `format`（打包成 zip / tar.gz / tar.zst）—— T17-4。

背景：T17-4 之前引擎只会往 ZIP 里写东西。加 format 以后有三件事必须有人守着，
否则改坏了 `cargo test` 能全绿、界面 widget 测试也能全绿：

  ① **产物真能被第三方读**：tar.gz 用 Python 的 tarfile 读、tar.zst 用系统
     `tar --zstd` 读（Python 标准库没有 zstd），内容与源文件逐字节一致；
  ② **矩阵真的挡住了**：tar 系不允许加密 / 分卷 / 伪装 —— 这几条必须在协议层
     明确报 BAD_PARAM，不能「静默忽略」变成另一个包；
  ③ **老行为不变**：不传 format 时还是原来的伪装包（封面 + ZIP）。

还有一个只有真跑才看得见的坑（2026-10-31 实测撞出来的）：
CLI 的自检原来只认 ZIP 的 EOCD，tar 系打完会打一行「❌ 自检失败」。
所以这里额外验一条：**协议返回的 format 字段与实际产物一致**（界面靠它显示）。

用法：python3 tests/format_proto.py <引擎路径>
"""
import json
import os
import subprocess
import sys
import tempfile
import tarfile
import zipfile

引擎 = sys.argv[1] if len(sys.argv) > 1 else "yshjy"
项目根 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
封面 = os.path.join(项目根, "assets", "cover", "mini.mp4")

失败 = []


def 断言(条件, 说明):
    if 条件:
        print(f"  ✅ {说明}")
    else:
        print(f"  ❌ {说明}")
        失败.append(说明)


def 跑一条(命令):
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
        p.wait(timeout=30)


def 打包(素材, 输出, **额外):
    命令 = {"id": 1, "cmd": "pack", "inputs": [素材], "out": 输出}
    命令.update(额外)
    return 跑一条(命令)


def 读tar(路径, zst=False):
    """返回 {条目名: 内容}；zst 走系统 tar（Python 标准库没 zstd）。"""
    if not zst:
        with tarfile.open(路径, "r:gz") as t:
            表 = {}
            for m in t.getmembers():
                if m.isfile():
                    f = t.extractfile(m)
                    if f is not None:
                        表[m.name] = f.read()
            return 表
    根 = os.path.dirname(路径)
    解到 = os.path.join(根, "zst_解开")
    os.makedirs(解到, exist_ok=True)
    r = subprocess.run(
        ["tar", "--zstd", "-xf", 路径, "-C", 解到], capture_output=True, text=True
    )
    if r.returncode != 0:
        raise RuntimeError(f"tar --zstd 失败: {r.stderr.strip()}")
    表 = {}
    for 目录, _, 文件们 in os.walk(解到):
        for f in 文件们:
            p = os.path.join(目录, f)
            表[os.path.relpath(p, 解到)] = open(p, "rb").read()
    return 表


def main():
    根 = tempfile.mkdtemp(prefix="yshjy_format_proto_")
    素材 = os.path.join(根, "素材")
    os.makedirs(os.path.join(素材, "子目录"))
    甲 = "hello 格式\n" * 30
    with open(os.path.join(素材, "甲.txt"), "w", encoding="utf-8") as f:
        f.write(甲)
    with open(os.path.join(素材, "子目录", "乙.txt"), "w", encoding="utf-8") as f:
        f.write("world\n")

    print("── ① tar.gz：真打真解（Python tarfile） ──")
    out_gz = os.path.join(根, "a.tar.gz")
    响应 = 打包(素材, out_gz, mode="plain", format="tar.gz")
    断言(响应.get("ok") is True, f"打包成功（{响应.get('error')}）")
    结果 = 响应.get("result") or {}
    断言(结果.get("format") == "tar.gz", f"响应回的 format 是 tar.gz（{结果.get('format')}）")
    if os.path.exists(out_gz):
        try:
            表 = 读tar(out_gz)
            断言(表.get("素材/甲.txt") == 甲.encode(), "甲.txt 内容一致")
            断言(表.get("素材/子目录/乙.txt") == b"world\n", "子目录里的乙.txt 内容一致")
            断言(len(表) == 2, f"文件条目正好 2 个（读回 {len(表)}）")
        except Exception as e:  # noqa: BLE001
            断言(False, f"tar.gz 读得开（{e}）")
    else:
        断言(False, "tar.gz 产物存在")

    print("── ② tar.zst：真打真解（系统 tar --zstd） ──")
    out_zst = os.path.join(根, "a.tar.zst")
    响应 = 打包(素材, out_zst, mode="plain", format="tar.zst")
    断言(响应.get("ok") is True, f"打包成功（{响应.get('error')}）")
    结果 = 响应.get("result") or {}
    断言(结果.get("format") == "tar.zst", f"响应回的 format 是 tar.zst（{结果.get('format')}）")
    if os.path.exists(out_zst):
        try:
            表 = 读tar(out_zst, zst=True)
            断言(表.get("素材/甲.txt") == 甲.encode(), "甲.txt 内容一致")
            断言(表.get("素材/子目录/乙.txt") == b"world\n", "子目录里的乙.txt 内容一致")
        except Exception as e:  # noqa: BLE001
            断言(False, f"tar.zst 读得开（{e}）")
    else:
        断言(False, "tar.zst 产物存在")

    print("── ③ 普通 zip：还是标准 ZIP ──")
    out_zip = os.path.join(根, "a.zip")
    响应 = 打包(素材, out_zip, mode="plain", format="zip")
    断言(响应.get("ok") is True, f"打包成功（{响应.get('error')}）")
    if os.path.exists(out_zip):
        try:
            with zipfile.ZipFile(out_zip) as z:
                断言(z.read("素材/甲.txt") == 甲.encode(), "zip 里甲.txt 内容一致")
                断言(len(z.namelist()) == 2, f"zip 条目 2 个（读回 {len(z.namelist())}）")
        except Exception as e:  # noqa: BLE001
            断言(False, f"zip 读得开（{e}）")

    print("── ④ 老行为不变：不传 format + 伪装模式仍是 zip 伪装包 ──")
    if os.path.exists(封面):
        out_伪装 = os.path.join(根, "伪装.mp4")
        响应 = 打包(素材, out_伪装, cover=封面)
        断言(响应.get("ok") is True, f"伪装打包成功（{响应.get('error')}）")
        结果 = 响应.get("result") or {}
        断言(结果.get("format") == "zip", f"默认格式仍是 zip（{结果.get('format')}）")
        if os.path.exists(out_伪装):
            字节 = open(out_伪装, "rb").read()
            断言(字节.find(b"PK\x03\x04") > 0, "伪装包里有 ZIP 头且不在开头（封面在前）")
    else:
        print(f"  ⏭ 跳过：找不到封面 {封面}")

    print("── ⑤ 矩阵：tar 系必须明确拒绝（不能静默忽略） ──")
    用例 = [
        ("伪装模式 + tar.gz", dict(cover=封面, format="tar.gz"), "只"),
        ("tar.gz + 密码", dict(mode="plain", format="tar.gz", password="123"), "加密"),
        ("tar.zst + 分卷", dict(mode="plain", format="tar.zst", split=1024 * 1024), "分卷"),
        ("tar.gz + 自解压", dict(format="tar.gz", sfx="linux"), "自解压"),
        ("format 写错（rar）", dict(mode="plain", format="rar"), "zip"),
    ]
    for 名, 额外, 关键词 in 用例:
        输出 = os.path.join(根, f"拒绝_{名.replace(' ', '_')}.bin")
        响应 = 打包(素材, 输出, **额外)
        断言(响应.get("ok") is not True, f"{名}：被拒绝（不该成功）")
        错误 = json.dumps(响应, ensure_ascii=False)
        断言(
            关键词 in 错误,
            f"{名}：错误里点明了原因（含「{关键词}」）→ {错误[:120]}",
        )
        断言(not os.path.exists(输出), f"{名}：没有留下半成品文件")

    print()
    if 失败:
        print(f"❌ {len(失败)} 项失败")
        return 1
    print("✅ 全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
