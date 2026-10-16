#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""make_pack_layouts.py —— 生成 L1~L8「压缩（打包）布局」样本。

每条布局都产出两版：`*_plain`（无密码，ZIP_DEFLATED）与 `*_pwd`
（AES-256 / AE-2 / ZIP_STORED，与 SteganographierGUI 的加密分支一致）。

布局定义（L = Layout；「外壳」= 一个 5 秒 320x240 mpeg4 的小 MP4）
  L1  尾部追加 ZIP            [外壳 MP4][ZIP]
  L2  尾部 free 原子包 ZIP     [外壳 MP4][free 头 8B][随机 pad 64~256B][ZIP]
                              （隐写者 1.3.x 模式A：不改外壳任何字节）
  L3  ftyp 后 free 原子内嵌 ZIP [ftyp][free 头+ZIP][mdat][moov(stco 已补偿)][尾部噪声]
                              （隐写者模式B「按注释意图」的修正版：噪声写在文件末尾，
                                 stco/co64 += len(free_atom)）
  L4  插在视频中间            [.. mdat 前半 ..][ZIP][.. mdat 后半 ..]
                              （插点选在 stco 记录的 chunk 边界；mdat size += len(ZIP)；
                                  stco/co64 里 >= 插点的偏移 += len(ZIP)）
  L5  分散多段                ZIP 切 3 块插到 3 个 chunk 边界，逻辑顺序拼起来仍是完整 ZIP
  L6  前后都加垃圾            [5MiB 垃圾][外壳 MP4][ZIP][5MiB 垃圾]
  L7  同一个文件里放两个独立包  [外壳 MP4][ZIP1][ZIP2]（两个内容不同的包）
  L8  外套 MKV（附件轨）       mkv：cover5.mp4 转封装 + ZIP 作附件
                              （本机无 mkvmerge，用 ffmpeg -attach 达到同等字节结构）

资源红线：单样本 <= 30MB、样本总量 <= 150MB；写盘全部流式（1MiB 块）。

用法：
    python3 tests/make_pack_layouts.py            # 生成全部，落到 tests/data/pack/
    python3 tests/make_pack_layouts.py --only L4 L5
"""
import argparse
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
ASSETS = os.path.join(HERE, "assets", "pack")
DATA = os.path.join(HERE, "data", "pack")
WORK = os.path.join(HERE, ".work", "pack")

FFMPEG = "/usr/bin/ffmpeg"
SEVENZIP = "/usr/bin/7z"

PWD = "P@ssw0rd!2026"
FIXED_MTIME = 1750000000
JUNK_MIB = 5                      # L6 前后各 5MiB 垃圾
CHUNK = 1 << 20                   # 流式块大小

CONTAINERS = {b"moov", b"trak", b"mdia", b"minf", b"stbl", b"udta", b"edts", b"mvex"}

# 隐写者模式B 用的伪造压缩包特征码池
FAKE_SIGS = [b"Rar!\x1a\x07\x00", b"Rar!\x1a\x07\x01\x00", b"7z\xbc\xaf\x27\x1c",
             b"PK\x03\x04", b"\x1f\x8b\x08", b"BZh9", b"\xfd7zXZ\x00"]


# --------------------------------------------------------------------------
# 小工具
# --------------------------------------------------------------------------
def sh(cmd, timeout=300, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd)
    return r.returncode, r.stdout or "", r.stderr or ""


def reexec_with_pyzipper():
    """AES-256/AE-2 ZIP 必须靠 pyzipper（与隐写者写端对齐）。
    如果当前解释器没有它，就换一个有的再跑一遍（用环境变量防死循环）。"""
    if os.environ.get("PACK_REEXEC") == "1":
        return
    try:
        import pyzipper  # noqa: F401
        return
    except ImportError:
        pass
    import glob
    cands = sorted(glob.glob(os.path.expanduser("~/.hermes/tools/python-*/bin/python3")))
    cands += [shutil.which("python3"), "/usr/bin/python3"]
    for c in cands:
        if not c or not os.path.exists(c):
            continue
        try:
            r = subprocess.run([c, "-c", "import pyzipper"], capture_output=True, timeout=30)
        except Exception:
            continue
        if r.returncode == 0:
            os.environ["PACK_REEXEC"] = "1"
            os.execv(c, [c, os.path.abspath(sys.argv[0])] + sys.argv[1:])
    print("[warn] 找不到带 pyzipper 的解释器 → AES 包退化为 7z -mem=AES256", file=sys.stderr)


def size_of(p):
    return os.path.getsize(p) if os.path.exists(p) else 0


def sha256_of(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(CHUNK), b""):
            h.update(b)
    return h.hexdigest()


def stable_seed(name):
    """确定性种子：保证同一布局每次重跑都产出完全相同的字节
    （不能用 hash()——它受 PYTHONHASHSEED 影响，进程间不稳定）。"""
    return int.from_bytes(hashlib.sha256(name.encode()).digest()[:4], "big") & 0xFFFF


def pin(p):
    try:
        os.utime(p, (FIXED_MTIME, FIXED_MTIME))
    except OSError:
        pass


# --------------------------------------------------------------------------
# 载荷（两套内容不同的 payload，供 L7 用）
# --------------------------------------------------------------------------
def build_payload_sources():
    src1 = os.path.join(ASSETS, "payload1")
    src2 = os.path.join(ASSETS, "payload2")
    for d in (src1, src2):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d, exist_ok=True)

    rnd = random.Random(0x5A17)
    files1 = [
        ("README.txt", ("这是一个压缩布局兼容性测试载荷。\n" * 260)),
        ("data.bin", rnd.randbytes(8192)),            # 不可压缩
        ("中文名.txt", ("中文行内容 abcdefghij\n" * 300)),
        ("sub/nest.bin", rnd.randbytes(3072)),
    ]
    files2 = [
        ("second_pack.txt", ("第二个独立压缩包的独有内容 payload-2\n" * 200)),
        ("s2.bin", rnd.randbytes(5120)),
        ("第二个包_中文.txt", ("二包中文\n" * 150)),
    ]
    for d, files in ((src1, files1), (src2, files2)):
        for name, content in files:
            p = os.path.join(d, name)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as f:
                f.write(content.encode("utf-8") if isinstance(content, str) else content)
            pin(p)
    return src1, src2


def _walk_sorted(src):
    out = []
    for root, _dirs, files in os.walk(src):
        for fn in files:
            out.append(os.path.join(root, fn))
    return sorted(out)


def make_zip_plain(src, dst, arcprefix):
    if os.path.exists(dst):
        os.remove(dst)
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as zf:
        for p in _walk_sorted(src):
            rel = os.path.relpath(p, src).replace(os.sep, "/")
            zi = zipfile.ZipInfo(f"{arcprefix}{rel}", date_time=(2026, 10, 16, 12, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = 0o644 << 16
            zf.writestr(zi, open(p, "rb").read())
    pin(dst)
    return dst


def make_zip_pwd(src, dst, arcprefix):
    """AES-256 / AE-2 / ZIP_STORED —— 与隐写者 compress_files 的加密分支一致。"""
    if os.path.exists(dst):
        os.remove(dst)
    try:
        import pyzipper
    except ImportError:
        pyzipper = None
    if pyzipper is not None:
        with pyzipper.AESZipFile(dst, "w",
                                 compression=pyzipper.ZIP_STORED,
                                 encryption=pyzipper.WZ_AES) as zf:
            zf.setpassword(PWD.encode("utf-8"))
            for p in _walk_sorted(src):
                rel = os.path.relpath(p, src).replace(os.sep, "/")
                zf.write(p, arcname=f"{arcprefix}{rel}",
                         compress_type=pyzipper.ZIP_STORED)
        pin(dst)
        return dst
    # 退化路径：7z 造 AES-256 zip（同样 AE-2 / AES-256）
    tmpdir = os.path.join(WORK, "aestmp")
    shutil.rmtree(tmpdir, ignore_errors=True)
    os.makedirs(tmpdir, exist_ok=True)
    sh([SEVENZIP, "a", "-tzip", "-mem=AES256", "-p" + PWD, "-mm=Copy",
        os.path.join(tmpdir, "p.zip"), "."], cwd=src)
    shutil.move(os.path.join(tmpdir, "p.zip"), dst)
    pin(dst)
    return dst


# --------------------------------------------------------------------------
# MP4 box 工具（只读 moov 那一段，其余流式）
# --------------------------------------------------------------------------
def read_at(path, pos, n):
    with open(path, "rb") as f:
        f.seek(pos)
        return f.read(n)


def top_atoms(path):
    """顺序扫顶层 box，返回 [(offset, size, type_bytes)]。size 含 8/16 字节头。"""
    total = os.path.getsize(path)
    out = []
    pos = 0
    with open(path, "rb") as f:
        while pos + 8 <= total:
            f.seek(pos)
            h = f.read(16)
            if len(h) < 8:
                break
            size = int.from_bytes(h[:4], "big")
            typ = h[4:8]
            if size == 1:
                if len(h) < 16:
                    break
                size = int.from_bytes(h[8:16], "big")
            elif size == 0:
                size = total - pos
            if size < 8 or pos + size > total:
                break
            out.append((pos, size, typ))
            pos += size
    return out


def box_hdr_len(data, p):
    size = int.from_bytes(data[p:p + 4], "big")
    if size == 1:
        return 16, size
    if size == 0:
        return 8, len(data) - p
    return 8, size


def find_stco_in(data, base):
    """在 moov 字节里递归找 stco/co64。
    返回 [{'abs': stco盒绝对偏移, 'type': b'stco'|b'co64', 'n': 条目数,
            'ent_rel': 条目的 moov 内相对偏移}]"""
    res = []

    def walk(start, end):
        p = start
        while p + 8 <= end:
            size = int.from_bytes(data[p:p + 4], "big")
            typ = data[p + 4:p + 8]
            hdr = 8
            if size == 1:
                if p + 16 > end:
                    return
                size = int.from_bytes(data[p + 8:p + 16], "big")
                hdr = 16
            elif size == 0:
                size = end - p
            if size < hdr or p + size > end:
                return
            if typ in (b"stco", b"co64"):
                n = int.from_bytes(data[p + hdr + 4:p + hdr + 8], "big")
                res.append({"abs": base + p, "type": typ, "n": n,
                            "ent_rel": p + hdr + 8, "box_off": p, "box_size": size})
            elif typ in CONTAINERS:
                walk(p + hdr, p + size)
            p += size

    walk(0, len(data))
    return res


def stco_entries(path):
    """返回 (moov_off, moov_size, moov_bytes, [ {type,n,ent_rel,abs} ])"""
    atoms = top_atoms(path)
    moov = [a for a in atoms if a[2] == b"moov"]
    if not moov:
        raise SystemExit(f"{path} 没有 moov")
    moff, msize, _ = moov[0]
    mdata = read_at(path, moff, msize)
    return moff, msize, mdata, find_stco_in(mdata, moff)


def entry_values(path, entinfo):
    moff, msize, mdata, _ = entinfo
    out = []
    for e in entinfo[3]:
        w = 4 if e["type"] == b"stco" else 8
        for i in range(e["n"]):
            o = e["ent_rel"] + i * w
            if o + w > len(mdata):
                break
            out.append((e["box_off"], i, w, int.from_bytes(mdata[o:o + w], "big")))
    return out


def patch_moov(mdata, entinfo, inserts):
    """inserts: [(原始文件偏移, 该点之前累计插入的字节数)]；按偏移升序。
    返回打过补丁的 moov 字节（长度不变）。"""
    buf = bytearray(mdata)
    moff, msize, _mdata, stcos = entinfo
    for e in stcos:
        w = 4 if e["type"] == b"stco" else 8
        for i in range(e["n"]):
            o = e["ent_rel"] + i * w
            if o + w > len(buf):
                break
            val = int.from_bytes(buf[o:o + w], "big")
            delta = 0
            for (pt, acc) in inserts:
                if val >= pt:
                    delta += acc
            if delta:
                buf[o:o + w] = (val + delta).to_bytes(w, "big")
    return bytes(buf)


def patch_box_size(path, box_off, delta, mdata=None):
    """把一个 box 的 size 字段 +delta，返回补丁后的 (off, 被替换长度, 新字节)。
    注意：被替换长度必须与实际写出的字节数一致，否则会把后续字节写重。"""
    raw = read_at(path, box_off, 16)
    size = int.from_bytes(raw[:4], "big")
    if size == 1:
        new = int.from_bytes(raw[8:16], "big") + delta
        return (box_off, 16, raw[:8] + new.to_bytes(8, "big"))
    return (box_off, 4, (size + delta).to_bytes(4, "big"))


# --------------------------------------------------------------------------
# 输出组装（流式）
# --------------------------------------------------------------------------
class Composer:
    """把外壳 MP4 + 若干「插入物」+ 若干「定长替换」按原始坐标组装成新文件。"""

    def __init__(self, cover):
        self.cover = cover
        self.size = os.path.getsize(cover)
        self.inserts = []     # (off, bytes)
        self.patches = []     # (off, length, bytes)

    def ins(self, off, data):
        self.inserts.append((off, data))

    def patch(self, off, length, data):
        self.patches.append((off, length, data))

    def render(self, out_path):
        events = [(o, 0, d) for o, d in self.inserts] + \
                 [(o, l, d) for o, l, d in self.patches]
        events.sort(key=lambda e: (e[0], e[1]))
        tmp = out_path + ".tmp"
        with open(self.cover, "rb") as fin, open(tmp, "wb") as fout:
            pos = 0

            def copy_upto(end):
                nonlocal pos
                fin.seek(pos)
                left = end - pos
                while left > 0:
                    b = fin.read(min(CHUNK, left))
                    if not b:
                        raise IOError("cover 读取越界")
                    fout.write(b)
                    left -= len(b)
                pos = end

            for off, plen, payload in events:
                if off < pos:
                    continue
                copy_upto(off)
                fout.write(payload)
                if plen:
                    pos = off + plen
            copy_upto(self.size)
        os.replace(tmp, out_path)
        pin(out_path)
        return size_of(out_path)


# --------------------------------------------------------------------------
# 外壳视频
# --------------------------------------------------------------------------
def build_covers():
    os.makedirs(ASSETS, exist_ok=True)
    outs = {}
    # 带一条音轨 → mov 复用器会把音视频交错成多个 chunk，stco 里就有真实的多条 chunk 边界，
    # L4/L5 才能把 ZIP 插在「两个 chunk 之间」（既不切断样本，又能让 stco 精确补偿）。
    specs = [("cover5.mp4", 5, []),
             ("cover5_fast.mp4", 5, ["-movflags", "+faststart"]),
             ("cover5_vonly.mp4", 5, ["-an"])]
    for name, dur, extra in specs:
        p = os.path.join(ASSETS, name)
        cmd = [FFMPEG, "-hide_banner", "-loglevel", "error",
               "-f", "lavfi", "-i", f"testsrc=size=320x240:rate=10:duration={dur}",
               "-f", "lavfi", "-i", f"sine=frequency=440:sample_rate=44100:duration={dur}",
               "-c:v", "mpeg4", "-q:v", "8", "-pix_fmt", "yuv420p",
               "-c:a", "aac", "-b:a", "32k"] + extra + ["-y", p]
        rc, _o, err = sh(cmd)
        if rc != 0:
            raise SystemExit(f"ffmpeg 生成 {name} 失败: {err[-400:]}")
        outs[name] = p
        pin(p)
    return outs


def noise_block(rnd):
    """隐写者模式B 的噪声块：假特征码 + urandom + 假特征码 + urandom + 空 mdat。"""
    out = bytearray()
    out += rnd.choice(FAKE_SIGS)
    out += rnd.randbytes(1024 * rnd.randint(5, 10))
    out += rnd.choice(FAKE_SIGS)
    out += rnd.randbytes(1024 * rnd.randint(5, 10))
    out += b"\x00\x00\x00\x08mdat"
    return bytes(out)


def junk_stream(nbytes, seed):
    """确定性伪随机垃圾，但按块生成，不整包驻留。"""
    rnd = random.Random(seed)
    blk = rnd.randbytes(CHUNK)
    return blk, nbytes


def write_junk(fout, nbytes, seed):
    blk, n = junk_stream(min(nbytes, CHUNK), seed)
    left = nbytes
    while left > 0:
        take = min(left, len(blk))
        fout.write(blk[:take])
        left -= take


# --------------------------------------------------------------------------
# L1 ~ L8
# --------------------------------------------------------------------------
def layout_L1(cover, zip_bytes, out, ctx):
    c = Composer(cover)
    c.ins(os.path.getsize(cover), zip_bytes)
    c.render(out)
    return "尾部追加 ZIP", [(os.path.getsize(cover), len(zip_bytes))]


def layout_L2(cover, zip_bytes, out, ctx):
    rnd = random.Random(ctx["seed"])
    pad = rnd.randint(64, 256)
    head = (8 + pad + len(zip_bytes)).to_bytes(4, "big") + b"free"
    c = Composer(cover)
    c.ins(os.path.getsize(cover), head + rnd.randbytes(pad) + zip_bytes)
    c.render(out)
    return f"尾部 free 原子（8B 头 + {pad}B 随机 pad + ZIP）", \
           [(os.path.getsize(cover) + 8 + pad, len(zip_bytes))]


def layout_L3(cover, zip_bytes, out, ctx):
    atoms = top_atoms(cover)
    ftyp = [a for a in atoms if a[2] == b"ftyp"][0]
    ins_at = ftyp[0] + ftyp[1]
    free = (8 + len(zip_bytes)).to_bytes(4, "big") + b"free" + zip_bytes
    moff, msize, mdata, ent = stco_entries(cover)
    patched = patch_moov(mdata, (moff, msize, mdata, ent), [(ins_at, len(free))])
    rnd = random.Random(ctx["seed"] + 3)
    tail = noise_block(rnd) + noise_block(rnd)
    c = Composer(cover)
    c.ins(ins_at, free)
    c.patch(moff, msize, patched)
    c.ins(os.path.getsize(cover), tail)
    c.render(out)
    return (f"ftyp 后 free 原子内嵌 ZIP（stco += {len(free)}）+ 尾部噪声 {len(tail)}B",
            [(ins_at + 8, len(zip_bytes))])


def _chunk_points(cover, want, mdat):
    """在 mdat 载荷区内挑 want 个 chunk 起点（stco 记录的真实 chunk 偏移）。"""
    mstart, msize, _typ = mdat
    lo = mstart + (16 if int.from_bytes(read_at(cover, mstart, 4), "big") == 1 else 8)
    hi = mstart + msize
    ent = stco_entries(cover)
    vals = sorted({v for (_b, _i, _w, v) in entry_values(cover, ent) if lo <= v < hi})
    if len(vals) < want:
        # chunk 太少 → 退化为等距切点（仍落在 mdat 载荷内）
        span = hi - lo
        vals = [lo + span * (i + 1) // (want + 1) for i in range(want)]
    else:
        idx = [(i + 1) * len(vals) // (want + 1) for i in range(want)]
        vals = [vals[i] for i in idx]
    return sorted(set(vals))


def layout_L4(cover, zip_bytes, out, ctx):
    atoms = top_atoms(cover)
    mdat = [a for a in atoms if a[2] == b"mdat"][0]
    pts = _chunk_points(cover, 1, mdat)
    at = pts[len(pts) // 2]
    moff, msize, mdata, ent = stco_entries(cover)
    patched = patch_moov(mdata, (moff, msize, mdata, ent), [(at, len(zip_bytes))])
    c = Composer(cover)
    c.ins(at, zip_bytes)
    c.patch(moff, msize, patched)
    c.patch(*patch_box_size(cover, mdat[0], len(zip_bytes)))
    c.render(out)
    return (f"mdat 内 chunk 边界插入 ZIP（插点={at}, mdat size += {len(zip_bytes)}, stco/co64 分段补偿）",
            [(at, len(zip_bytes))])


def layout_L5(cover, zip_bytes, out, ctx):
    atoms = top_atoms(cover)
    mdat = [a for a in atoms if a[2] == b"mdat"][0]
    pts = _chunk_points(cover, 3, mdat)
    parts = split_zip_by_members(zip_bytes, len(pts))
    # patch_moov 的语义是「插点位置 → 该点插入了多少字节」，对 v >= 插点 的偏移累加。
    # 所以这里只传每个插点自身的长度（不要传累计值，否则会重复累加）。
    ins_list = [(at, part) for at, part in zip(pts, parts)]
    moff, msize, mdata, ent = stco_entries(cover)
    patched = patch_moov(mdata, (moff, msize, mdata, ent),
                         [(at, len(part)) for (at, part) in ins_list])
    c = Composer(cover)
    for at, part in ins_list:
        c.ins(at, part)
    c.patch(moff, msize, patched)
    c.patch(*patch_box_size(cover, mdat[0], len(zip_bytes)))
    c.render(out)
    spans = [(at, len(part)) for at, part in ins_list]
    return (f"ZIP 切 {len(parts)} 块分插到 {pts}（mdat size += {len(zip_bytes)}, stco 分段补偿）",
            spans)


def layout_L6(cover, zip_bytes, out, ctx):
    csize = os.path.getsize(cover)
    junk_n = JUNK_MIB * 1024 * 1024
    tmp = out + ".tmp"
    with open(tmp, "wb") as fout:
        write_junk(fout, junk_n, ctx["seed"] + 11)
        with open(cover, "rb") as fin:
            for b in iter(lambda: fin.read(CHUNK), b""):
                fout.write(b)
        fout.write(zip_bytes)
        write_junk(fout, junk_n, ctx["seed"] + 22)
    os.replace(tmp, out)
    pin(out)
    return (f"[{JUNK_MIB}MiB 垃圾][外壳 MP4][ZIP][{JUNK_MIB}MiB 垃圾]（外壳整体被前置推后 {junk_n}B，未改 moov）",
            [(junk_n + csize, len(zip_bytes))])


def layout_L7(cover, zip_bytes, out, ctx):
    z2 = ctx["zip2_bytes"]
    csize = os.path.getsize(cover)
    c = Composer(cover)
    c.ins(csize, zip_bytes)
    c.ins(csize, z2)
    c.render(out)
    return ("尾部追加两个独立 ZIP（ZIP1 再用 ZIP2 接在其后）",
            [(csize, len(zip_bytes)), (csize + len(zip_bytes), len(z2))])


def layout_L8(cover, zip_bytes, out, ctx):
    zp = os.path.join(WORK, "attach_" + os.path.basename(out) + ".zip")
    with open(zp, "wb") as f:
        f.write(zip_bytes)
    cmd = [FFMPEG, "-hide_banner", "-loglevel", "error", "-y",
           "-i", cover, "-attach", zp,
           "-metadata:s:t:0", "mimetype=application/zip",
           "-metadata:s:t:0", "filename=payload.zip",
           "-map", "0", "-c", "copy",
           "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
           out]
    rc, _o, err = sh(cmd)
    if rc != 0:
        raise SystemExit(f"ffmpeg 封装 MKV 失败: {err[-500:]}")
    pin(out)
    return ("MKV 外套：视频轨 = cover5 原样转封装，ZIP 作 MKV 附件轨（mimetype=application/zip）", [])


# --------------------------------------------------------------------------
def split_zip_by_members(zip_bytes, nparts):
    """按成员边界把 ZIP 切成 nparts 段（段序拼接 == 原 ZIP）。"""
    import io
    zio = io.BytesIO(zip_bytes)
    with zipfile.ZipFile(zio) as z:
        infos = sorted(z.infolist(), key=lambda i: i.header_offset)
        starts = [i.header_offset for i in infos]
    eocd = zip_bytes.rfind(b"PK\x05\x06")
    size_cd = int.from_bytes(zip_bytes[eocd + 12:eocd + 16], "little")
    cd_start = eocd - size_cd
    bounds = [0] + starts + [cd_start, len(zip_bytes)]
    bounds = sorted(set(b for b in bounds if 0 <= b <= len(zip_bytes)))
    if len(bounds) - 1 < nparts:
        step = len(zip_bytes) // nparts
        cuts = [step * k for k in range(1, nparts)]
    else:
        idx = [(i + 1) * (len(bounds) - 1) // nparts for i in range(nparts - 1)]
        cuts = [bounds[i] for i in idx]
    edges = [0] + cuts + [len(zip_bytes)]
    return [zip_bytes[edges[i]:edges[i + 1]] for i in range(len(edges) - 1)]


LAYOUTS = {
    "L1": layout_L1, "L2": layout_L2, "L3": layout_L3, "L4": layout_L4,
    "L5": layout_L5, "L6": layout_L6, "L7": layout_L7, "L8": layout_L8,
}
DESC = {
    "L1": "尾部追加 ZIP（标准 / WinRAR 兼容）",
    "L2": "尾部 free 原子包 ZIP（隐写者 1.3.x 模式A）",
    "L3": "ftyp 后 free 原子内嵌 ZIP（隐写者模式B 修正版）",
    "L4": "插在视频中间（mdat chunk 边界，改 mdat size + stco）",
    "L5": "分散多段（ZIP 切块分插，收尾处拼接）",
    "L6": "前后都加垃圾（ZIP 夹在 5MiB 垃圾中间）",
    "L7": "同一个文件里放两个独立包",
    "L8": "外套 MKV（附件轨）",
}


def main():
    reexec_with_pyzipper()
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", nargs="*", default=None)
    ap.add_argument("--cover", default="cover5.mp4")
    args = ap.parse_args()

    for d in (ASSETS, DATA, WORK):
        os.makedirs(d, exist_ok=True)

    src1, src2 = build_payload_sources()
    zp = make_zip_plain(src1, os.path.join(ASSETS, "payload1_plain.zip"), "payload/")
    zpw = make_zip_pwd(src1, os.path.join(ASSETS, "payload1_pwd.zip"), "payload/")
    z2p = make_zip_plain(src2, os.path.join(ASSETS, "payload2_plain.zip"), "second/")
    z2w = make_zip_pwd(src2, os.path.join(ASSETS, "payload2_pwd.zip"), "second/")
    covers = build_covers()
    cover = covers[args.cover]

    zbytes = {k: open(v, "rb").read() for k, v in
              (("plain", zp), ("pwd", zpw), ("plain2", z2p), ("pwd2", z2w))}

    print("== 载荷 ==")
    for k in ("plain", "pwd", "plain2", "pwd2"):
        p = {"plain": zp, "pwd": zpw, "plain2": z2p, "pwd2": z2w}[k]
        if k.startswith("plain"):
            with zipfile.ZipFile(p) as z:
                nm = len(z.namelist())
        else:
            with _pwdzip(p) as z:
                nm = len(z.namelist())
        print(f"  {k:7s} {os.path.basename(p):24s} {size_of(p):>8,} B  成员={nm}")
    print(f"  外壳 {os.path.basename(cover)} {size_of(cover):,} B")
    atoms = top_atoms(cover)
    print("  外壳原子链:", " ".join(f"{t.decode('latin1')}({s})" for _o, s, t in atoms))
    moff, msize, mdata, ent = stco_entries(cover)
    vals = [v for (_b, _i, _w, v) in entry_values(cover, (moff, msize, mdata, ent))]
    print(f"  moov@{moff}+{msize}  stco/co64 条目数={len(vals)}  前几个={vals[:6]}")

    only = set(args.only) if args.only else set(LAYOUTS)
    with zipfile.ZipFile(zp) as z:
        members = z.namelist()
    manifest = {"password": PWD, "cover": os.path.basename(cover),
                "members": members,
                "cover_size": size_of(cover),
                "cover_sha256": sha256_of(cover),
                "cover_atoms": [{"off": o, "size": s, "type": t.decode("latin1")}
                                for o, s, t in atoms],
                "stco_entries": len(vals),
                "zip_plain": {"path": os.path.basename(zp), "size": size_of(zp),
                              "sha256": sha256_of(zp)},
                "zip_pwd": {"path": os.path.basename(zpw), "size": size_of(zpw),
                            "sha256": sha256_of(zpw)},
                "zip_plain2": {"path": os.path.basename(z2p), "size": size_of(z2p),
                               "sha256": sha256_of(z2p)},
                "zip_pwd2": {"path": os.path.basename(z2w), "size": size_of(z2w),
                             "sha256": sha256_of(z2w)},
                "layouts": {}}

    total = 0
    for L in sorted(LAYOUTS):
        if L not in only:
            continue
        for variant in ("plain", "pwd"):
            z = zbytes[variant]
            z2 = zbytes["pwd2" if variant == "pwd" else "plain2"]
            out = os.path.join(DATA, f"{L}_{variant}.mp4" if L != "L8" else f"{L}_{variant}.mkv")
            ctx = {"seed": stable_seed(L), "zip2_bytes": z2}
            struct_desc, spans = LAYOUTS[L](cover, z, out, ctx)
            total += size_of(out)
            vrc, _vo, verr = sh([FFMPEG, "-v", "error", "-i", out, "-f", "null", "-"],
                                timeout=300)
            manifest["layouts"][f"{L}_{variant}"] = {
                "layout": L, "variant": variant, "file": os.path.basename(out),
                "size": size_of(out), "sha256": sha256_of(out),
                "structure": struct_desc,
                "zip_sha256": sha256_of(zp if variant == "plain" else zpw),
                "zip_size": size_of(zp if variant == "plain" else zpw),
                "zip_spans": spans,
                "extra_zip_sha256": sha256_of(z2p if variant == "plain" else z2w),
                "video_rc": vrc, "video_err": [l for l in verr.strip().splitlines() if l.strip()][:2],
            }
            print(f"  ✔ {os.path.basename(out):18s} {size_of(out):>10,} B  "
                  f"视频rc={vrc:<4} {struct_desc}")

    with open(os.path.join(HERE, "pack_manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)

    print(f"\n== tests/data/pack 合计 {total:,} B ({total/1024/1024:.2f} MiB) ==")
    import resource
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    print(f"[gen] 本进程峰值 RSS = {peak/1024:.1f} MiB")
    print("manifest -> tests/pack_manifest.json")


class _pwdzip:
    """上下文包装：只用来看成员数（pyzipper 可能缺）。"""
    def __init__(self, p):
        self.p = p

    def __enter__(self):
        try:
            import pyzipper
            self.z = pyzipper.AESZipFile(self.p)
            self.z.setpassword(PWD.encode("utf-8"))
        except ImportError:
            self.z = zipfile.ZipFile(self.p)
        return self.z

    def __exit__(self, *a):
        self.z.close()
        return False


if __name__ == "__main__":
    main()
