#!/usr/bin/env python3
"""探针：协议 `pack` 的 `tail`（不改封面，接在文件末尾）—— T7 / E2。

背景：`pack.tail` 引擎里一直有，界面却只**显示**结果、从不传。E2 把它接成
界面开关之后，这条协议行为仍**没有任何探针守着** —— widget 测试只验「界面传了
什么」，引擎真怎么落盘没人管，改坏了 `flutter test` 照样绿。

不看日志、只看产物字节（「内部选择没被回显时，用产物的字节位置证明它真变了」）：
  ① 不传 tail（默认）→ 自动模式：ZIP 头在封面**之后**（中间插了 free 盒）
  ② tail=true       → 尾部模式：ZIP 头**紧贴封面末尾**（偏移 == 封面大小）
  ③ tail=false 显式 → 与 ① 同一个语义（显式 false 不能变成第三套行为）
正面对照组：三份产物都要能把原文件解出来、内容一致 —— 否则「为了紧贴而写坏
布局」也会全绿。

⚠️ 偏移的**绝对值**每次都不一样（ZIP 头大小随文件名/时间戳变），所以只断言
「等于封面大小」还是「大于封面大小」，别写死 1789 这种数字。

用法：python3 tests/tail_proto.py <引擎路径>
（取二进制的顺序：验新代码请显式传 `target/debug/yshjy`；
  `target/release/yshjy` 往往是上次发版留下的旧版 —— 那样验的是旧代码。）
"""
import io
import json
import os
import subprocess
import sys
import tempfile
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
    """起一个真 --serve，发一条命令，读到同 id 的响应为止"""
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
        p.wait(timeout=15)


def 打包(素材, 输出, tail):
    命令 = {"id": 1, "cmd": "pack", "inputs": [素材], "out": 输出, "cover": 封面}
    if tail is not None:
        命令["tail"] = tail
    return 跑一条(命令)


def 看产物(路径):
    """返回（首个 PK 偏移, 解出来的内容字典, 解压报错）"""
    with open(路径, "rb") as f:
        字节 = f.read()
    偏移 = 字节.find(b"PK\x03\x04")
    if 偏移 < 0:
        return -1, {}, None
    try:
        with zipfile.ZipFile(io.BytesIO(字节[偏移:])) as z:
            return 偏移, {n: z.read(n) for n in z.namelist()}, None
    except Exception as e:  # noqa: BLE001 —— 探针要把真实报错打出来
        return 偏移, {}, str(e)


def main():
    if not os.path.exists(封面):
        print(f"⏭ 跳过：找不到内置封面 {封面}")
        return 0
    封面大小 = os.path.getsize(封面)

    根 = tempfile.mkdtemp(prefix="yshjy_tail_proto_")
    素材 = os.path.join(根, "素材")
    os.makedirs(素材)
    with open(os.path.join(素材, "甲.txt"), "w", encoding="utf-8") as f:
        f.write("hello 杨三海\n" * 50)
    with open(os.path.join(素材, "乙.txt"), "w", encoding="utf-8") as f:
        f.write("world\n")

    结果 = {}
    for 名, tail in (("默认", None), ("尾部模式", True), ("显式关掉", False)):
        输出 = os.path.join(根, f"{名}.mp4")
        响应 = 打包(素材, 输出, tail)
        断言(响应.get("ok") is True, f"{名}：打包成功（{响应.get('error')}）")
        if not os.path.exists(输出):
            断言(False, f"{名}：产物存在")
            continue
        偏移, 内容, 解压错 = 看产物(输出)
        断言(解压错 is None, f"{名}：产物里的 ZIP 段能解开（{解压错}）")
        断言(
            内容.get("素材/甲.txt") == ("hello 杨三海\n" * 50).encode(),
            f"{名}：甲.txt 内容一致",
        )
        断言(内容.get("素材/乙.txt") == b"world\n", f"{名}：乙.txt 内容一致")
        结果[名] = 偏移

    if len(结果) == 3:
        断言(
            结果["尾部模式"] == 封面大小,
            f"尾部模式：ZIP 头紧贴封面末尾（{结果['尾部模式']} == 封面 {封面大小}）",
        )
        断言(
            结果["默认"] > 封面大小,
            f"默认：ZIP 头在封面之后、留了 free 盒（{结果['默认']} > {封面大小}）",
        )
        断言(
            结果["显式关掉"] > 封面大小,
            f"显式 false 与默认同语义（{结果['显式关掉']} > {封面大小}）",
        )

    print()
    if 失败:
        print(f"❌ {len(失败)} 项失败")
        return 1
    print("✅ 全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
