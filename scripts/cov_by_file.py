#!/usr/bin/env python3
"""把 `cargo llvm-cov --text` 的逐行明细按文件汇总成覆盖率表（升序：最裸的模块在最上面）。

为什么需要这个脚本:
  `cargo llvm-cov --text` 只输出每个源文件的逐行命中明细（几万行），**没有末尾的文件汇总表**；
  `--summary-only` 又只给一个总计。要回答「哪些模块几乎是裸的」，只能自己解析逐行明细。

用法:
  cargo llvm-cov --text --no-fail-fast > /tmp/cov.txt
  python3 scripts/cov_by_file.py /tmp/cov.txt                  # 全量
  python3 scripts/cov_by_file.py /tmp/cov.txt --min-lines 50   # 只看可执行行数 ≥ 50 的文件（滤掉小文件噪音）

读法要点（写进报告的）:
  - 入口 main.rs 常是 0%，多数是**统计口径**问题: tests/*.sh、协议探针跑的是编好的二进制，
    llvm-cov 只统计被插桩的 lib 测试进程，外部脚本的覆盖不计入 —— 别据此报「CLI 没测」。
  - 真正的缺口通常在协议层 / FFI 层（shell 与引擎之间的通道），那里低才是用户会出事的地方。
  - 本项目实测（2026-11-01，0.9.47）：详见 docs/AUDIT-PHASE0.md 的 T15 节。
"""
import argparse
import os
import re
import sys

# 文件头: /abs/path/src/foo.rs:
RE_FILE = re.compile(r"^(/\S+\.rs):$")
# 明细行: 行号|执行次数|源码     执行次数为空 = 不可执行行；可能是 "12" 或 "1.23k"
RE_LINE = re.compile(r"^\s*(\d+)\|([^|]*)\|")


def count(value: str) -> float:
    v = value.strip()
    if v.endswith("k"):
        return float(v[:-1]) * 1000
    try:
        return float(v)
    except ValueError:
        return 1.0  # 非空但解析不了，按「执行过」算


def parse(path: str) -> dict:
    stats: dict[str, list[int]] = {}
    cur = None
    with open(path, encoding="utf-8", errors="replace") as fh:
        for raw in fh:
            line = raw.rstrip("\n")
            m = RE_FILE.match(line)
            if m:
                cur = m.group(1)
                stats[cur] = [0, 0]  # [可执行行, 命中行]
                continue
            if cur is None:
                continue
            m2 = RE_LINE.match(line)
            if not m2:
                continue
            if m2.group(2).strip():
                stats[cur][0] += 1
                if count(m2.group(2)) > 0:
                    stats[cur][1] += 1
    return stats


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("covfile", help="`cargo llvm-cov --text` 重定向出来的文件")
    ap.add_argument("--min-lines", type=int, default=0, help="只显示可执行行数不低于该值的文件")
    args = ap.parse_args()

    if not os.path.exists(args.covfile):
        print(f"找不到 {args.covfile}", file=sys.stderr)
        return 1

    stats = parse(args.covfile)
    rows = []
    for f, (total, hit) in stats.items():
        if total == 0 or total < args.min_lines:
            continue
        rows.append((hit / total * 100, total, hit, f))
    if not rows:
        print("没解析到任何源文件 —— 确认导出的确实是 --text 格式（行首形如 `   12|       |code`）", file=sys.stderr)
        return 1

    rows.sort()  # 覆盖率升序 = 最裸的在前
    all_lines = sum(r[1] for r in rows)
    all_hit = sum(r[2] for r in rows)
    print(f"总计：{all_hit}/{all_lines} 行 = {all_hit / all_lines * 100:.1f}%（{len(rows)} 个文件）\n")
    print(f"{'覆盖率':>7}  {'可执行':>7} {'命中':>7}  文件")
    print("-" * 70)
    for pct, total, hit, f in rows:
        flag = "  ← 几乎是裸的" if pct < 30 else ("  ← 偏低" if pct < 60 else "")
        print(f"{pct:6.1f}%  {total:7d} {hit:7d}  {f}{flag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
