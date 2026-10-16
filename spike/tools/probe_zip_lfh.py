#!/usr/bin/env python3
"""深挖伪装 ZIP 的 LFH / ZIP64 extra，为 Rust spike 提供精确基准。"""
import struct, os

p = os.path.expanduser("~/下载/baiduwangpan/示例包/lvIesSotSDRhnZfL_1.mp4.zip")
ZIP_BASE = 119248031
CD_REAL = 8711802959
EOCD64_OFF = 8711803036
LOC_OFF = 8711803092

print("file_size", os.path.getsize(p))

def parse_extra(extra, cs, us, lho=None):
    i = 0
    while i + 4 <= len(extra):
        hid, hsz = struct.unpack_from("<HH", extra, i)
        body = extra[i + 4:i + 4 + hsz]
        print(f"   - extra id={hid:#06x} size={hsz} body={body.hex()}")
        if hid == 0x0001:
            j = 0; out = {}
            if us == 0xffffffff:
                out["usize"] = struct.unpack_from("<Q", body, j)[0]; j += 8
            if cs == 0xffffffff:
                out["csize"] = struct.unpack_from("<Q", body, j)[0]; j += 8
            if lho is not None and lho == 0xffffffff:
                out["lho"] = struct.unpack_from("<Q", body, j)[0]; j += 8
            print("      zip64 ->", out)
        i += 4 + hsz

with open(p, "rb") as f:
    print("\n[文件头 32 字节]", f.read(32).hex())
    for off in (ZIP_BASE - 4, ZIP_BASE, ZIP_BASE + 4):
        f.seek(off); print(f"@ {off}: {f.read(8)!r}")

    f.seek(ZIP_BASE); h = f.read(46 + 128)
    sig, ver, flags, method, mt, md, crc, cs, us, fnl, efl = struct.unpack_from("<IHHHHHIIIHH", h, 0)
    print(f"\n[LFH@{ZIP_BASE}] sig={sig:#010x} ver={ver:#x} flags={flags:#06x} method={method} crc={crc:08x}")
    print(f"  csize32={cs:#x} usize32={us:#x} fnl={fnl} efl={efl} name={h[30:30+fnl]!r}")
    parse_extra(h[30 + fnl:30 + fnl + efl], cs, us)

    f.seek(CD_REAL); e = f.read(46 + 128)
    fnl2 = struct.unpack_from("<H", e, 28)[0]; efl2 = struct.unpack_from("<H", e, 30)[0]
    cs2 = struct.unpack_from("<I", e, 20)[0]; us2 = struct.unpack_from("<I", e, 24)[0]
    lho2 = struct.unpack_from("<I", e, 42)[0]
    print(f"\n[CD entry] name={e[46:46+fnl2]!r} cs32={cs2:#x} us32={us2:#x} lho32={lho2:#x}")
    parse_extra(e[46 + fnl2:46 + fnl2 + efl2], cs2, us2, lho2)

    dstart = ZIP_BASE + 30 + fnl + efl
    print(f"\n[数据起点] {dstart}   [数据长度] 8592555306")

    # 尾部结构核对
    f.seek(EOCD64_OFF); b = f.read(56)
    print(f"\n[EOCD64@{EOCD64_OFF}] rec_size={struct.unpack_from('<Q',b,4)[0]} cd_off={struct.unpack_from('<Q',b,40)[0]}")
    print(f"   Δ = CD_real - cd_off = {CD_REAL - struct.unpack_from('<Q',b,40)[0]}")
    f.seek(LOC_OFF); b = f.read(20)
    print(f"[LOC@{LOC_OFF}] declared_eocd64={struct.unpack_from('<Q',b,8)[0]}")
    print(f"   Δ = EOCD64_off - declared = {EOCD64_OFF - struct.unpack_from('<Q',b,8)[0]}")
    print(f"\n>>> ZIP_BASE(实测) = {ZIP_BASE}; LFH 记录 offset=0 ⇒ LFH_real = 0 + Δ = {0 + (CD_REAL-8592554928)}")
