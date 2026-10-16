#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""extreme_video_check.py —— E2 专项：ZIP 插在视频中间后，视频还能播吗？
   （1）原视频            assets/cover/ev10.mp4
   （2）E2 样本（ZIP 还在视频中间）
   （3）把两段视频拼回（去掉中间 ZIP）后
"""
import hashlib
import json
import os
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
ASSETS = os.path.join(HERE, "assets")
WORK = os.path.join(HERE, ".work", "extreme", "e2")
os.makedirs(WORK, exist_ok=True)

FFMPEG = "/usr/bin/ffmpeg"
MANIFEST = os.path.join(HERE, "extreme_manifest.json")


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def ffmpeg_rc(p):
    r = subprocess.run([FFMPEG, "-v", "error", "-i", p, "-f", "null", "-"],
                       capture_output=True, text=True, timeout=180)
    return r.returncode, (r.stderr or "").strip().splitlines()[:2]


def main():
    with open(MANIFEST, encoding="utf-8") as f:
        man = json.load(f)
    s = man["samples"]["E2_video_split_zip_middle.mp4"]
    mid = s["video_split"]
    vlen = s["video_len"]
    zlen = man["zip1_len"]
    sample = os.path.join(DATA, "E2_video_split_zip_middle.mp4")
    orig = os.path.join(ASSETS, "cover", "ev10.mp4")

    data = open(sample, "rb").read()
    head = data[:mid]
    tail = data[mid + zlen:]
    rejoin = os.path.join(WORK, "rejoin.mp4")
    with open(rejoin, "wb") as f:
        f.write(head + tail)

    out = {}
    for tag, p in [("原视频 ev10.mp4", orig), ("E2 样本(含 ZIP)", sample), ("拼回(去掉 ZIP)", rejoin)]:
        rc, err = ffmpeg_rc(p)
        out[tag] = {"path": p, "size": os.path.getsize(p), "ffmpeg_rc": rc, "ffmpeg_err": err,
                    "sha256": sha(p)}
        print(f"{tag:20s} size={os.path.getsize(p):>7} ffmpeg_rc={rc:<4} sha256={out[tag]['sha256'][:16]} {err}")

    same = out["原视频 ev10.mp4"]["sha256"] == out["拼回(去掉 ZIP)"]["sha256"]
    print(f"\n拼回 === 原视频 ? {same}")
    out["rejoin_equals_original"] = same

    # 顺便：把 E2 的视频头尾分别也能 ffmpeg 吗（残片）
    with open(os.path.join(WORK, "head_only.mp4"), "wb") as f:
        f.write(head)
    with open(os.path.join(WORK, "tail_only.mp4"), "wb") as f:
        f.write(tail)
    for tag, p in [("仅视频前半", os.path.join(WORK, "head_only.mp4")),
                   ("仅视频后半", os.path.join(WORK, "tail_only.mp4"))]:
        rc, err = ffmpeg_rc(p)
        out[tag] = {"ffmpeg_rc": rc, "ffmpeg_err": err, "size": os.path.getsize(p)}
        print(f"{tag:20s} size={os.path.getsize(p):>7} ffmpeg_rc={rc:<4} {err}")

    with open(os.path.join(HERE, "extreme_video_results.json"), "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=2)
    print("\n→ tests/extreme_video_results.json")


if __name__ == "__main__":
    main()
