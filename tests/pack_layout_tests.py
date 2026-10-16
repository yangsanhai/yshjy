#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""pack_layout_tests.py —— 对 tests/data/pack 的 L1~L8 样本实测「布局 × 兼容性」矩阵。

测四类东西：
  A. 视频可用性      ffmpeg -v error -i F -f null -   的退出码 + stderr
  B. 我们的工具（Python 复刻 tests/extreme_tests.py 的算法，与 Rust 侧同思路）
       1) 全文件流式签名扫描（LFH/CDFH/EOCD + 各种假特征码）
       2) 偏移修正：EOCD 的 size_cd/offset_cd 推起点，再用真实 LFH 校正
       3) 虚接 SplicedIO：把同一文件里不连续的若干区间当一个逻辑流
       4) CRC 自校验后才算「解出」；流式算 sha256 与原始 ZIP 对比
       5) MKV 附件通道（L8）：自己解析 EBML 找 AttachedFile/FileData
  C. 其他解压器      unzip -l/-t、python3 -m zipfile -l、7z l、7z l -t#、7z x、file
  D. 密码            7z x 不带密码 / 带密码；pyzipper 自动代入密码

另跑一组「修正是否救得回视频」的对照实验（写到 .work，不进 data/）。

用法：python3 tests/pack_layout_tests.py [L1 L4 ...]
"""
import hashlib
import io
import json
import os
import resource
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import zipfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import make_pack_layouts as M  # noqa: E402

DATA = os.path.join(HERE, "data", "pack")
MANIFEST = os.path.join(HERE, "pack_manifest.json")
WORK = os.path.join(HERE, ".work", "pack")

FFMPEG = "/usr/bin/ffmpeg"
FFPROBE = "/usr/bin/ffprobe"
SEVENZIP = "/usr/bin/7z"
PY3 = "/usr/bin/python3"            # 「普通用户机器上的 python3」（无 pyzipper）

LFH_SIG = b"PK\x03\x04"
CDFH_SIG = b"PK\x01\x02"
EOCD_SIG = b"PK\x05\x06"
FAKE_SIGS = {
    b"Rar!\x1a\x07\x00": "RAR4", b"Rar!\x1a\x07\x01\x00": "RAR5",
    b"7z\xbc\xaf\x27\x1c": "7Z", b"\x1f\x8b\x08": "GZIP",
    b"BZh9": "BZIP2", b"\xfd7zXZ\x00": "XZ", LFH_SIG: "ZIP",
}
MAX_CANDS = 400


def reexec_with_pyzipper():
    M.reexec_with_pyzipper()


# --------------------------------------------------------------------------
# 带内存测量的子进程
# --------------------------------------------------------------------------
def run_measured(cmd, timeout=300, cwd=None):
    t0 = time.time()
    outf = tempfile.TemporaryFile()
    errf = tempfile.TemporaryFile()
    pid = os.fork()
    if pid == 0:
        try:
            os.chdir(cwd or os.getcwd())
            os.dup2(outf.fileno(), 1)
            os.dup2(errf.fileno(), 2)
            os.execvp(cmd[0], cmd)
        except Exception:
            os._exit(127)
    status, ru, deadline = None, None, t0 + timeout
    while True:
        wpid, st, rusage = os.wait4(pid, os.WNOHANG)
        if wpid == pid:
            status, ru = st, rusage
            break
        if time.time() > deadline:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.wait4(pid, 0)
            status = -9
            break
        time.sleep(0.01)
    el = time.time() - t0
    outf.seek(0); out = outf.read().decode("utf-8", "replace"); outf.close()
    errf.seek(0); err = errf.read().decode("utf-8", "replace"); errf.close()
    rc = os.waitstatus_to_exitcode(status) if isinstance(status, int) and status >= 0 else status
    return {"rc": rc, "out": out, "err": err, "sec": round(el, 3),
            "rss_kb": ru.ru_maxrss if ru else 0}


def tail_lines(s, n=3, width=110):
    ls = [l.rstrip() for l in s.splitlines() if l.strip()]
    return [l[:width] for l in ls[-n:]]


# --------------------------------------------------------------------------
# A. 视频
# --------------------------------------------------------------------------
def video_check(path):
    r = run_measured([FFMPEG, "-v", "error", "-i", path, "-f", "null", "-"], timeout=300)
    p = run_measured([FFPROBE, "-v", "error", "-show_entries",
                      "format=format_name,duration:stream=codec_type,codec_name",
                      "-of", "default=nw=1", path], timeout=120)
    streams = [l.split("=", 1)[1] for l in p["out"].splitlines()
               if l.startswith("codec_type=")]
    return {"ffmpeg_rc": r["rc"], "ffmpeg_err": tail_lines(r["err"], 2),
            "ffprobe_rc": p["rc"], "ffprobe_streams": streams,
            "rss_kb": max(r["rss_kb"], p["rss_kb"]), "sec": r["sec"]}


def video_check_quick(path):
    r = run_measured([FFMPEG, "-v", "error", "-i", path, "-f", "null", "-"], timeout=300)
    return r["rc"], tail_lines(r["err"], 2)


# --------------------------------------------------------------------------
# 扫描 + 虚接（与 extreme_tests.py 同思路）
# --------------------------------------------------------------------------
def scan_sigs(path, chunk=1 << 20):
    pats = [(LFH_SIG, "LFH"), (CDFH_SIG, "CDFH"), (EOCD_SIG, "EOCD")]
    res = {"LFH": [], "CDFH": [], "EOCD": []}
    fake = {}
    with open(path, "rb") as f:
        base, carry = 0, b""
        while True:
            buf = f.read(chunk)
            if not buf:
                break
            data = carry + buf
            dbase = base - len(carry)
            for pat, name in pats:
                i = 0
                while True:
                    j = data.find(pat, i)
                    if j < 0:
                        break
                    res[name].append(dbase + j)
                    i = j + 1
            for pat, name in FAKE_SIGS.items():
                i = 0
                while True:
                    j = data.find(pat, i)
                    if j < 0:
                        break
                    fake[name] = fake.get(name, 0) + 1
                    i = j + 1
            carry = data[-7:]
            base += len(buf)
    for k in res:
        res[k] = sorted(set(res[k]))
    res["fake_sigs"] = {k: v for k, v in sorted(fake.items()) if k != "ZIP"}
    return res


def read_at(path, pos, n):
    with open(path, "rb") as f:
        f.seek(pos)
        return f.read(n)


def plausible_lfh(path, c):
    """廉价预筛：把随机数据里蒙到的 PK\\x03\\x04 剔除掉，避免穷举验证。"""
    h = read_at(path, c, 30)
    if len(h) < 30 or h[:4] != LFH_SIG:
        return False
    _ver, flags, method = struct.unpack_from("<HHH", h, 4)
    nl, el = struct.unpack_from("<HH", h, 26)
    if method not in (0, 8, 9, 12, 14, 93, 98, 99):
        return False
    if flags & 0x2000:
        return False
    if nl == 0 or nl > 512 or el > 4096:
        return False
    return True


class SplicedIO(io.RawIOBase):
    def __init__(self, path, ranges):
        self._f = open(path, "rb")
        self.ranges = [(int(b), int(l)) for b, l in ranges]
        self.prefix, acc = [], 0
        for _b, l in self.ranges:
            acc += l
            self.prefix.append(acc)
        self.total, self.pos = acc, 0

    def readable(self):
        return True

    def seekable(self):
        return True

    def readinto(self, b):
        if self.pos >= self.total:
            return 0
        lo, hi = 0, len(self.prefix)
        while lo < hi:
            mid = (lo + hi) // 2
            if self.prefix[mid] <= self.pos:
                lo = mid + 1
            else:
                hi = mid
        i = lo
        base = 0 if i == 0 else self.prefix[i - 1]
        off = self.pos - base
        bstart, blen = self.ranges[i]
        want = min(len(b), blen - off)
        self._f.seek(bstart + off)
        data = self._f.read(want)
        b[:len(data)] = data
        self.pos += len(data)
        return len(data)

    def seek(self, off, whence=0):
        t = off if whence == 0 else (self.pos + off if whence == 1 else self.total + off)
        if t < 0:
            raise ValueError("negative seek")
        self.pos = min(t, self.total)
        return self.pos

    def tell(self):
        return self.pos

    def close(self):
        try:
            self._f.close()
        finally:
            super().close()


def build_ranges(path, first_lfh, eocd_end):
    ranges, seg_start, p, hole = [], first_lfh, first_lfh, 0
    with open(path, "rb") as f:
        while p + 4 <= eocd_end:
            f.seek(p)
            hdr = f.read(30)
            if len(hdr) < 30 or hdr[0:4] != LFH_SIG:
                break
            flags = struct.unpack_from("<H", hdr, 6)[0]
            csize = struct.unpack_from("<I", hdr, 18)[0]
            nl = struct.unpack_from("<H", hdr, 26)[0]
            el = struct.unpack_from("<H", hdr, 28)[0]
            if flags & 0x08:                      # data descriptor：走不通，退回整段
                return [(first_lfh, eocd_end - first_lfh)], 0
            data_end = p + 30 + nl + el + csize
            if data_end > eocd_end:
                return [(first_lfh, eocd_end - first_lfh)], 0
            f.seek(data_end)
            sig = f.read(4)
            if sig == CDFH_SIG:
                break
            if sig == LFH_SIG:
                p = data_end
                continue
            found, s = None, data_end + 1          # 洞里向后找下一个 PK
            while s + 4 <= eocd_end:
                f.seek(s)
                blk = f.read(min(64 * 1024, eocd_end - s))
                if len(blk) < 4:
                    break
                jl, jc = blk.find(LFH_SIG), blk.find(CDFH_SIG)
                cands = [x for x in (jl, jc) if x >= 0]
                if cands:
                    found = s + min(cands)
                    break
                s += len(blk) - 3
            if found is None:
                ranges.append((seg_start, data_end - seg_start))
                hole += eocd_end - data_end
                break
            ranges.append((seg_start, data_end - seg_start))
            hole += found - data_end
            seg_start, p = found, found
    if seg_start < eocd_end:
        ranges.append((seg_start, eocd_end - seg_start))
    return ranges, hole


def zip_header_info(zio):
    try:
        zio.seek(0)
        h = zio.read(30)
        if len(h) < 30 or h[:4] != LFH_SIG:
            return {}
        method = struct.unpack_from("<H", h, 8)[0]
        flags = struct.unpack_from("<H", h, 6)[0]
        return {"method": method, "flags": flags,
                "encrypted": bool(flags & 0x1), "aes": method == 99}
    except Exception:
        return {}


def try_read_zip(path, ranges, pwd=None):
    """用虚接逻辑流开 ZIP，逐条 CRC 校验；返回读取结果。"""
    out = {"n": 0, "ok": 0, "names": [], "detail": []}
    try:
        zio = SplicedIO(path, ranges)
    except Exception as e:
        out["detail"].append(f"{type(e).__name__}: {e}")
        return out
    try:
        with open(path, "rb") as f:
            f.seek(ranges[0][0])
            hdr = f.read(30)
        method = struct.unpack_from("<H", hdr, 8)[0]
        flags = struct.unpack_from("<H", hdr, 6)[0]
        out["method"], out["flags"] = method, flags
        out["encrypted"] = bool(flags & 0x1)
        out["aes"] = (method == 99)
        if method == 99:
            import pyzipper
            zf = pyzipper.AESZipFile(zio)
            if pwd:
                zf.setpassword(pwd.encode("utf-8"))
            out["needs_password"] = True
        else:
            zf = zipfile.ZipFile(zio)
            if pwd and (flags & 0x1):
                zf.setpassword(pwd.encode("utf-8"))
            out["needs_password"] = bool(flags & 0x1)
        with zf:
            infos = zf.infolist()
            out["n"] = len(infos)
            out["names"] = [i.filename for i in infos][:8]
            for zi in infos:
                try:
                    data = zf.read(zi)
                    if zlib.crc32(data) & 0xFFFFFFFF == zi.CRC or zi.CRC == 0:
                        out["ok"] += 1
                    else:
                        out["detail"].append(f"{zi.filename}: CRC 不符")
                except Exception as e:
                    out["detail"].append(f"{zi.filename}: {type(e).__name__}: {e}")
    except Exception as e:
        out["detail"].append(f"{type(e).__name__}: {e}")
    finally:
        try:
            zio.close()
        except Exception:
            pass
    return out


def sha256_ranges(path, ranges):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b, l in ranges:
            f.seek(b)
            left = l
            while left > 0:
                d = f.read(min(1 << 20, left))
                if not d:
                    break
                h.update(d)
                left -= len(d)
    return h.hexdigest()


# --------------------------------------------------------------------------
# MKV 附件通道（L8）
# --------------------------------------------------------------------------
def ebml_vint(f, keep_marker):
    b0 = f.read(1)
    if not b0:
        return None, 0
    v = b0[0]
    if v == 0:
        return None, 0
    mask, ln = 0x80, 1
    while not (v & mask):
        mask >>= 1
        ln += 1
    rest = f.read(ln - 1)
    val = v
    for c in rest:
        val = (val << 8) | c
    if keep_marker:
        return val, ln
    return val & ((1 << (7 * ln)) - 1), ln


MKV_SEGMENT, MKV_ATTACHMENTS, MKV_ATTACHEDFILE, MKV_FILEDATA = \
    0x18538067, 0x1941A469, 0x61A7, 0x465C


def mkv_attachments(path):
    """返回 [(name?, offset, size)] —— FileData 载荷的绝对偏移与长度。"""
    out = []
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        def walk(end, depth):
            while f.tell() + 2 < end:
                pos = f.tell()
                eid, _ = ebml_vint(f, True)
                if eid is None:
                    break
                esz, _ = ebml_vint(f, False)
                hdr_end = f.tell()
                if esz is None or hdr_end + esz > end:
                    esz = end - hdr_end
                if eid == MKV_FILEDATA:
                    out.append((hdr_end, esz))
                elif eid in (MKV_SEGMENT, MKV_ATTACHMENTS, MKV_ATTACHEDFILE):
                    if depth < 6:
                        walk(hdr_end + esz, depth + 1)
                f.seek(hdr_end + esz)
        walk(size, 0)
    return out


# --------------------------------------------------------------------------
# 我们的工具：全文件扫描 + 偏移修正 + 虚接
# --------------------------------------------------------------------------
def our_extract(path, pwd=None, manifest_rec=None):
    size = os.path.getsize(path)
    res = {"file": os.path.basename(path), "channels": [], "archives": []}

    # 通道 1：MKV 附件
    if read_at(path, 0, 4) == b"\x1a\x45\xdf\xa3":
        atts = mkv_attachments(path)
        res["channels"].append("mkv")
        for (off, ln) in atts:
            ranges = [(off, ln)]
            r = try_read_zip(path, ranges, pwd)
            r.update({"channel": "mkv-attachment", "start": off, "logical_len": ln,
                      "sha256": sha256_ranges(path, ranges), "ranges": ranges})
            r["solved"] = bool(r["n"] > 0 and r["ok"] == r["n"])
            res["archives"].append(r)

    # 通道 2：ZIP（EOCD → 起点搜索 → 虚接 → CRC）
    sigs = scan_sigs(path)
    res["sigs"] = {k: len(v) for k, v in sigs.items() if k != "fake_sigs"}
    res["fake_sigs"] = sigs["fake_sigs"]
    eocds = sigs["EOCD"]
    res["channels"].append("zip")
    if eocds:
        for j in range(len(eocds) - 1, -1, -1):
            eo = eocds[j]
            lo = 0 if j == 0 else eocds[j - 1] + 22
            hdr = read_at(path, eo, 22)
            if len(hdr) < 22:
                continue
            entries = struct.unpack_from("<H", hdr, 10)[0]
            size_cd = struct.unpack_from("<I", hdr, 12)[0]
            off_cd = struct.unpack_from("<I", hdr, 16)[0]
            clen = struct.unpack_from("<H", hdr, 20)[0]
            eocd_end = eo + 22 + clen
            guess = eo - size_cd - off_cd
            pool = [c for c in sigs["LFH"]
                    if lo <= c <= eo and plausible_lfh(path, c)]
            near = [c for c in pool if abs(c - guess) < 64]
            others = sorted((c for c in pool if c not in set(near)),
                            key=lambda c: abs(c - guess))
            cands = (near + others)[:MAX_CANDS]
            best, hit = None, None
            for c in cands:
                ranges, hole = build_ranges(path, c, eocd_end)
                v = try_read_zip(path, ranges, pwd)
                if best is None or v["ok"] > best[0]:
                    best = (v["ok"], v, c, ranges, hole)
                if v["n"] > 0 and v["ok"] == v["n"]:
                    hit = (c, ranges, hole, v)
                    break
            rec = {"channel": "zip-eocd", "eocd": eo, "offset_from_eof": size - eo,
                   "entries_declared": entries, "eocd_end": eocd_end,
                   "naive_start_guess": guess, "candidates": len(cands)}
            if hit:
                c, ranges, hole, v = hit
                rec.update({"solved": True, "start": c, "delta": c - guess,
                            "ranges": ranges, "skipped": hole, "logical_len":
                            sum(l for _b, l in ranges),
                            "sha256": sha256_ranges(path, ranges), **v})
            else:
                rec.update({"solved": False})
                if best:
                    rec.update({"best_start": best[2], "best_ranges": best[3],
                                "best_ok": best[1]["ok"], "best_n": best[1]["n"],
                                "best_detail": best[1]["detail"][:3]})
            res["archives"].append(rec)
    return res


def match_archives(res, expected_shas):
    """把找到的 archive 与期望的 ZIP sha256 对上。"""
    got = {}
    for a in res["archives"]:
        if a.get("sha256"):
            got.setdefault(a["sha256"], []).append(a)
    hits = {s: (s in got) for s in expected_shas}
    return got, hits


# --------------------------------------------------------------------------
# C. 外部工具
# --------------------------------------------------------------------------
def tools_on(path, tag, pwd, zcopy_dir, members=()):
    out = {}
    cmds = [
        ("unzip_l", ["unzip", "-l", path]),
        ("unzip_t", ["unzip", "-t", path]),
        ("py_zipfile_l", [PY3, "-m", "zipfile", "-l", path]),
        ("7z_l", [SEVENZIP, "l", path]),
        ("7z_l_t#", [SEVENZIP, "l", "-t#", path]),
        ("7z_l_t#a", [SEVENZIP, "l", "-t#a", path]),
        ("file", ["file", "-b", path]),
    ]
    zp = os.path.join(zcopy_dir, os.path.basename(path) + ".zip")
    shutil.copyfile(path, zp)
    cmds.append(("unzip_l_aszip", ["unzip", "-l", zp]))
    for label, cmd in cmds:
        r = run_measured(cmd, timeout=180)
        txt = r["out"] + r["err"]
        seen = sum(1 for m in members if m in txt)
        out[label] = {"rc": r["rc"], "sec": r["sec"], "rss_kb": r["rss_kb"],
                      "members_seen": seen,
                      "tail": tail_lines(txt, 4, 130),
                      "err": r["err"][:500],
                      "full": txt[:900]}
    # 实际解压（不带密码）
    xd = os.path.join(WORK, "x", tag, "nopwd")
    shutil.rmtree(xd, ignore_errors=True)
    os.makedirs(xd, exist_ok=True)
    r = run_measured([SEVENZIP, "x", "-y", "-o" + xd, path], timeout=180)
    n, tot = 0, 0
    for root, _d, files in os.walk(xd):
        for fn in files:
            n += 1
            tot += os.path.getsize(os.path.join(root, fn))
    out["7z_x_nopwd"] = {"rc": r["rc"], "files": n, "bytes": tot, "rss_kb": r["rss_kb"],
                         "tail": tail_lines(r["out"] + r["err"], 3, 130)}
    # 实际解压（带密码）
    xd2 = os.path.join(WORK, "x", tag, "pwd")
    shutil.rmtree(xd2, ignore_errors=True)
    os.makedirs(xd2, exist_ok=True)
    r2 = run_measured([SEVENZIP, "x", "-y", "-p" + pwd, "-o" + xd2, path], timeout=180)
    n2, tot2 = 0, 0
    for root, _d, files in os.walk(xd2):
        for fn in files:
            n2 += 1
            tot2 += os.path.getsize(os.path.join(root, fn))
    out["7z_x_pwd"] = {"rc": r2["rc"], "files": n2, "bytes": tot2, "rss_kb": r2["rss_kb"],
                       "tail": tail_lines(r2["out"] + r2["err"], 3, 130)}
    # 扫描模式实解：7z x -t#（把整份文件按签名切成「垃圾/内嵌归档」段再逐个解）
    xd3 = os.path.join(WORK, "x", tag, "scan")
    shutil.rmtree(xd3, ignore_errors=True)
    os.makedirs(xd3, exist_ok=True)
    r4 = run_measured([SEVENZIP, "x", "-t#", "-y", "-o" + xd3, path], timeout=300)
    n3, tot3 = 0, 0
    for root, _d, files in os.walk(xd3):
        for fn in files:
            n3 += 1
            tot3 += os.path.getsize(os.path.join(root, fn))
    out["7z_x_scan"] = {"rc": r4["rc"], "files": n3, "bytes": tot3, "rss_kb": r4["rss_kb"],
                        "tail": tail_lines(r4["out"] + r4["err"], 3, 130)}
    # 解出来的内容与原始成员是否一致（对 plain 版）
    out["_extracted_bytes"] = max(tot, tot2)
    # MKV 附件工具
    if path.lower().endswith(".mkv"):
        for label, spec in (("ffmpeg_dump_attachment", "t:0"), ("ffmpeg_dump_attachment_t", "t")):
            d = os.path.join(WORK, "attach", f"{tag}_{label}.bin")
            os.makedirs(os.path.dirname(d), exist_ok=True)
            if os.path.exists(d):
                os.remove(d)
            r3 = run_measured([FFMPEG, "-v", "error", "-dump_attachment:" + spec, d,
                               "-i", path, "-f", "null", "-"], timeout=120)
            out[label] = {"rc": r3["rc"],
                          "wrote": os.path.getsize(d) if os.path.exists(d) else 0,
                          "tail": tail_lines(r3["err"], 2, 130)}
        p = run_measured([FFPROBE, "-v", "error", "-show_streams", "-of",
                          "default=nw=1", path], timeout=120)
        out["ffprobe_streams"] = {"rc": p["rc"], "tail": tail_lines(p["out"], 6, 130)}
    return out


# --------------------------------------------------------------------------
# 对照实验：修正 moov/stco 到底救不救得回视频
# --------------------------------------------------------------------------
def fix_experiments(manifest):
    d = os.path.join(WORK, "fixexp")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d, exist_ok=True)
    out = {}
    cover = os.path.join(M.ASSETS, manifest["cover"])
    fast = os.path.join(M.ASSETS, "cover5_fast.mp4")
    z = open(os.path.join(M.ASSETS, manifest["zip_plain"]["path"]), "rb").read()

    atoms = M.top_atoms(cover)
    mdat = [a for a in atoms if a[2] == b"mdat"][0]
    ftyp = [a for a in atoms if a[2] == b"ftyp"][0]
    mo = M.stco_entries(cover)
    vals = [v for (_b, _i, _w, v) in M.entry_values(cover, mo)]
    mid = sorted(vals)[len(vals) // 2]

    def emit(name, comp):
        p = os.path.join(d, name)
        comp.render(p)
        return p

    # (1) L4 不做任何修正
    c = M.Composer(cover)
    c.ins(mid, z)
    out["L4_无修正"] = (emit("l4_raw.mp4", c), None)
    # (2) L4 只改 mdat size
    c = M.Composer(cover)
    c.ins(mid, z)
    c.patch(*M.patch_box_size(cover, mdat[0], len(z)))
    out["L4_只改mdat_size"] = (emit("l4_mdatonly.mp4", c), None)
    # (3) L4 只改 stco
    moff, msize, mdata, ent = mo
    c = M.Composer(cover)
    c.ins(mid, z)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(mid, len(z))]))
    out["L4_只改stco"] = (emit("l4_stcoonly.mp4", c), None)
    # (4) L4 两者都改（= 正式 L4）
    c = M.Composer(cover)
    c.ins(mid, z)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(mid, len(z))]))
    c.patch(*M.patch_box_size(cover, mdat[0], len(z)))
    out["L4_全修正(=正式L4)"] = (emit("l4_full.mp4", c), None)

    # (5) L3 隐写者原始 bug 版：噪声逐原子插入、stco 只补偿 free 长度
    import random as _r
    rnd = _r.Random(1)
    free = (8 + len(z)).to_bytes(4, "big") + b"free" + z
    ins_at = ftyp[0] + ftyp[1]
    rest_start = ins_at
    segments = []
    pos = rest_start
    total = os.path.getsize(cover)
    with open(cover, "rb") as f:
        while pos + 8 <= total:
            f.seek(pos)
            h = f.read(16)
            size = int.from_bytes(h[:4], "big")
            if size == 1:
                size = int.from_bytes(h[8:16], "big")
            elif size == 0:
                size = total - pos
            if size < 8 or pos + size > total:
                break
            segments.append((pos, size))
            pos += size
    c = M.Composer(cover)
    c.ins(ins_at, free)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(ins_at, len(free))]))
    for (p, s) in segments:
        c.ins(p + s, M.noise_block(rnd))
    out["L3_隐写者原版(bug)"] = (emit("l3_bug.mp4", c), None)

    # (7a) L4 插在「非 chunk 边界」+ 全修正 —— 证明「必须插在 chunk 边界」
    inner = mdat[0] + 8 + 1234
    c = M.Composer(cover)
    c.ins(inner, z)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(inner, len(z))]))
    c.patch(*M.patch_box_size(cover, mdat[0], len(z)))
    out["L4_非chunk边界_全修正"] = (emit("l4_notchunk.mp4", c), None)

    # (6) L3 修正版但噪声在末尾（= 正式 L3）
    c = M.Composer(cover)
    c.ins(ins_at, free)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(ins_at, len(free))]))
    c.ins(os.path.getsize(cover), M.noise_block(_r.Random(2)))
    out["L3_修正版(噪声在末尾)"] = (emit("l3_fixed.mp4", c), None)

    # (7) L6 加 stco 补偿（外层垃圾把整条视频推后）
    junk = M.JUNK_MIB * 1024 * 1024
    p = os.path.join(d, "l6_stco.mp4")
    moff2, msize2, mdata2, ent2 = mo
    with open(p, "wb") as fo:
        M.write_junk(fo, junk, 11)
        foo = M.Composer(cover)
        foo.patch(moff2, msize2, M.patch_moov(mdata2, mo, [(0, junk)]))
        tmp = p + ".mid"
        foo.render(tmp)
        with open(tmp, "rb") as fi:
            for b in iter(lambda: fi.read(1 << 20), b""):
                fo.write(b)
        os.remove(tmp)
        fo.write(z)
        M.write_junk(fo, junk, 22)
    out["L6_加stco补偿"] = (p, None)

    # (7b) L6 加 stco 补偿 + 强制 -f mp4（判断「容器结构没坏、只是识别不到」还是真坏）
    rc, err = video_check_quick(p)
    r2 = run_measured([FFMPEG, "-v", "error", "-f", "mp4", "-i", p, "-f", "null", "-"],
                      timeout=300)
    out["L6_加stco补偿_强制-f-mp4"] = (p, {"ffmpeg_rc_forced_f_mp4": r2["rc"],
                                        "ffmpeg_err_forced_f_mp4": tail_lines(r2["err"], 2)})

    # (7c) L6 原样（无 stco 补偿）+ 强制 -f mp4
    p2 = os.path.join(d, "l6_raw.mp4")
    with open(p2, "wb") as fo:
        M.write_junk(fo, junk, 11)
        with open(cover, "rb") as fi:
            for b in iter(lambda: fi.read(1 << 20), b""):
                fo.write(b)
        fo.write(z)
        M.write_junk(fo, junk, 22)
    r3 = run_measured([FFMPEG, "-v", "error", "-f", "mp4", "-i", p2, "-f", "null", "-"],
                      timeout=300)
    out["L6_原样_强制-f-mp4"] = (p2, {"ffmpeg_rc_forced_f_mp4": r3["rc"],
                                    "ffmpeg_err_forced_f_mp4": tail_lines(r3["err"], 2)})

    # (7d) L5 无修正 / 全修正
    pts = M._chunk_points(cover, 3, mdat)
    parts = M.split_zip_by_members(z, len(pts))
    c = M.Composer(cover)
    for at, part in zip(pts, parts):
        c.ins(at, part)
    out["L5_无修正"] = (emit("l5_raw.mp4", c), None)
    acc, ins = 0, []
    for at, part in zip(pts, parts):
        ins.append((at, len(part)))
    c = M.Composer(cover)
    for at, part in zip(pts, parts):
        c.ins(at, part)
    c.patch(moff, msize, M.patch_moov(mdata, mo, [(at, a) for (at, a) in ins]))
    c.patch(*M.patch_box_size(cover, mdat[0], len(z)))
    out["L5_全修正(=正式L5)"] = (emit("l5_full.mp4", c), None)

    # (7e) faststart 外壳下的 L4（mdat 在 moov 之后）
    if os.path.exists(fast):
        a2 = M.top_atoms(fast)
        md2 = [x for x in a2 if x[2] == b"mdat"][0]
        mo2b = M.stco_entries(fast)
        vals2 = [v for (_b, _i, _w, v) in M.entry_values(fast, mo2b)]
        pts2 = M._chunk_points(fast, 1, md2)
        at3 = pts2[len(pts2) // 2]
        c = M.Composer(fast)
        c.ins(at3, z)
        c.patch(mo2b[0], mo2b[1], M.patch_moov(mo2b[2], mo2b, [(at3, len(z))]))
        c.patch(*M.patch_box_size(fast, md2[0], len(z)))
        out["L4_faststart外壳_全修正"] = (emit("l4_fast_full.mp4", c), None)

    # (8) faststart 外壳下的 L3
    if os.path.exists(fast):
        a = M.top_atoms(fast)
        ft2 = [x for x in a if x[2] == b"ftyp"][0]
        mo2 = M.stco_entries(fast)
        free2 = (8 + len(z)).to_bytes(4, "big") + b"free" + z
        at2 = ft2[0] + ft2[1]
        c = M.Composer(fast)
        c.ins(at2, free2)
        c.patch(mo2[0], mo2[1], M.patch_moov(mo2[2], mo2, [(at2, len(free2))]))
        out["L3_faststart外壳"] = (emit("l3_fast.mp4", c), None)

    res = {}
    for k, (p, extra) in out.items():
        rc, err = video_check_quick(p)
        rec = {"file": os.path.relpath(p, HERE), "ffmpeg_rc": rc, "ffmpeg_err": err,
               "size": os.path.getsize(p)}
        if extra:
            rec.update(extra)
        res[k] = rec
    return res


# --------------------------------------------------------------------------
def eocd_window_experiment(man):
    """ZIP 的 EOCD 距文件尾多远时，普通解压器就找不到了？（决定 L1/L2/L3 的可用边界）"""
    zpath = os.path.join(M.ASSETS, man["zip_plain"]["path"])
    z = open(zpath, "rb").read()
    d = os.path.join(WORK, "eocdwin")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d, exist_ok=True)
    rows = []
    for junk_n in (0, 1024, 16 * 1024, 32 * 1024, 40 * 1024, 48 * 1024,
                   56 * 1024, 60 * 1024, 64 * 1024, 96 * 1024, 128 * 1024, 1 << 20):
        p = os.path.join(d, f"zip_plus_{junk_n}.bin")
        with open(p, "wb") as f:
            f.write(z)
            f.write(b"\x00" * junk_n)
        row = {"junk_after_zip": junk_n}
        for label, cmd in (("unzip_l", ["unzip", "-l", p]),
                           ("py_zipfile_l", [PY3, "-m", "zipfile", "-l", p]),
                           ("7z_l", [SEVENZIP, "l", p])):
            r = run_measured(cmd, timeout=60)
            txt = r["out"] + r["err"]
            row[label] = {"rc": r["rc"],
                          "members_seen": sum(1 for m in man["members"] if m in txt)}
        rows.append(row)
    return rows


def main():
    reexec_with_pyzipper()
    with open(MANIFEST, encoding="utf-8") as f:
        man = json.load(f)
    only = sys.argv[1:] or None
    os.makedirs(WORK, exist_ok=True)
    zdir = os.path.join(WORK, "aszip")
    os.makedirs(zdir, exist_ok=True)
    pwd = man["password"]

    exp = {}
    for key, lay in man["layouts"].items():
        exp[key] = [lay["zip_sha256"]] + ([lay["extra_zip_sha256"]]
                                          if lay["layout"] == "L7" else [])

    results = {"password": pwd, "samples": {}}
    order = sorted(man["layouts"], key=lambda k: (k.split("_")[0], k))
    if only:
        order = [k for k in order if k.split("_")[0] in only]

    peak_self = 0
    for key in order:
        lay = man["layouts"][key]
        path = os.path.join(DATA, lay["file"])
        tag = key
        print(f"\n{'='*84}\n### {key}  {lay['size']:,} B  —— {lay['structure']}")
        rec = {"layout": lay["layout"], "variant": lay["variant"],
               "file": lay["file"], "size": lay["size"], "structure": lay["structure"]}

        rec["video"] = video_check(path)
        print(f"  视频: ffmpeg rc={rec['video']['ffmpeg_rc']}  "
              f"ffprobe streams={rec['video']['ffprobe_streams']}  "
              f"err={rec['video']['ffmpeg_err']}")

        _t_our = time.time()
        our = our_extract(path, pwd)
        got, hits = match_archives(our, exp[key])
        rec["our"] = our
        rec["our_expect_hits"] = hits
        rec["our_sec"] = round(time.time() - _t_our, 3)
        for a in our["archives"]:
            if a.get("solved"):
                print(f"  [我们] ✅ {a['channel']} start={a['start']} Δ={a.get('delta')} "
                      f"虚接段={len(a.get('ranges', []))} 跳过={a.get('skipped')} "
                      f"→ 成员 {a.get('ok')}/{a.get('n')} AES={a.get('aes')} "
                      f"sha256={a.get('sha256', '')[:12]}")
            else:
                print(f"  [我们] ❌ {a['channel']} eocd={a.get('eocd')} "
                      f"best={a.get('best_ok')}/{a.get('best_n')} {a.get('best_detail', '')[:2]}")
        print(f"  [我们] 期望命中 {hits}  假特征码 {our.get('fake_sigs')}")

        rec["tools"] = tools_on(path, tag, pwd, zdir, man.get("members", ()))
        for k in ("unzip_l", "unzip_l_aszip", "unzip_t", "py_zipfile_l", "7z_l", "7z_l_t#",
                  "7z_l_t#a", "7z_x_nopwd", "7z_x_pwd", "7z_x_scan", "file"):
            t = rec["tools"][k]
            extra = f" files={t.get('files')}" if "files" in t else ""
            seen = f" seen={t['members_seen']}/{len(man.get('members', ()))}" \
                if "members_seen" in t else ""
            print(f"  [工具] {k:12s} rc={t['rc']:<3} rss={t.get('rss_kb', 0):>6}KB{extra}{seen} "
                  f"| {t['tail'][-1] if t.get('tail') else ''}")
        for k in ("ffmpeg_dump_attachment", "ffmpeg_dump_attachment_t"):
            if k in rec["tools"]:
                t = rec["tools"][k]
                print(f"  [工具] {k:24s} rc={t['rc']} wrote={t['wrote']} {t['tail']}")

        results["samples"][key] = rec
        peak_self = max(peak_self, resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)

    print(f"\n{'='*84}\n### 对照实验：修正 moov/stco 能不能救回视频")
    fx = fix_experiments(man)
    for k, v in fx.items():
        extra = f"  [强制-f mp4 rc={v.get('ffmpeg_rc_forced_f_mp4')}]" \
            if "ffmpeg_rc_forced_f_mp4" in v else ""
        print(f"  {k:26s} ffmpeg_rc={v['ffmpeg_rc']:<4} {v['ffmpeg_err']}{extra}")
    results["fix_experiments"] = fx

    print(f"\n{'='*84}\n### EOCD 距文件尾多远就没人找得到（ZIP 后面加 N 字节垃圾）")
    rows = eocd_window_experiment(man)
    for r in rows:
        print(f"  尾部垃圾 {r['junk_after_zip']:>8,} B | unzip rc={r['unzip_l']['rc']:<2} "
              f"seen={r['unzip_l']['members_seen']}/4 | zipfile rc={r['py_zipfile_l']['rc']:<2} "
              f"seen={r['py_zipfile_l']['members_seen']}/4 | 7z rc={r['7z_l']['rc']:<2} "
              f"seen={r['7z_l']['members_seen']}/4")
    results["eocd_window"] = rows

    results["harness_peak_rss_kb"] = peak_self
    print(f"\n[harness] 本进程峰值 RSS = {peak_self/1024:.1f} MiB")
    outp = os.path.join(HERE, "pack_results.json")
    with open(outp, "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print(f"[harness] 结果 → {outp}")


if __name__ == "__main__":
    main()
