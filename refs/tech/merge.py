#!/usr/bin/env python3
"""把 refs/tech/*.md 的拆解清单合并成总台账。

用法: python3 merge.py            # 摘要统计
      python3 merge.py --full     # 输出全部条目
"""
import os, re, sys, glob

HERE = os.path.dirname(os.path.abspath(__file__))
DECISIONS = ["✅", "🔄", "⚠️", "❓"]

def parse(path):
    """从 markdown 里抽出表格条目。"""
    rows, section = [], ""
    for line in open(path, encoding="utf-8"):
        line = line.rstrip()
        m = re.match(r"^#{2,4}\s*(?:环节\s*[：:]?\s*)?(.+)$", line)
        if m and ("环节" in line or "读取" in line or "判断" in line
                  or "解压" in line or "压缩" in line or "修补" in line):
            section = m.group(1).strip()
        if line.startswith("|") and line.count("|") >= 4:
            cells = [c.strip() for c in line.strip("|").split("|")]
            if len(cells) < 5:
                continue
            if set(cells[0]) <= set("-: ") or "技术点" in line:
                continue
            rows.append({"src": os.path.basename(path), "section": section, "cells": cells})
    return rows

def main():
    files = sorted(glob.glob(os.path.join(HERE, "*.md")))
    files = [f for f in files if not f.endswith("README.md")]
    if not files:
        print("还没有拆解结果（等子任务产出）")
        return

    all_rows = []
    for f in files:
        rs = parse(f)
        all_rows += rs
        print(f"  {os.path.basename(f):<38} {len(rs):>4} 条")

    print(f"\n  合计 {len(all_rows)} 条技术点\n")

    # 按决定统计
    counts = {d: 0 for d in DECISIONS}
    other = 0
    for r in all_rows:
        joined = " ".join(r["cells"])
        hit = False
        for d in DECISIONS:
            if d in joined:
                counts[d] += 1
                hit = True
                break
        if not hit:
            other += 1

    label = {"✅": "已采用", "🔄": "待采用", "⚠️": "已评估-不采用", "❓": "未判定"}
    print("  按决定分布：")
    for d in DECISIONS:
        print(f"    {d} {label[d]:<14} {counts[d]:>4} 条")
    if other:
        print(f"    （未标注决定）      {other:>4} 条")

    if "--full" in sys.argv:
        print("\n\n══════ 全部条目 ══════")
        for r in all_rows:
            print(f"[{r['src']}] {r['section']}")
            print("   " + " | ".join(r["cells"]))
    else:
        print("\n  提示：加 --full 可输出全部条目")

    # 输出等待被采用的清单
    todo = [r for r in all_rows if "🔄" in " ".join(r["cells"])]
    if todo:
        print(f"\n══════ 待采用清单（{len(todo)} 条，按此排期）══════")
        for r in todo:
            c = r["cells"]
            name = c[1] if len(c) > 1 else "?"
            print(f"  · [{r['section']}] {name}   ← {r['src']}")

if __name__ == "__main__":
    main()
