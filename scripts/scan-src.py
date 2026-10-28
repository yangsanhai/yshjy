#!/usr/bin/env python3
"""扫描 src/ 生成模块地图 —— `docs/INTERNALS.md` 的数字来源。

**为什么要有这个脚本**：项目已经 27k 行 / 49 个文件，模块职责与依赖关系靠人脑记不住，
手写在文档里的数字又必然漂（man 页版本号、补全 18/35 项都栽在「手抄副本」上）。
所以：「内部全景」里的行数、依赖次数、事件数、错误码数**一律由脚本产出**。

用法：
    python3 scripts/scan-src.py            # 打印模块地图 + 依赖矩阵 + 核对项
    python3 scripts/scan-src.py --check    # 只打印关键计数（给文档核对用）

改完代码后跑一次，把 `docs/INTERNALS.md` §3 / §5 的数字对上（**改文档，别改脚本**）。
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]  # 项目根
SRC = ROOT / "src"

MOD = re.compile(r"^//!\s?(.*)$")
USE = re.compile(r"^use crate::(\w+)")

# 分层假设（只用于发现「底层调高层」的反向依赖，不是铁律）
LAYER = {
    "io": 0, "error": 0, "ctx": 0, "paths": 0, "event": 0, "safety": 0, "i18n": 0,
    "config": 0, "cli_spec": 0, "progress": 0, "hardware": 0,
    "scan": 1, "unpack": 1, "extract": 1, "extract7z": 1, "extract_rar": 1,
    "extract_stream": 1, "layout": 1, "pack": 1, "pack_post": 1, "rescue": 1,
    "repair": 1, "restore": 1, "resume": 1, "preview": 1, "probe": 1,
    "cover_extract": 1, "drill": 1, "select": 1, "password_hint": 1,
    "report": 1, "trash": 1, "tools": 1,
    "batch": 2, "job": 2, "api": 2, "ask": 2, "serve": 2,
    "main": 3,
}


def scan() -> list[tuple]:
    rows = []
    for p in sorted(SRC.rglob("*.rs")):
        rel = p.relative_to(SRC).as_posix()
        text = p.read_text(encoding="utf-8", errors="replace")
        lines = text.splitlines()
        doc: list[str] = []
        for ln in lines[:60]:
            m = MOD.match(ln)
            if m:
                t = m.group(1).strip()
                if t and not t.startswith("#"):
                    doc.append(t)
                if len(doc) >= 2:
                    break
            elif doc:
                break
        deps = sorted({m.group(1) for ln in lines if (m := USE.match(ln))})
        pub_fn = len(re.findall(r"pub (?:unsafe )?fn ", text))
        pub_ty = len(re.findall(r"pub struct |pub enum |pub trait ", text))
        rows.append((rel, len(lines), doc, deps, pub_fn, pub_ty, text))
    return rows


def main() -> int:
    rows = scan()
    if "--check" in sys.argv:
        total = sum(r[1] for r in rows)
        ev = len({m.group(1) for m in re.finditer(r'"([A-Z][A-Z_]{3,})"',
                                                  (SRC / "event.rs").read_text(encoding="utf-8"))})
        err_src = (SRC / "error.rs").read_text(encoding="utf-8")
        block = err_src.split("pub enum ErrorCode", 1)[-1].split("}", 1)[0]
        codes = len(re.findall(r"^\s{4}[A-Z][A-Za-z]+", block, re.M))
        impls = len(re.findall(r"impl InArchive for",
                               (SRC / "unpack.rs").read_text(encoding="utf-8")))
        api = (SRC / "api.rs").read_text(encoding="utf-8")
        seg = re.search(r'"hello"(.{0,300}?)"answer"', api, re.S)
        cmds = len(re.findall(r'"\w+"', seg.group(0))) if seg else 0
        gui = list((ROOT / "gui" / "lib").rglob("*.dart"))
        gui_lines = sum(len(f.read_text(encoding="utf-8", errors="replace").splitlines()) for f in gui)
        print(f"src 行数        {total}")
        print(f"src 文件数      {len(rows)}")
        print(f"事件数          {ev}")
        print(f"错误码数        {codes}")
        print(f"InArchive impl  {impls}（含 1 个测试假解压器）")
        print(f"协议命令数      {cmds}")
        print(f"gui/lib         {gui_lines} 行 / {len(gui)} 文件")
        return 0

    print("=== 模块地图（行数 | pub项 | 内部依赖 | 职责）===")
    for rel, n, doc, deps, pf, pt, _ in rows:
        d = f"  // {doc[0][:56]}" if doc else ""
        print(f"{n:>5}  {rel:<24} pub:{pf:>3}fn/{pt:>3}ty  ← {','.join(deps) if deps else '-'}{d}")

    cnt = Counter()
    for _rel, _n, _doc, deps, *_ in rows:
        cnt.update(deps)
    print("\n=== 被依赖次数 top 20（越靠前越底层）===")
    for name, c in cnt.most_common(20):
        print(f"  {c:>3}  {name}")

    print("\n=== 反向依赖（底层用了高层，逐条看过再决定要不要拆）===")
    for rel, _n, _doc, deps, *_ in rows:
        me = rel.replace(".rs", "").split("/")[0]
        my = LAYER.get(me)
        if my is None:
            continue
        for d in deps:
            dl = LAYER.get(d)
            if dl is not None and dl > my:
                print(f"  {rel} (L{my}) → {d} (L{dl})")

    total = sum(r[1] for r in rows)
    print(f"\n=== 合计 {total} 行 / {len(rows)} 文件 ===")
    print("    核对项用 `--check` 打印（event/error cmds 等）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
