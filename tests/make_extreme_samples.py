#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""make_extreme_samples.py —— 生成「变态场景」样本 E1~E7。

设计原则（用户资源红线）：
  * 单个样本 <= 30 MB，样本总量 <= 200 MB（本批总计约 10.4 MB）。
  * 视频一律 160x120 / 10s 以内，mpeg4 -q:v 25 → 单文件 ~30KB。
  * 全部可复现：随机数用固定种子，mtime 钉死。

样本清单：
  E1  前后都有超长垃圾        [垃圾 5MB][ZIP][垃圾 5MB]
  E2  视频被压缩包插在中间     [视频前半][ZIP][视频后半]
  E3  压缩包被视频插在中间     [ZIP前半][视频][ZIP后半]         ← 关键
  E4  压缩包中间段被替换       [ZIP前半][视频(覆盖原中段)][ZIP后半] ← 模拟真丢数据
  E5  压缩包头尾各被切掉一块   [ZIP 去掉头部一段 + 尾部一段]
  E6  一个文件藏两个不同压缩包 [ZIP1][视频][ZIP2]
  E7  多个穿插（可选）         [视频][ZIP1][视频][ZIP2]

产出：
  tests/data/E*.{bin,mp4,zip}
  tests/extreme_manifest.json   结构 + 期望成员（供测试脚本算恢复率）
"""
import hashlib
import json
import os
import random
import shutil
import struct
import subprocess
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
DATA = os.path.join(HERE, "data")
ASSETS = os.path.join(HERE, "assets")
WORK = os.path.join(HERE, ".work", "extreme")
EXT_ASSETS = os.path.join(ASSETS, "extreme")

FFMPEG = "/usr/bin/ffmpeg"
FIXED_MTIME = 1750000000


def sh(cmd, timeout=180, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd)
    return r.returncode, r.stdout or "", r.stderr or ""


def size_of(p):
    return os.path.getsize(p) if os.path.exists(p) else 0


def sha256_of(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def pin(path):
    os.utime(path, (FIXED_MTIME, FIXED_MTIME))


# --------------------------------------------------------------------------
# 载荷
# --------------------------------------------------------------------------
def build_payloads():
    """两份可区分的小载荷，压成 zip1 / zip2。"""
    dir1 = os.path.join(EXT_ASSETS, "payload1")
    dir2 = os.path.join(EXT_ASSETS, "payload2")
    for d in (dir1, dir2):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d, exist_ok=True)

    rnd = random.Random(20261016)
    # payload1：6 个成员，3 个可压缩文本 + 2 个不可压缩随机 + 1 个中文名
    files1 = [
        ("r1_a.txt", ("A" * 400) * 15),                      # ~6KB 文本
        ("r2_b.bin", rnd.randbytes(6000)),                   # 6000 随机（deflate 里几乎 stored）
        ("r3_c.txt", ("中文内容测试行\n" * 500)),             # 多字节文本
        ("r4_d.bin", rnd.randbytes(6000)),
        ("r5_e.txt", ("E" * 300) * 20),
        ("中文_名.txt", "文件名与内容都含中文\n" * 200),
    ]
    for name, content in files1:
        if isinstance(content, str):
            content = content.encode("utf-8")
        with open(os.path.join(dir1, name), "wb") as f:
            f.write(content)
    files2 = [
        ("q1.txt", ("Q" * 200) * 10),
        ("q2.bin", rnd.randbytes(4000)),
        ("q3_二.txt", "第二个压缩包的独有内容\n" * 100),
    ]
    for name, content in files2:
        if isinstance(content, str):
            content = content.encode("utf-8")
        with open(os.path.join(dir2, name), "wb") as f:
            f.write(content)

    zip1 = os.path.join(EXT_ASSETS, "payload1.zip")
    zip2 = os.path.join(EXT_ASSETS, "payload2.zip")
    for d, z in ((dir1, zip1), (dir2, zip2)):
        if os.path.exists(z):
            os.remove(z)
        with zipfile.ZipFile(z, "w", zipfile.ZIP_DEFLATED) as zf:
            for fn in sorted(os.listdir(d)):
                zf.write(os.path.join(d, fn), "payload/" + fn)
    pin(zip1)
    pin(zip2)
    return zip1, zip2, dir1, dir2


def build_videos():
    """两个小视频。ev10 = 10s（E2 用，需从中间切开）；ev6 = 6s（E3/E4/E6/E7 用）。"""
    cover = os.path.join(ASSETS, "cover")
    os.makedirs(cover, exist_ok=True)
    outs = {}
    specs = [
        ("ev10.mp4", 10, []),
        ("ev10_fast.mp4", 10, ["-movflags", "+faststart"]),
        ("ev6.mp4", 6, []),
    ]
    for name, dur, extra in specs:
        p = os.path.join(cover, name)
        cmd = [FFMPEG, "-hide_banner", "-loglevel", "error", "-f", "lavfi",
               "-i", f"testsrc=size=160x120:rate=10:duration={dur}",
               "-c:v", "mpeg4", "-q:v", "25"] + extra + ["-y", p]
        rc, _o, err = sh(cmd)
        if rc != 0:
            raise SystemExit(f"ffmpeg 生成 {name} 失败: {err[-400:]}")
        outs[name] = p
    return outs


def read_bytes(p):
    with open(p, "rb") as f:
        return f.read()


def zip_members(zip_path):
    """返回 [(name, size, crc, header_offset, compress_size)] 按物理位置排序。"""
    with zipfile.ZipFile(zip_path) as z:
        infos = sorted(z.infolist(), key=lambda i: i.header_offset)
        return [(i.filename, i.file_size, i.CRC, i.header_offset, i.compress_size)
                for i in infos]


def ziplen_upto_cd(zip_path):
    """CD 起点（= EOCD 里 offset_cd 的绝对位置，这里 zip 无前缀，绝对=相对）。"""
    data = read_bytes(zip_path)
    e = data.rfind(b"PK\x05\x06")
    return int.from_bytes(data[e + 16:e + 20], "little"), e


# --------------------------------------------------------------------------
# E1~E7
# --------------------------------------------------------------------------
def write(path, *chunks):
    with open(path, "wb") as f:
        for c in chunks:
            f.write(c)
    pin(path)
    return size_of(path)


def main():
    os.makedirs(DATA, exist_ok=True)
    os.makedirs(WORK, exist_ok=True)
    os.makedirs(EXT_ASSETS, exist_ok=True)

    zip1, zip2, _d1, _d2 = build_payloads()
    vids = build_videos()
    z1 = read_bytes(zip1)
    z2 = read_bytes(zip2)
    ev10 = read_bytes(vids["ev10.mp4"])
    ev10f = read_bytes(vids["ev10_fast.mp4"])
    ev6 = read_bytes(vids["ev6.mp4"])
    cd_off1, eocd1 = ziplen_upto_cd(zip1)
    cd_off2, eocd2 = ziplen_upto_cd(zip2)

    m1 = zip_members(zip1)
    m2 = zip_members(zip2)
    # 每个成员在 zip 里的物理区间 [header_offset, 下一个 header_offset 或 CD 起点)
    starts1 = [x[3] for x in m1]
    ends1 = starts1[1:] + [cd_off1]
    ext1 = list(zip(m1, starts1, ends1))
    starts2 = [x[3] for x in m2]
    ends2 = starts2[1:] + [cd_off2]

    rnd = random.Random(99887766)
    JUNK = rnd.randbytes(5 * 1024 * 1024)

    manifest = {"zip1": os.path.basename(zip1), "zip1_len": len(z1),
                "zip2": os.path.basename(zip2), "zip2_len": len(z2),
                "members1": [{"name": a, "size": b, "crc": c, "hdr": d, "csize": e}
                             for (a, b, c, d, e) in m1],
                "members2": [{"name": a, "size": b, "crc": c, "hdr": d, "csize": e}
                             for (a, b, c, d, e) in m2],
                "samples": {}}

    made = {}

    # ---- E1 [垃圾5MB][ZIP][垃圾5MB]
    p = os.path.join(DATA, "E1_junk5m_zip_junk5m.bin")
    sz = write(p, JUNK, z1, JUNK)
    made["E1_junk5m_zip_junk5m.bin"] = {
        "structure": "[随机垃圾 5MiB][完整 ZIP1][随机垃圾 5MiB]",
        "zip_span": [len(JUNK), len(JUNK) + len(z1)],
        "note": "EOCD 距文件尾整整 5MiB → 尾部 64KB 窗口够不着"}

    # ---- E2 [视频前半][ZIP][视频后半]
    mid = len(ev10) // 2
    p = os.path.join(DATA, "E2_video_split_zip_middle.mp4")
    sz = write(p, ev10[:mid], z1, ev10[mid:])
    made["E2_video_split_zip_middle.mp4"] = {
        "structure": f"[ev10 前半 {mid}B][完整 ZIP1][ev10 后半 {len(ev10)-mid}B]",
        "zip_span": [mid, mid + len(z1)],
        "video": "ev10.mp4", "video_split": mid, "video_len": len(ev10),
        "video_sha256": hashlib.sha256(ev10).hexdigest(),
        "note": "ZIP 完整地在视频中间；把两段视频拼回应还原原始视频"}

    # ---- E3 [ZIP前半][视频][ZIP后半]（切点落在**成员边界**）
    s = ext1[3][2]                            # = 成员 r4_d.bin 结束 / r5_e.txt 开始的边界
    p = os.path.join(DATA, "E3_zip_split_by_video.mp4")
    sz = write(p, z1[:s], ev6, z1[s:])
    made["E3_zip_split_by_video.mp4"] = {
        "structure": f"[ZIP1 前 {s}B][ev6 {len(ev6)}B][ZIP1 后 {len(z1)-s}B]",
        "zip_part1": [0, s], "inserted": [s, s + len(ev6)],
        "zip_part2": [s + len(ev6), s + len(ev6) + len(z1) - s],
        "note": "切点落在成员边界(r4_d.bin 结束处)；两段 ZIP 都在 → 虚接跳过视频应直接还原完整 ZIP1"}

    # ---- E3b 切点落在**成员数据中间**（更狠的变体）
    s2 = (ext1[3][1] + ext1[3][2]) // 2      # 落在 r4_d.bin 数据中段
    p = os.path.join(DATA, "E3b_zip_cut_inside_member.mp4")
    sz = write(p, z1[:s2], ev6, z1[s2:])
    made["E3b_zip_cut_inside_member.mp4"] = {
        "structure": f"[ZIP1 前 {s2}B][ev6 {len(ev6)}B][ZIP1 后 {len(z1)-s2}B]",
        "zip_part1": [0, s2], "inserted": [s2, s2 + len(ev6)],
        "zip_part2": [s2 + len(ev6), s2 + len(ev6) + len(z1) - s2],
        "note": "切点在成员 r4_d.bin 的数据中间 → 虚接要精确知道切点，需 CRC 引导搜索"}

    # ---- E4 [ZIP前半][视频覆盖原中段][ZIP后半]
    #       洞 = 第 4 个成员(索引 3, r4_d.bin) 的整个物理区间
    hole_start, hole_end = ext1[3][1], ext1[3][2]
    hole_len = hole_end - hole_start
    cover = (ev6 * ((hole_len // len(ev6)) + 1))[:hole_len]
    p = os.path.join(DATA, "E4_zip_middle_overwritten.mp4")
    sz = write(p, z1[:hole_start], cover, z1[hole_end:])
    made["E4_zip_middle_overwritten.mp4"] = {
        "structure": f"[ZIP1 前 {hole_start}B][视频覆盖 {hole_len}B][ZIP1 后 {len(z1)-hole_end}B]",
        "hole": [hole_start, hole_end], "hole_len": hole_len,
        "lost_member": m1[3][0],
        "survivors": [m1[i][0] for i in range(len(m1)) if i != 3],
        "note": "原第 4 成员(r4_d.bin)的字节真的没了；其余成员物理位置不变"}

    # ---- E5 头尾截断
    head_cut = ext1[0][2]          # 砍掉第 1 成员整个区间
    tail_cut = 200                 # 砍掉 EOCD + 部分 CD
    p = os.path.join(DATA, "E5_zip_headtail_truncated.zip")
    body = z1[head_cut:len(z1) - tail_cut]
    sz = write(p, body)
    made["E5_zip_headtail_truncated.zip"] = {
        "structure": f"[ZIP1 去掉头 {head_cut}B + 尾 {tail_cut}B]",
        "head_cut": head_cut, "tail_cut": tail_cut,
        "lost_head_member": m1[0][0],
        "survivors": [m1[i][0] for i in range(1, len(m1))],
        "note": "EOCD/中心目录丢失；只剩中间成员可救"}

    # ---- E6 [ZIP1][视频][ZIP2]
    p = os.path.join(DATA, "E6_two_zips.mp4")
    sz = write(p, z1, ev6, z2)
    made["E6_two_zips.mp4"] = {
        "structure": f"[完整 ZIP1 {len(z1)}B][ev6 {len(ev6)}B][完整 ZIP2 {len(z2)}B]",
        "zip1_span": [0, len(z1)], "zip2_span": [len(z1) + len(ev6), len(z1) + len(ev6) + len(z2)],
        "note": "两个不同的压缩包；只看最后一个 EOCD 会漏掉第一个"}

    # ---- E7 [视频][ZIP1][视频][ZIP2]
    p = os.path.join(DATA, "E7_vid_zip_vid_zip.mp4")
    sz = write(p, ev10f, z1, ev6, z2)
    made["E7_vid_zip_vid_zip.mp4"] = {
        "structure": f"[ev10_fast {len(ev10f)}B][ZIP1][ev6 {len(ev6)}B][ZIP2]",
        "zip1_span": [len(ev10f), len(ev10f) + len(z1)],
        "zip2_span": [len(ev10f) + len(z1) + len(ev6), len(ev10f) + len(z1) + len(ev6) + len(z2)],
        "note": "多个穿插（含前后都有视频）"}

    manifest["samples"] = made
    with open(os.path.join(HERE, "extreme_manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)

    print("===== 变态场景样本清单 =====")
    tot = 0
    for fn in sorted(os.listdir(DATA)):
        if fn.startswith("E"):
            s = size_of(os.path.join(DATA, fn))
            tot += s
            print(f"  {fn:42s} {s:>10,} B")
    print(f"  {'合计':42s} {tot:>10,} B  ({tot/1024/1024:.2f} MiB)")
    print("\nzip1 成员（物理序）:")
    for (a, b, c, d, e), st, en in ext1:
        print(f"  {a:14s} size={b:>6} csize={e:>6} hdr={st:>6} 区间[{st},{en})")
    print("zip2:", [m2[i][0] for i in range(len(m2))])
    print("\nmanifest -> tests/extreme_manifest.json")


if __name__ == "__main__":
    main()
