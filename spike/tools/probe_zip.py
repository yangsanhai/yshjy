#!/usr/bin/env python3
"""定位伪装 ZIP 的真实结构（spike 用，只读文件头尾，不读 8.6GB）。"""
import struct, sys, os

path = sys.argv[1] if len(sys.argv) > 1 else "/home/user/下载/baiduwangpan/示例包/lvIesSotSDRhnZfL_1.mp4.zip"
fsize = os.path.getsize(path)
print(f"file      = {path}")
print(f"file_size = {fsize} ({fsize/1e9:.4f} GB)")

MAX_BACK = 65572 + 4096
with open(path, "rb") as f:
    f.seek(max(0, fsize - MAX_BACK))
    tail = f.read()
base = fsize - len(tail)
print(f"tail_base = {base}  tail_len = {len(tail)}")

# --- EOCD ---
i = tail.rfind(b"PK\x05\x06")
assert i >= 0, "no EOCD"
eocd_off = base + i
sig, disk, cddisk, n_this, n_tot, cd_size, cd_off, cmlen = struct.unpack_from("<IHHHHIIH", tail, i)
print(f"\n[EOCD] off={eocd_off} (EOF-{fsize-eocd_off})  entries={n_tot}  cd_size={cd_size}  cd_offset_field={cd_off}")
comment = tail[i + 22: i + 22 + cmlen]
print(f"       comment_len={cmlen}  comment={comment[:200]!r}")

# --- EOCD64 locator (20 bytes right before EOCD) ---
loc_off = eocd_off - 20
with open(path, "rb") as f:
    f.seek(loc_off); loc = f.read(20)
lsig, ldisk, l_eocd64, l_disks = struct.unpack("<IIQI", loc)
print(f"\n[LOC64] off={loc_off}  sig={lsig:08x} (expect 07064b50)  declared_eocd64_off={l_eocd64}")
assert lsig == 0x07064b50, "locator sig mismatch"

# --- EOCD64 (right before locator) ---
with open(path, "rb") as f:
    f.seek(loc_off - 56); probe = f.read(56)
e64_off = loc_off - struct.unpack_from("<Q", probe, 4)[0] - 12
with open(path, "rb") as f:
    f.seek(e64_off); e64 = f.read(56)
print(f"\n[EOCD64] off={e64_off}  sig={struct.unpack_from('<I',e64,0)[0]:08x} (expect 06064b50)")
rec_size, ver1, ver2, edisk, ecddisk, en_this, en_tot = struct.unpack_from("<QHHIIQQ", e64, 4)
d64_cd_size, d64_cd_off = struct.unpack_from("<QQ", e64, 40)
print(f"         record_size={rec_size}  entries={en_tot}  cd_size={d64_cd_size}  cd_offset_field={d64_cd_off}")
print(f"         gap: loc_off - e64_off = {loc_off - e64_off} (expect rec_size+12={rec_size+12})")

# --- Δ 三个量 ---
# 真实 CD 位置：向后扫 CDFH 魔数。CD 一定在 EOCD64 之前
with open(path, "rb") as f:
    f.seek(e64_off - 300_000) if e64_off > 300_000 else f.seek(0)
    win_base = f.tell(); win = f.read(e64_off - win_base)
cdfh_rel = win.rfind(b"PK\x01\x02")
cd_real = win_base + cdfh_rel
print(f"\n[CD]    真实位置 first_CDFH={cd_real}")
print(f"        EOCD.cd_offset_field      = {cd_off}")
print(f"        EOCD64.cd_offset_field    = {d64_cd_off}")

d_cd   = cd_real - d64_cd_off          # 我们的 ZIP 起点公式
d_eocd = e64_off - l_eocd64            # zip2 Detect 会算出的 archive_offset
print(f"\nΔ_cd    = C  - D64 = {d_cd}")
print(f"Δ_eocd  = P  - L   = {d_eocd}")
print(f"Δ_entry(待抽样)")

# --- 抽样第一条 CDFH entry ---
with open(path, "rb") as f:
    f.seek(cd_real); e = f.read(300)
sig, vm, vn, flags, method, mt, md, crc, csize, usize = struct.unpack_from("<IHHHHHHIII", e, 0)
fnl, efl, fcl = struct.unpack_from("<HHH", e, 28)
name = e[46:46+fnl]
lho = struct.unpack_from("<I", e, 42)[0]
print(f"\n[CD entry 0] name={name!r} flags={flags:04x} method={method}")
print(f"             crc={crc:08x} csize={csize} usize={usize} local_header_offset_field={lho}")

# local file header 真实位置
with open(path, "rb") as f:
    f.seek(cd_real - 1_000_000) if cd_real > 1_000_000 else f.seek(0)
    wb = f.tell(); w = f.read(cd_real - wb)
lfh_rel = w.find(b"PK\x03\x04")
print(f"             (+ 再往前找 CDFH 中记录的 offset 处)")
# 找所有 LFH
pos = 0; found = []
while True:
    j = w.find(b"PK\x03\x04", pos)
    if j < 0: break
    found.append(wb + j); pos = j + 1
print(f"             窗口内 PK\\x03\\x04 出现 {len(found)} 次, 最后几个: {found[-5:]}")
lfh_real = found[-1] if found else None
print(f"             Δ_entry = LFH_real - local_header_offset_field = {lfh_real - lho if lfh_real else 'n/a'}")

print(f"\n===== 汇总 =====")
print(f"Δ_cd   = {d_cd}")
print(f"Δ_eocd = {d_eocd}")
print(f"Δ_entry= {lfh_real - lho if lfh_real else 'n/a'}")
print(f"三者是否相等: {d_cd == d_eocd == (lfh_real - lho if lfh_real else -1)}")
