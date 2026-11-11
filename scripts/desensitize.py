#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 yangsanhai（杨三海解压）
"""发布前端的两道自检：家目录路径脱敏 + 真实敏感词黑名单扫描。

用法：
  python3 scripts/desensitize.py                          # 真改家目录路径（改完用 git diff 复核）
  python3 scripts/desensitize.py --check                  # 只查家目录残留，命中退出码 1
  python3 scripts/desensitize.py --denylist <文件> --check # 查黑名单词，命中退出码 1

为什么要有这个脚本：开发库（~/hermes/yshjy）里到处是 `/home/<作者>` 这样的真实路径
（测试记录 JSON、文档里的命令示例、几个测试 fixture）。发布库是它的**只读副本**，
每次同步后跑一遍这个脚本再提交 —— 手工改一定会漏，而且漏了看不出来。

**教训（2026-11-11）**：第一版只做了路径替换，结果仓库公开后用户发现正文里还留着他真实的
文件名、解压密码和网盘目录号（README / DESIGN / CLI-SPEC / 界面文案 / tests/README 共 122 处）。
路径脱敏**不等于**内容脱敏 —— 所以加了 --denylist：词表放在**库外的私有文件**里
（`~/hermes/.private/yshjy-denywords.txt`，660 权限），脚本本身不含任何敏感词，
推送前必须 `--denylist ... --check` 返回 0。

规则：
  · 家目录一律换成 `/home/user`（中性、长度相近、不含 shell 特殊字符，不破坏引用与断言）。
  · 跳过二进制扩展名（改了会破坏 PE / 图片 / 归档，那 11 处 PE 调试串留着不影响功能）。
  · 黑名单只做检查不自动替换：替换成什么必须人来看，脚本不乱猜。
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


def load_denylist(path):
    words = []
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            words.append(line)
    return words


def scan_denylist(words):
    hits = []
    for rel in tracked_files():
        p = ROOT / rel
        if p.suffix.lower() in BINARY_SUFFIXES:
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for w in words:
            n = text.count(w)
            if n:
                hits.append((rel, w, n))
    return hits


def main(argv):
    check_only = "--check" in argv
    denylist = None
    if "--denylist" in argv:
        i = argv.index("--denylist")
        if i + 1 >= len(argv):
            print("用法：--denylist <词表文件>", file=sys.stderr)
            return 2
        denylist = argv[i + 1]

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
    rc = 0
    if check_only:
        if hits:
            print("发现 %d 处家目录残留（%d 个文件）：" % (total, len(hits)))
            for rel, n in hits[:20]:
                print("  %3d  %s" % (n, rel))
            if len(hits) > 20:
                print("  …还有 %d 个文件" % (len(hits) - 20))
            rc = 1
        else:
            print("干净：跟踪文件里没有 %s" % HOME_PATH)
    else:
        print("改了 %d 个文件 / %d 处 → 替换成 %s" % (len(changed), total, REPLACEMENT))
        for rel, n in changed[:15]:
            print("  %3d  %s" % (n, rel))
        if len(changed) > 15:
            print("  …还有 %d 个文件" % (len(changed) - 15))

    if denylist:
        words = load_denylist(denylist)
        bad = scan_denylist(words)
        if bad:
            print("\n黑名单命中 %d 处（真实文件名 / 密码 / 私有目录必须换成通用示例）：" % len(bad))
            for rel, w, n in bad[:20]:
                # 只显示文件与条数，不把敏感词再打一遍到终端/日志里
                print("  %3d 次  %s  （第 %d 条规则）" % (n, rel, words.index(w) + 1))
            rc = 1
        else:
            print("\n黑名单干净：%d 条规则全部未命中" % len(words))

    if skipped_binary:
        print("跳过二进制（改了会坏，不影响功能）：%d 个" % len(skipped_binary))
        for rel in skipped_binary[:5]:
            print("  %s" % rel)
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
