#!/usr/bin/env python3
"""给核心源码加 MIT 署名头（SPDX + 版权行）。

用法：
    python3 scripts/add-spdx.py            # 预演：只报哪些文件缺头
    python3 scripts/add-spdx.py --apply    # 真写

为什么要有这个：MIT 许可证第 1 条要求「在软件的所有副本或实质部分中包含
上述版权声明和许可声明」。文件头是最普遍、最容易被保留的一份；删掉它就是
违反许可证，也是我们日后举证时最先看的地方。

配套的另一半在 `src/signature.rs`：那里把作者写死成常量 + 一个 `#[used]`
静态串，保证任何人用这份源码重编出来的二进制里都能 `strings | grep yangsanhai` 搜到。

范围：核心引擎（src/、ffi/src/、build.rs）与界面（gui/lib/）。
不碰：tests/（测试夹具里常要构造"没有头"的样本）、scripts/、refs/。
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

HEAD_RUST = (
    "// SPDX-License-Identifier: MIT\n"
    "// Copyright (c) 2026 yangsanhai（杨三海解压）\n"
)
HEAD_DART = HEAD_RUST
MARK = "SPDX-License-Identifier"

TARGETS = [
    (ROOT / "src", "*.rs"),
    (ROOT / "ffi" / "src", "*.rs"),
    (ROOT / "gui" / "lib", "*.dart"),
]


def files():
    for base, pattern in TARGETS:
        if base.is_dir():
            yield from sorted(base.rglob(pattern))
    build_rs = ROOT / "build.rs"
    if build_rs.is_file():
        yield build_rs


def main() -> int:
    apply = "--apply" in sys.argv
    missing, already = [], 0
    for p in files():
        text = p.read_text(encoding="utf-8")
        first = text.splitlines()[0] if text.splitlines() else ""
        if MARK in first or MARK in text[:400]:
            already += 1
            continue
        missing.append(p)

    print(f"扫描完成：已有署名头 {already} 个，缺 {len(missing)} 个")
    for p in missing:
        print("   缺:", p.relative_to(ROOT))

    if not apply:
        print("\n（预演模式，什么都没写。加 --apply 真写）")
        return 1 if missing else 0

    head = HEAD_RUST
    wrote = 0
    for p in missing:
        text = p.read_text(encoding="utf-8")
        # 保留原有换行风格：CRLF 文件补 CRLF 头
        if "\r\n" in text[:200]:
            h = head.replace("\n", "\r\n")
        else:
            h = head
        p.write_text(h + text, encoding="utf-8", newline="")
        wrote += 1
    print(f"\n已写入 {wrote} 个文件")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
