#!/usr/bin/env python3
"""手工构造小体积 ZIP64 伪装包，用于验证「情形 C」基准不一致时的 locator 补丁方案。

内部偏移全部相对归档起点；再在文件头垫 M 字节垃圾 ⇒ 真实位置 = 记录值 + M。
情形 A：Δ_eocd == Δ_cd == M（zip crate 能直接开）
情形 C：把 locator 的声明值改小 4096 ⇒ Δ_eocd = M + 4096 != Δ_cd = M（必须打补丁）
"""
import struct, zlib, sys, os

OUT = sys.argv[1] if len(sys.argv) > 1 else "testdata/zip64_shifted_a.zip"
TAMPER = len(sys.argv) > 2 and sys.argv[2] == "tamper"
M = 1_000_000                     # 垫在最前面的垃圾字节数
JUNK = b"\xaa" * M

NAME = b"示例包.7z.001"
PAYLOAD = bytes((i * 7) % 251 for i in range(4096)) + b"\x37\x7a\xbc\xaf\x27\x1c" + b"TAIL"
CRC = zlib.crc32(PAYLOAD) & 0xFFFFFFFF
US = len(PAYLOAD)
CS = len(PAYLOAD)          # stored，所以 csize == usize

# ── LFH ──
lfh_extra = struct.pack("<HHQQ", 0x0001, 16, US, CS)
lfh = struct.pack("<IHHHHHIIIHH", 0x04034B50, 45, 0, 0, 0, 0x21, CRC,
                  0xFFFFFFFF, 0xFFFFFFFF, len(NAME), len(lfh_extra)) + NAME + lfh_extra
lho = 0
data_off = len(lfh)
cd_off = data_off + len(PAYLOAD)

# ── CDFH (zip64 extra: usize, csize, lho) ──
cd_extra = struct.pack("<HHQQQ", 0x0001, 24, US, CS, lho)
cdfh = struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 45, 45, 0, 0, 0, 0x21, CRC,
                   0xFFFFFFFF, 0xFFFFFFFF, len(NAME), len(cd_extra), 0, 0, 0, 0, 0xFFFFFFFF) + NAME + cd_extra
cd_size = len(cdfh)
e64_off = cd_off + cd_size

# ── EOCD64 ──
eocd64 = struct.pack("<IQHHIIQQQQ", 0x06064B50, 44, 45, 45, 0, 0, 1, 1, cd_size, cd_off)
loc_off = e64_off + len(eocd64)
declared = e64_off
if TAMPER:
    declared = e64_off - 4096          # ← 篡改
locator = struct.pack("<IIQI", 0x07064B50, 0, declared, 1)
eocd_off = loc_off + len(locator)
eocd = struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 0xFFFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0)

archive = lfh + PAYLOAD + cdfh + eocd64 + locator + eocd
blob = JUNK + archive

with open(OUT, "wb") as f:
    f.write(blob)

print(f"写出 {OUT}  大小={len(blob)}")
print(f"  内部: lho={lho} data={data_off} cd_off={cd_off} cd_size={cd_size} e64={e64_off} loc={loc_off} eocd={eocd_off}")
print(f"  垫字节 M={M}  tamper={TAMPER}")
print(f"  期望 真实CD位置 = {cd_off + M}, 真实EOCD64 = {e64_off + M}, locator声明值 = {declared}")
print(f"  期望 Δ_cd = {(cd_off + M) - cd_off}   Δ_eocd = {(e64_off + M) - declared}")
