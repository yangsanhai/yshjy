#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""extreme_tests.py —— 对 tests/data/E* 变态场景样本实测「我们的方案」与常规工具。

「我们的方案」在 Python 侧复刻（与 spike/src/bin/zip_extreme.rs 同一套算法）：
  1) 全文件流式签名扫描（LFH / CDFH / EOCD）
  2) 偏移修正：按 EOCD 的 size_cd/offset_cd 推起点，再用真实 LFH 校正
  3) 虚接（SplicedIO：把不连续区间拼成一个 Read+Seek 逻辑流）
  4) 无目录表傅底（rescue）：只按 local header 顺序扫描 + CRC32 校验
  5) 未知切点：CRC 引导搜索（E3b 专用）

外部工具对照：7z l / 7z l -t# / 7z t / unzip -l / unzip -t / python3 -m zipfile
内存：每个外部命令用 fork+wait4 取 ru_maxrss；本进程用 getrusage(RUSAGE_SELF)。

用法：python3 tests/extreme_tests.py            # 跑全部
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
DATA = os.path.join(HERE, "data")
WORK = os.path.join(HERE, ".work", "extreme")
MANIFEST = os.path.join(HERE, "extreme_manifest.json")

FFMPEG = "/usr/bin/ffmpeg"
SEVENZIP = "/usr/bin/7z"

LFH_SIG = b"PK\x03\x04"
CDFH_SIG = b"PK\x01\x02"
EOCD_SIG = b"PK\x05\x06"


# --------------------------------------------------------------------------
# 内存测量（fork + wait4 → 精确到子进程峰值 RSS）
# --------------------------------------------------------------------------
def run_measured(cmd, timeout=180):
    t0 = time.time()
    outf = tempfile.TemporaryFile()
    errf = tempfile.TemporaryFile()
    pid = os.fork()
    if pid == 0:
        try:
            os.dup2(outf.fileno(), 1)
            os.dup2(errf.fileno(), 2)
            os.execvp(cmd[0], cmd)
        except Exception:
            os._exit(127)
    status = None
    ru = None
    deadline = t0 + timeout
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
    outf.seek(0)
    out = outf.read().decode("utf-8", "replace")
    errf.seek(0)
    err = errf.read().decode("utf-8", "replace")
    outf.close()
    errf.close()
    rc = os.waitstatus_to_exitcode(status) if isinstance(status, int) and status >= 0 else status
    rss_kb = ru.ru_maxrss if ru else 0
    return {"rc": rc, "out": out, "err": err, "sec": round(el, 3), "rss_kb": rss_kb}


# --------------------------------------------------------------------------
# 流式签名扫描
# --------------------------------------------------------------------------
def scan_sigs(path, chunk=1 << 20):
    pats = [(LFH_SIG, "LFH"), (CDFH_SIG, "CDFH"), (EOCD_SIG, "EOCD")]
    res = {"LFH": [], "CDFH": [], "EOCD": []}
    with open(path, "rb") as f:
        base = 0
        carry = b""
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
            carry = data[-3:]
            base += len(buf)
    for k in res:
        res[k] = sorted(set(res[k]))
    return res


def read_at(path, pos, n):
    with open(path, "rb") as f:
        f.seek(pos)
        return f.read(n)


# --------------------------------------------------------------------------
# 虚接：把同文件多个区间当逻辑流
# --------------------------------------------------------------------------
class SplicedIO(io.RawIOBase):
    def __init__(self, path, ranges):
        self._f = open(path, "rb")
        self.ranges = [(int(b), int(l)) for b, l in ranges]
        self.prefix = []
        acc = 0
        for _b, l in self.ranges:
            acc += l
            self.prefix.append(acc)
        self.total = acc
        self.pos = 0

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
        b[: len(data)] = data
        self.pos += len(data)
        return len(data)

    def seek(self, off, whence=0):
        if whence == 0:
            t = off
        elif whence == 1:
            t = self.pos + off
        elif whence == 2:
            t = self.total + off
        else:
            raise ValueError("bad whence")
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
    """从 first_lfh 走到 CD 起点，挖掉成员之间的洞。返回 (ranges, hole, walkable)。"""
    ranges = []
    seg_start = first_lfh
    p = first_lfh
    hole = 0
    walkable = True
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
            if flags & 0x08:
                return [(first_lfh, eocd_end - first_lfh)], 0, False
            data_end = p + 30 + nl + el + csize
            if data_end > eocd_end:
                return [(first_lfh, eocd_end - first_lfh)], 0, False
            f.seek(data_end)
            sig = f.read(4)
            if sig == CDFH_SIG:
                break
            if sig == LFH_SIG:
                p = data_end
                continue
            # 洞：向后找下一个 PK 签名（分块）
            found = None
            s = data_end + 1
            while s + 4 <= eocd_end:
                f.seek(s)
                blk = f.read(min(64 * 1024, eocd_end - s))
                if len(blk) < 4:
                    break
                jl = blk.find(LFH_SIG)
                jc = blk.find(CDFH_SIG)
                cands = [x for x in (jl, jc) if x >= 0]
                if cands:
                    found = s + min(cands)
                    break
                s += len(blk) - 3
            if found is None:
                ranges.append((seg_start, data_end - seg_start))
                hole += eocd_end - data_end
                seg_start = eocd_end
                break
            ranges.append((seg_start, data_end - seg_start))
            hole += found - data_end
            seg_start = found
            p = found
    if seg_start < eocd_end:
        ranges.append((seg_start, eocd_end - seg_start))
    return ranges, hole, walkable


def open_and_verify(path, ranges):
    """虚接后交给标准 zipfile 解，逐条 CRC 校验。"""
    try:
        zio = SplicedIO(path, ranges)
        with zipfile.ZipFile(zio) as z:
            infos = z.infolist()
            ok = 0
            detail = []
            for zi in infos:
                try:
                    data = z.read(zi)
                    if zlib.crc32(data) & 0xFFFFFFFF == zi.CRC:
                        ok += 1
                    else:
                        detail.append(f"{zi.filename}: CRC 不符")
                except Exception as e:
                    detail.append(f"{zi.filename}: {type(e).__name__}: {e}")
            return {"n": len(infos), "ok": ok, "detail": detail}
    except Exception as e:
        return {"n": 0, "ok": 0, "detail": [f"{type(e).__name__}: {e}"]}


def our_scheme_locate(path, sigs):
    """对每个 EOCD 候选做起点搜索 + 虚接 + 校验。"""
    eocds = sigs["EOCD"]
    lfhs = sigs["LFH"]
    out = []
    if not eocds:
        return {"eocds": 0, "archives": [], "note": "全文件无 EOCD"}
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
        cands = [c for c in lfhs if lo <= c <= eo]
        rec = {"eocd": eo, "entries_declared": entries, "size_cd": size_cd,
               "offset_cd": off_cd, "eocd_from_eof": os.path.getsize(path) - eo,
               "naive_start_guess": guess, "candidates": len(cands)}
        hit = None
        best = None
        for c in cands:
            ranges, hole, walkable = build_ranges(path, c, eocd_end)
            v = open_and_verify(path, ranges)
            if best is None or v["ok"] > best[0]:
                best = (v["ok"], v, c, ranges, hole)
            if v["n"] > 0 and v["ok"] == v["n"]:
                hit = (c, ranges, hole, v)
                break
        if hit:
            c, ranges, hole, v = hit
            rec.update({"solved": True, "start_lfh": c, "delta": c - guess,
                        "ranges": ranges, "skipped": hole,
                        "entries": v["n"], "crc_ok": v["ok"]})
        else:
            rec.update({"solved": False, "candidates_tried": len(cands)})
            if best:
                rec.update({"best_start": best[2], "best_entries": best[1]["n"],
                            "best_crc_ok": best[1]["ok"], "best_ranges": best[3],
                            "best_skipped": best[4], "best_detail": best[1]["detail"][:6]})
        out.append(rec)
    return {"eocds": len(eocds), "archives": out}


# --------------------------------------------------------------------------
# 救援模式：只按 local header 扫描（无目录表）
# --------------------------------------------------------------------------
def rescue_local_headers(path, outdir):
    os.makedirs(outdir, exist_ok=True)
    size = os.path.getsize(path)
    # 先流式找所有 LFH
    sigs = scan_sigs(path)["LFH"]
    res = {"found_lfh": len(sigs), "recovered": [], "failed": [], "bytes": 0, "failed_bytes": 0}
    with open(path, "rb") as f:
        for off in sigs:
            f.seek(off)
            hdr = f.read(30)
            if len(hdr) < 30 or hdr[0:4] != LFH_SIG:
                continue
            (ver, flags, method, mt, md, crc, csz, usz, nl, el) = struct.unpack_from("<HHHHHIIIHH", hdr, 4)
            name = f.read(nl).decode("utf-8", "replace")
            if flags & 0x08:
                res["failed"].append(f"{name}: data descriptor(bit3)")
                continue
            if method not in (0, 8):
                res["failed"].append(f"{name}: method={method}")
                continue
            f.seek(off + 30 + nl + el)
            raw = f.read(csz)
            if len(raw) < csz:
                res["failed"].append(f"{name}: 数据截断({len(raw)}/{csz})")
                continue
            try:
                data = raw if method == 0 else zlib.decompressobj(-15).decompress(raw)
            except Exception as e:
                res["failed"].append(f"{name}: 解压失败 {type(e).__name__}")
                continue
            if zlib.crc32(data) & 0xFFFFFFFF != crc:
                res["failed"].append(f"{name}: CRC 不符（写出 {len(data)}B 但内容不可信）")
                res["failed_bytes"] += len(data)
                continue
            dst = os.path.join(outdir, name.replace("/", "__"))
            with open(dst, "wb") as o:
                o.write(data)
            res["recovered"].append({"name": name, "bytes": len(data)})
            res["bytes"] += len(data)
    return res


# --------------------------------------------------------------------------
# 未知切点：解析 CD → 按名字定位每个成员的物理 LFH → shift ∈ {0, m}
#   → 被切断成员的数据区间里用 CRC 引导搜索精确切点 a
# --------------------------------------------------------------------------
def parse_cd(path, cd_start, count):
    """解析中心目录，返回 [{name,lho,csize,usize,crc,method,nl,el}]。"""
    ent = []
    with open(path, "rb") as f:
        f.seek(cd_start)
        for _ in range(count):
            hdr = f.read(46)
            if len(hdr) < 46 or hdr[0:4] != CDFH_SIG:
                break
            (ver, vneed, flags, method, mt, md, crc, csize, usize, nl, el, cl,
             dsk, iattr, eattr, lho) = struct.unpack_from("<HHHHHHIIIHHHHHII", hdr, 4)
            name = f.read(nl).decode("utf-8", "replace")
            f.read(el + cl)
            ent.append({"name": name, "lho": lho, "csize": csize, "usize": usize,
                        "crc": crc, "method": method, "nl": nl, "el": el})
    return ent


def find_lfh_by_name(path, sigs, name):
    """在扫描到的 LFH 里找到名字匹配的那个，返回 (pos, nl, el, method, csize, crc, flags)。"""
    nb = name.encode("utf-8")
    for pos in sigs["LFH"]:
        hdr = read_at(path, pos, 30)
        if len(hdr) < 30 or hdr[0:4] != LFH_SIG:
            continue
        nl = struct.unpack_from("<H", hdr, 26)[0]
        el = struct.unpack_from("<H", hdr, 28)[0]
        nm = read_at(path, pos + 30, nl)
        if nm == nb:
            flags = struct.unpack_from("<H", hdr, 6)[0]
            method = struct.unpack_from("<H", hdr, 8)[0]
            crc = struct.unpack_from("<I", hdr, 14)[0]
            csize = struct.unpack_from("<I", hdr, 18)[0]
            return pos, nl, el, method, csize, crc, flags
    return None


def resolve_insert(path, sigs, eo, size_cd, off_cd, clen, zip_start):
    """把「ZIP 中间被插了一段长度 m 的数据」建模出来，定位切点 a 并虚接校验。"""
    L = off_cd + size_cd + 22 + clen
    eocd_end = eo + 22 + clen
    span = eocd_end - zip_start
    m = span - L
    if m <= 0:
        return {"ok": False, "reason": f"跨度({span})<=逻辑长度({L})，非插入场景"}
    cd_start = eo - size_cd
    ent = parse_cd(path, cd_start, len(sigs["LFH"]))
    if not ent:
        return {"ok": False, "reason": "CD 解析失败"}
    shifts = []
    for e in ent:
        got = find_lfh_by_name(path, sigs, e["name"])
        if got is None:
            shifts.append(None)
            continue
        ph, nl, el, method, csize, crc, flags = got
        shifts.append({"ent": e, "phys": ph, "nl": nl, "el": el,
                       "shift": ph - zip_start - e["lho"]})
    # 前缀 = shift 0 的连续成员；后缀 = shift m
    k = None
    for i, s in enumerate(shifts):
        if s and abs(s["shift"]) < 1:
            k = i
        else:
            break
    if k is None or k >= len(shifts) - 1:
        return {"ok": False, "reason": "找不到「前缀 shift=0 / 后缀 shift=m」的切分",
                "m": m, "shifts": [None if s is None else s["shift"] for s in shifts]}
    sk = shifts[k]
    dstart = sk["phys"] + 30 + sk["nl"] + sk["el"]
    upper = zip_start + shifts[k + 1]["ent"]["lho"]
    found = None
    tries = 0
    for a in range(dstart, upper + 1):
        tries += 1
        pre = read_at(path, dstart, a - dstart)
        post = read_at(path, a + m, sk["ent"]["csize"] - (a - dstart))
        raw = pre + post
        if len(raw) != sk["ent"]["csize"]:
            continue
        try:
            data = raw if sk["ent"]["method"] == 0 else zlib.decompressobj(-15).decompress(raw)
        except Exception:
            continue
        if zlib.crc32(data) & 0xFFFFFFFF == sk["ent"]["crc"]:
            found = a
            break
    if found is None:
        return {"ok": False, "reason": f"CRC 引导搜索 {tries} 次未命中切点", "m": m, "window": [dstart, upper]}
    a = found
    ranges = [(zip_start, a - zip_start), (a + m, eocd_end - (a + m))]
    v = open_and_verify(path, ranges)
    return {"ok": (v["n"] > 0 and v["ok"] == v["n"]), "m": m, "cut_a": a,
            "interrupted_member": sk["ent"]["name"], "search_tries": tries,
            "ranges": ranges, "entries": v["n"], "crc_ok": v["ok"], "detail": v["detail"][:6]}


# --------------------------------------------------------------------------
# 外部工具
# --------------------------------------------------------------------------
def tools_on(path, tag):
    out = {}
    for label, cmd in [
        ("7z_l", [SEVENZIP, "l", path]),
        ("7z_l_t#", [SEVENZIP, "l", "-t#", path]),
        ("7z_t", [SEVENZIP, "t", path]),
        ("unzip_l", ["unzip", "-l", path]),
        ("unzip_t", ["unzip", "-t", path]),
        ("zipfile_l", ["python3", "-m", "zipfile", "-l", path]),
        ("zipfile_t", ["python3", "-m", "zipfile", "-t", path]),
    ]:
        r = run_measured(cmd, timeout=120)
        tail = [l for l in (r["out"] + r["err"]).strip().splitlines() if l.strip()]
        out[label] = {"rc": r["rc"], "rss_kb": r["rss_kb"], "sec": r["sec"],
                      "tail": tail[-4:] if tail else []}
    # 实解一遍
    xdir = os.path.join(WORK, "x", tag)
    shutil.rmtree(xdir, ignore_errors=True)
    os.makedirs(xdir, exist_ok=True)
    r = run_measured([SEVENZIP, "x", "-y", "-o" + xdir, path], timeout=180)
    n = tot = 0
    for root, _d, files in os.walk(xdir):
        for fn in files:
            n += 1
            tot += os.path.getsize(os.path.join(root, fn))
    out["7z_x"] = {"rc": r["rc"], "rss_kb": r["rss_kb"], "files": n, "bytes": tot,
                   "tail": [l for l in (r["out"] + r["err"]).strip().splitlines() if l.strip()][-2:]}
    return out


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------
def main():
    with open(MANIFEST, encoding="utf-8") as f:
        manifest = json.load(f)
    os.makedirs(WORK, exist_ok=True)
    only = sys.argv[1:] or None
    files = sorted(f for f in os.listdir(DATA) if f.startswith("E"))
    if only:
        files = [f for f in files if any(o in f for o in only)]

    exp_bytes = sum(m["size"] for m in manifest["members1"])
    exp_names = [m["name"] for m in manifest["members1"]]
    expected = {"zip1_members": len(exp_names), "zip1_bytes": exp_bytes, "names": exp_names}

    results = {"expected": expected, "samples": {}}

    for fn in files:
        path = os.path.join(DATA, fn)
        size = os.path.getsize(path)
        print(f"\n{'='*80}\n### {fn}  ({size:,} B)")
        sigs = scan_sigs(path)
        print(f"  扫描: LFH={len(sigs['LFH'])} CDFH={len(sigs['CDFH'])} EOCD={len(sigs['EOCD'])}")
        our = our_scheme_locate(path, sigs)
        for a in our["archives"]:
            if a.get("solved"):
                print(f"  [我们的方案] ✅ EOCD@{a['eocd']} 起点LFH@{a['start_lfh']}"
                      f"(Δ={a['delta']}) 虚接段={a['ranges']} 跳过={a['skipped']}"
                      f" → entries={a['entries']} CRC {a['crc_ok']}/{a['entries']}")
            else:
                print(f"  [我们的方案] ❌ EOCD@{a['eocd']} 候选{a.get('candidates_tried')}个全失败"
                      f" best={a.get('best_crc_ok')}/{a.get('best_entries')} 跳过={a.get('best_skipped')}")

        # 救援
        rdir = os.path.join(WORK, "rescue", fn)
        shutil.rmtree(rdir, ignore_errors=True)
        rescue = rescue_local_headers(path, rdir)
        rec_bytes = rescue["bytes"]
        pct = 100.0 * rec_bytes / exp_bytes if exp_bytes else 0
        print(f"  [救援模式] 找到 {rescue['found_lfh']} 个 LFH → 恢复 {len(rescue['recovered'])}/{len(exp_names)} 成员"
              f" = {rec_bytes:,}/{exp_bytes:,} B ({pct:.1f}%)  失败项={rescue['failed']}")

        # 未知切点（插入建模 + CRC 引导搜索）
        ins = []
        eocds = sigs["EOCD"]
        for j in range(len(eocds) - 1, -1, -1):
            eo2 = eocds[j]
            lo2 = 0 if j == 0 else eocds[j - 1] + 22
            h2 = read_at(path, eo2, 22)
            if len(h2) < 22:
                continue
            size_cd2 = struct.unpack_from("<I", h2, 12)[0]
            off_cd2 = struct.unpack_from("<I", h2, 16)[0]
            clen2 = struct.unpack_from("<H", h2, 20)[0]
            cds = [c for c in sigs["LFH"] if lo2 <= c <= eo2]
            if not cds:
                continue
            r = resolve_insert(path, sigs, eo2, size_cd2, off_cd2, clen2, cds[0])
            r["eocd"] = eo2
            ins.append(r)
            if r.get("ok"):
                print(f"  [插入建模] ✅ 切点 a={r['cut_a']} m={r['m']} 被切断成员={r['interrupted_member']}"
                      f" 搜索{r['search_tries']}次 → entries={r['entries']} CRC {r['crc_ok']}/{r['entries']}")
            else:
                print(f"  [插入建模] ❌ {r.get('reason')}")

        tools = tools_on(path, fn)
        for k in ("7z_l", "7z_l_t#", "7z_t", "unzip_l", "unzip_t", "zipfile_l", "zipfile_t", "7z_x"):
            t = tools[k]
            extra = f" files={t.get('files')}" if "files" in t else ""
            print(f"  [工具] {k:11s} rc={t['rc']:<3} rss={t['rss_kb']:>6}KB{extra}  {t['tail'][-1][:70] if t['tail'] else ''}")

        results["samples"][fn] = {
            "size": size, "sigs": {k: len(v) for k, v in sigs.items()},
            "our_scheme": our, "insert_resolve": ins,
            "rescue": rescue, "rescue_pct": round(pct, 1),
            "tools": tools,
        }

    # 进程自身峰值内存
    self_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    results["harness_peak_rss_kb"] = self_rss
    print(f"\n[harness] 本进程峰值 RSS = {self_rss} KB ({self_rss/1024:.1f} MiB)")

    outp = os.path.join(HERE, "extreme_results.json")
    with open(outp, "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print(f"[harness] 结果 → {outp}")


if __name__ == "__main__":
    main()
