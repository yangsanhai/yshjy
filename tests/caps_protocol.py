#!/usr/bin/env python3
"""`caps` 按格式声明：协议 `hello.formats` 与 CLI `--caps --json` 必须是同一份真源。

由来（2026-10-29）：能力表原来只活在 CLI 里（`main.rs` 的硬编码组合清单），
而协议 `hello.caps` 是**全局一份**、跟格式无关 —— 前端想知道「这个格式能不能选条目」
只能 spawn 一个子进程跑 `--caps`，而 iOS 根本不允许 spawn（FFI 端物理上跑不了）。
顺手治了「手抄第二份真源」这个老病（本项目已在 man 页版本号 / bash 补全 / FFI 头文件上栽过三次）。

断言：
  1. `hello.formats` 条数与 CLI `--caps --json` 条数一致，且逐字段完全相同（同源，不是两份账）
  2. 能力**真的是按格式**的：zip 能选条目、gzip 这类裸流不能（若哪天退化成全局一份，这条会红）
  3. 7z 的 `encrypted_names` 为真（头加密能力）、zip 为假（zip 没有头加密）
  4. `format` / `shape` 是机器标识（小写、无空格、无中文），展示名不许由引擎产出
  5. 老前端兼容：不认识 `formats` 也必须有全局 `caps`（只加不删）

用法：python3 tests/caps_protocol.py [二进制路径]
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "target" / "release" / "yshjy")
if not Path(BIN).exists():
    BIN = str(ROOT / "target" / "debug" / "yshjy")
if not Path(BIN).exists():
    print("❌ 找不到 yshjy 二进制（先 cargo build --release）")
    sys.exit(1)

n_pass = n_fail = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✓ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ✗ {msg}")


CAPS_KEYS = ["encrypted", "encrypted_names", "split_volumes", "selective", "restores_mtime", "can_create"]


def key(row):
    return (row["format"], row["shape"])


# ── 取协议侧（--serve 一行一个 JSON）──────────────────────────────
hello = None
try:
    p = subprocess.run(
        [BIN, "--serve"],
        input=json.dumps({"id": 1, "cmd": "hello"}) + "\n",
        capture_output=True,
        text=True,
        timeout=20,
    )
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            d = json.loads(line)
        except Exception:
            continue
        if d.get("id") == 1 and d.get("ok"):
            hello = d["result"]
            break
except subprocess.TimeoutExpired:
    print("❌ --serve 超时")
    sys.exit(1)

if not hello:
    print("❌ 没从 --serve 拿到 hello 响应")
    sys.exit(1)

proto_rows = hello.get("formats")
cli_json = subprocess.run([BIN, "--caps", "--json"], capture_output=True, text=True, timeout=30)
cli_rows = json.loads(cli_json.stdout)

print("── 1. 协议 formats 与 CLI --caps 同源 ──")
if not proto_rows:
    bad("hello 里没有 formats（协议没加？）")
else:
    proto_map = {key(r): r for r in proto_rows}
    cli_map = {key(r): r for r in cli_rows}
    if set(proto_map) != set(cli_map):
        bad(f"两边格式集合不一致：只在协议={sorted(set(proto_map) - set(cli_map))} 只在 CLI={sorted(set(cli_map) - set(proto_map))}")
    else:
        diff = [
            k
            for k in proto_map
            if any(proto_map[k].get(f) != cli_map[k].get(f) for f in CAPS_KEYS)
        ]
        if diff:
            bad(f"两边能力位不一致：{diff[:4]}")
        else:
            ok(f"{len(proto_map)} 条格式能力逐字段一致（协议 / CLI 同一份真源）")

print("── 2. 能力是按格式而不是全局一份 ──")
try:
    zip_row = next(r for r in proto_rows if r["format"] == "zip" and r["shape"] == "plain")
    gz_row = next(r for r in proto_rows if r["format"] == "gzip")
    if zip_row["selective"] is True and gz_row["selective"] is False:
        ok("zip 能选条目、gzip（裸流）不能 —— 能力确实挂在格式上")
    else:
        bad(f"退化迹象：zip.selective={zip_row['selective']} gzip.selective={gz_row['selective']}")
except StopIteration:
    bad("找不到 zip/plain 或 gzip 那一行")

print("── 3. 7z 头加密能力 vs zip ──")
try:
    sz = next(r for r in proto_rows if r["format"] == "7z")
    zp = next(r for r in proto_rows if r["format"] == "zip" and r["shape"] == "plain")
    if sz["encrypted_names"] is True and zp["encrypted_names"] is False:
        ok("7z 声明支持头加密、zip 不声明（如实反映实现）")
    else:
        bad(f"7z.encrypted_names={sz['encrypted_names']} zip.encrypted_names={zp['encrypted_names']}")
except StopIteration:
    bad("找不到 7z 或 zip 行")

print("── 4. format / shape 是机器标识（不许引擎产出展示名）──")
bad_ids = [
    r for r in proto_rows
    if not isinstance(r.get("format"), str)
    or not isinstance(r.get("shape"), str)
    or any(ch.isupper() or ch.isspace() or ord(ch) > 127 for ch in r["format"] + r["shape"])
]
if bad_ids:
    bad(f"这些不是机器标识：{[ (r.get('format'), r.get('shape')) for r in bad_ids[:3] ]}")
else:
    ok("全部小写 ASCII、无空格（展示名归前端）")

print("── 5. 老前端兼容：全局 caps 还在 ──")
caps = hello.get("caps") or {}
need = {"preview", "selective", "resume", "resume_level", "background"}
if need.issubset(caps):
    ok(f"全局 caps 五项都在（formats 是新增字段，不认识就忽略）")
else:
    bad(f"全局 caps 缺项：{sorted(need - set(caps))}")

print()
print(f"── 结果：通过 {n_pass} · 失败 {n_fail} ──")
sys.exit(1 if n_fail else 0)
