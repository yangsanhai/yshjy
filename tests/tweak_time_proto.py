#!/usr/bin/env python3
"""探针：`--tweak-time` / 协议 `pack.tweak_time`（防炸链）—— T18-3。

网盘按内容哈希判重：同一份东西反复上传会秒传，甚至「炸链」（分享被判定重复）。
把条目时间戳打乱，整包字节就变、哈希也变，网盘当成新文件。

这里盯四件事，都是「改坏了照样能过 cargo test」的那种：

  ① **默认关**：不给参数时，同一份输入打两次必须**逐字节一致**
     （这条是项目既有约定，decoy 那边也在守 —— 别被新开关带坏）；
  ② **开了就每次不同**：zip 与 tar.gz 都要变（两个写入器都得改，只改一个就是半成品）；
  ③ **时间戳得像个正常文件**：落在 [2020, 现在+1天]，**绝不能是 1970 / 1980**
     （用户明确讨厌那种假时间）；
  ④ **协议层也通**：`pack.tweak_time: true` 与 CLI `--tweak-time` 是同一件事。

用法：python3 tests/tweak_time_proto.py <引擎路径>
"""
import hashlib
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile

引擎 = sys.argv[1] if len(sys.argv) > 1 else "yshjy"
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


def 命令行(args):
    return subprocess.run(args, capture_output=True, text=True)


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    根 = tempfile.mkdtemp(prefix="yshjy_tweak_")
    素材 = os.path.join(根, "素材")
    os.makedirs(素材)
    for i, name in enumerate(["甲.txt", "乙.txt", "丙.txt"]):
        with open(os.path.join(素材, name), "w", encoding="utf-8") as f:
            f.write(f"内容 {i}\n" * 20)

    def 打(格式, 次数, tweak=False):
        出 = []
        for i in range(次数):
            out = os.path.join(根, f"{格式}{'-tweak' if tweak else ''}-{i}.{格式}")
            args = [引擎, "ys", 素材, "--format", 格式, "-o", out, "--quiet"]
            if tweak:
                args.insert(3, "--tweak-time")
            r = 命令行(args)
            assert r.returncode == 0, f"打包失败：{r.stderr}"
            出.append(out)
        return 出

    print("── ① 默认关：两次打包必须逐字节一致 ──")
    for 格式 in ("zip", "tar.gz"):
        a, b = 打(格式, 2)
        断言(sha(a) == sha(b), f"{格式}：不开 tweak 时两次哈希一致")

    print("── ② 开了就每次不同（zip / tar.gz 两个写入器都要改） ──")
    for 格式 in ("zip", "tar.gz"):
        a, b = 打(格式, 2, tweak=True)
        断言(sha(a) != sha(b), f"{格式}：开了 tweak 两次哈希不同（防炸链的核心）")

    print("── ③ 时间戳得像正常文件（绝不能 1970 / 1980） ──")
    现在 = time.localtime().tm_year
    z = 打("zip", 1, tweak=True)[0]
    with zipfile.ZipFile(z) as zf:
        年们 = [info.date_time[0] for info in zf.infolist()]
    断言(年们 and all(1981 < y <= 现在 + 1 for y in 年们), f"zip 条目年份合理：{年们}")
    t = 打("tar.gz", 1, tweak=True)[0]
    with tarfile.open(t, "r:gz") as tf:
        t年 = [m.mtime for m in tf.getmembers() if m.isfile()]
    断言(
        t年 and all(946684800 < m < time.time() + 86400 for m in t年),
        f"tar 条目 mtime 在 2000 年之后、不是未来：{[time.strftime('%Y', time.localtime(m)) for m in t年]}",
    )

    print("── ④ 协议层也要通（pack.tweak_time） ──")
    甲 = os.path.join(根, "proto-0.zip")
    乙 = os.path.join(根, "proto-1.zip")
    for out in (甲, 乙):
        r = 跑一条({"id": 1, "cmd": "pack", "inputs": [素材], "out": out,
                   "mode": "plain", "format": "zip", "tweak_time": True})
        断言(r.get("ok") is True, f"协议 tweak_time 打包成功（{r.get('error')}）")
    断言(os.path.exists(甲) and os.path.exists(乙) and sha(甲) != sha(乙),
         "协议下两次产物也不同（与 CLI 同一套行为）")

    print()
    if 失败:
        print(f"❌ {len(失败)} 项失败")
        return 1
    print("✅ 全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
