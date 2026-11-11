#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 yangsanhai（杨三海解压）
"""把发布版里开发机的家目录路径脱敏。

用法：
  python3 scripts/desensitize.py           # 真改（改完用 git diff 复核）
  python3 scripts/desensitize.py --check   # 只检查，有命中就退出码 1（可进 CI / 推送前自检）

为什么要有这个脚本：开发库（~/hermes/yshjy）里到处是 `/home/<作者>` 这样的真实路径
（测试记录 JSON、文档里的命令示例、几个测试 fixture）。发布库是它的**只读副本**，
每次同步后跑一遍这个脚本再提交 —— 手工改一定会漏，而且漏了看不出来。

规则：
  · 一律替换成 `/home/user`（中性、长度相近、不含 shell 特殊字符，不会破坏引用与断言）。
  · 跳过二进制扩展名（改了会破坏 PE / 图片 / 归档，那 11 处 PE 调试串留着不影响功能）。
  · 脚本自己不自指：路径是 `"/home/" + "yangsanhai"` 拼出来的。

退出码：0 = 干净/已改完；1 = --check 发现残留；2 = 用法错。
"""
import pathlib
import subprocess
import sys

HOME_PATH = "/home/" + "yangsanhai"
REPLACEMENT = "/home/user"
BINARY_SUFFIXES = {
    ".exe", ".dll", ".so", ".a", ".rlib", ".bin", ".png", ".jpg", ".jpeg", ".gif", ".ico",
    ".webp", ".ttf", ".ttc", ".otf", ".woff", ".woff2", ".7z", ".rar", ".zip", ".gz", ".xz",
    ".zst", ".bz2", ".pdf", ".ico", ".mp4", ".mp3", ".wav",
}
ROOT = pathlib.Path(__file__).resolve().parent.parent


def tracked_files():
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z"],
        capture_output=True, check=True,
    ).stdout
    return [f for f in out.decode("utf-8").split("\0") if f]


def main(argv):
    check_only = "--check" in argv
    needle = HOME_PATH.encode()
    changed, hits, skipped_binary = [], [], []

    for rel in tracked_files():
        p = ROOT / rel
        if p.suffix.lower() in BINARY_SUFFIXES:
            try:
                if needle in p.read_bytes():
                    skipped_binary.append(rel)
            except OSError:
                pass
            continue
        try:
            data = p.read_bytes()
        except OSError:
            continue
        n = data.count(needle)
        if not n:
            continue
        hits.append((rel, n))
        if not check_only:
            p.write_bytes(data.replace(needle, REPLACEMENT.encode()))
            changed.append((rel, n))

    total = sum(n for _, n in hits)
    if check_only:
        if hits:
            print("发现 %d 处残留（%d 个文件）：" % (total, len(hits)))
            for rel, n in hits[:20]:
                print("  %3d  %s" % (n, rel))
            if len(hits) > 20:
                print("  …还有 %d 个文件" % (len(hits) - 20))
            return 1
        print("干净：跟踪文件里没有 %s" % HOME_PATH)
    else:
        print("改了 %d 个文件 / %d 处 → 替换成 %s" % (len(changed), total, REPLACEMENT))
        for rel, n in changed[:15]:
            print("  %3d  %s" % (n, rel))
        if len(changed) > 15:
            print("  …还有 %d 个文件" % (len(changed) - 15))

    if skipped_binary:
        print("跳过二进制（改了会坏，不影响功能）：%d 个" % len(skipped_binary))
        for rel in skipped_binary[:5]:
            print("  %s" % rel)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
