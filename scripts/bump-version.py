#!/usr/bin/env python3
"""版本号一条命令 —— 把散在全库的版本号一次改齐，并核对有没有漏。

用法：
    python3 scripts/bump-version.py 0.9.48      # 改到 0.9.48
    python3 scripts/bump-version.py --check     # 只核对，不改（提交前 / 发版前跑）
    python3 scripts/bump-version.py --check -q  # 只报结论，不列明细

为什么要这个工具：版本号散在 6 个文件 10 处，手改必漏。实测 2026-11-01 建这个工具时
`ffi/Cargo.lock` 还停在 **0.9.29** —— 比当时版本落后 18 个，一直没人发现，因为那个
子 crate（C FFI）平时根本不编译，谁也不看它的 lock。

真源与生成物分工（改版本只改真源，生成物重跑生成器）：
    真源    Cargo.toml · ffi/Cargo.toml · gui/pubspec.yaml · README.md（徽章 + 正文）·
            docs/PROTOCOL.md 的两处示例回包 · 两个 Cargo.lock 里的本地包条目
    生成物  docs/yshjy.1（man 页）+ scripts/completions/*（三份补全）
            → 改完版本还要 `cargo build --release && python3 scripts/gen-cli-assets.py`
            （本脚本也顺手改 man 页，但下次重跑生成器会按 Cargo.toml 覆盖，结果一致）
    不动    CHANGELOG.md / HANDOFF.md / docs/TODO.md（历史记录，改了就是假账）
            dist/（已发布快照，出包时重新生成）

设计上的两个硬约束（踩过才这么写）：
  1. **原子性**：先把所有文件读进内存、全部替换成功才写盘。任何一处定位失败就一个文件
     都不改，绝不留下「改了一半」的工作区。
  2. **lock 文件按包名锚定，不按旧版本号换**：lock 里的旧值可能落后很多（ffi 那份就是
     0.9.29），拿「当前版本」当 old 去找会一处都匹配不上。改成「name = "x" 后面那行
     version 无条件设成新版本」。

★ 第三条最重要 —— **正则必须锚定到语义前缀，绝不能只按「版本号形状」匹配**：
  建这工具时第一版把 README 正文那条写成 `（v([0-9.]+)）`，结果一次命中了 4 处，
  把三条**历史里程碑**「解压层…（v0.1.0）」一起改成了当前版本 —— 等于篡改版本历史。
  凡是「长得像版本号」的串（历史记录、里程碑、示例）都要靠前缀区分开，
  例如「## ✅ 现在就能用（v…）」而不是单独一个「（v…）」。改规则时先 `grep -c`
  看命中数，不是 1 就说明正则太宽。

退出码：0 全部一致 / 0（bump 成功）/ 1 有不一致或有地方定位失败。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# 版本号的唯一权威：Cargo.toml 的 [package] version（`--version` 输出的就是它）
SOURCE = "Cargo.toml"
SOURCE_RE = r'(?m)^version = "([^"]+)"'

# (文件, 定位正则, 说明, 类别)
# 正则里必须**恰好一个捕获组** = 版本号本身，替换时只动这个组、别的一个字不碰。
ANCHORS: list[tuple[str, str, str, str]] = [
    (SOURCE,             SOURCE_RE,
     "引擎版本（--version / 状态栏 / man 都从这来）", "source"),
    ("ffi/Cargo.toml",   r'(?m)^version = "([^"]+)"',
     "FFI 库版本（与引擎同版本）", "source"),
    ("gui/pubspec.yaml", r'(?m)^version: ([0-9][0-9A-Za-z.+-]*)',
     "界面版本（release.sh 会拒绝与引擎不一致）", "source"),
    ("README.md",        r'状态-v([0-9][0-9A-Za-z.+-]*)%20',
     "README 顶部徽章", "source"),
    ("README.md",        r'## ✅ 现在就能用（v([0-9][0-9A-Za-z.+-]*)）',
     "README「现在就能用」标题", "source"),
    ("docs/PROTOCOL.md", r'"engine": "([^"]+)"',
     "协议文档 hello 示例（带空格写法）", "source"),
    ("docs/PROTOCOL.md", r'"engine":"([^"]+)"',
     "协议文档回包示例（紧凑写法）", "source"),
    ("Cargo.lock",       r'(?m)^name = "yshjy"\nversion = "([^"]+)"',
     "主 Cargo.lock 的本地包", "lock"),
    ("ffi/Cargo.lock",   r'(?m)^name = "yshjy"\nversion = "([^"]+)"',
     "ffi/Cargo.lock 里的引擎", "lock"),
    ("ffi/Cargo.lock",   r'(?m)^name = "yshjy-ffi"\nversion = "([^"]+)"',
     "ffi/Cargo.lock 里的 FFI 库", "lock"),
    ("docs/yshjy.1",     r'"yshjy ([0-9][0-9A-Za-z.+-]*)"',
     "man 页版本行（生成物）", "generated"),
]

# metainfo 的 <releases> 是发版记录、不是当前版本，只提示不判定
METAINFO = "data/io.github.yangsanhai.yshjy.metainfo.xml"

VERSION_RE = re.compile(r'^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$')


def fail(msg: str) -> None:
    print(f"!! {msg}", file=sys.stderr)


def source_version() -> str:
    text = (ROOT / SOURCE).read_text(encoding="utf-8")
    m = re.search(SOURCE_RE, text)
    if not m:
        fail(f"{SOURCE} 里找不到 version 行")
        sys.exit(1)
    return m.group(1)


def current_values() -> list[tuple[str, str, str | None, str]]:
    """读每一处锚点当前的值。返回 (文件, 说明, 值或 None, 类别)。"""
    cache: dict[str, str] = {}
    rows: list[tuple[str, str, str | None, str]] = []
    for path, pattern, desc, kind in ANCHORS:
        if path not in cache:
            f = ROOT / path
            cache[path] = f.read_text(encoding="utf-8") if f.exists() else ""
        text = cache[path]
        if not text:
            rows.append((path, desc, None, kind))
            continue
        m = re.search(pattern, text)
        rows.append((path, desc, m.group(1) if m else None, kind))
    return rows


def check(quiet: bool) -> int:
    want = source_version()
    rows = current_values()
    bad: list[tuple[str, str, str | None]] = []

    if not quiet:
        print(f"权威版本（Cargo.toml）：{want}\n")
        print(f"  {'状态':<6} {'文件':<20} {'现值':<12} 说明")
        print(f"  {'-' * 6} {'-' * 20} {'-' * 12} {'-' * 34}")

    for path, desc, val, kind in rows:
        ok = val == want
        if not ok:
            bad.append((path, desc, val))
        if not quiet:
            mark = "✅" if ok else "❌"
            print(f"  {mark:<5} {path:<20} {val if val else '（未命中）':<12} {desc}")

    if not quiet:
        meta = ROOT / METAINFO
        if meta.exists():
            m = re.search(r'<release version="([^"]+)"', meta.read_text(encoding="utf-8"))
            if m:
                latest = m.group(1)
                note = "（落后，真发版时补一条 <release>）" if latest != want else ""
                print(f"\n  ℹ️  AppStream 发版记录最新：{latest} {note}".rstrip())

    if bad:
        sys.stdout.flush()
        print(f"\n❌ {len(bad)} 处不同步：", file=sys.stderr)
        for path, desc, val in bad:
            print(f"   {path}：{desc} = {val or '（未命中）'}（应为 {want}）", file=sys.stderr)
        return 1

    print(f"\n✅ 全部 {len(rows)} 处一致（{want}）")
    return 0


def bump(new: str) -> int:
    if not VERSION_RE.match(new):
        fail(f"版本号格式不对：{new}（形如 0.9.48 或 0.9.48-c）")
        return 1

    old = source_version()
    if old == new:
        print(f"ℹ️  已经是 {new}，什么都不用改")
        return 0

    # 先按文件归组：同一个文件里的多处锚点要顺序替换
    by_file: dict[str, list[tuple[str, str]]] = {}
    for path, pattern, desc, _kind in ANCHORS:
        by_file.setdefault(path, []).append((pattern, desc))

    # ★ 全部在内存里改，任何一处失败就一个文件都不写
    pending: dict[str, str] = {}
    problems: list[str] = []
    report: list[str] = []

    for path, items in by_file.items():
        f = ROOT / path
        if not f.exists():
            problems.append(f"{path}：文件不存在")
            continue
        text = original = f.read_text(encoding="utf-8")
        for pattern, desc in items:
            seen: list[str] = []

            def repl(m: re.Match[str], seen: list[str] = seen) -> str:
                seen.append(m.group(1))
                return m.group(0)[: m.start(1) - m.start(0)] + new + m.group(0)[m.end(1) - m.start(0):]

            text = re.sub(pattern, repl, text)
            if not seen:
                problems.append(f"{path}：{desc} —— 正则一处都没匹配上（文件被大改过？）")
            else:
                note = f"（{len(seen)} 处）" if len(seen) > 1 else ""
                if seen[0] not in (old, new):
                    note += f" ⚠️ 原值 {seen[0]} 与引擎版本不一致（lock 常这样）"
                report.append(f"  {path}  {desc}：{seen[0]} → {new}{note}")
        if text != original:
            pending[path] = text

    if problems:
        fail("有地方定位失败，**一个文件都没改**（不会留下改了一半的工作区）：")
        for p in problems:
            print(f"   {p}", file=sys.stderr)
        return 1

    for path, text in pending.items():
        (ROOT / path).write_text(text, encoding="utf-8")

    print(f"==> {old} → {new}（{len(report)} 处 / {len(pending)} 个文件）")
    for line in report:
        print(line)

    print("\n还要手动跟的两步（生成物，故意不放进本脚本）：")
    print("  1) man 页 + 三份 shell 补全：cargo build --release && python3 scripts/gen-cli-assets.py")
    print(f"  2) AppStream 发版记录：真发版时在 {METAINFO} 的 <releases> 顶端补一条")
    return 0


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if a not in ("-q", "--quiet")]
    quiet = len(args) != len(argv[1:])

    if not args or args[0] in ("-h", "--help"):
        print((__doc__ or "").strip())
        return 0 if args else 1

    if args[0] == "--check":
        return check(quiet)

    if len(args) > 1:
        fail("参数太多：一次只改一个版本号")
        return 1
    return bump(args[0])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
