import struct, sys
p = sys.argv[1]
with open(p, "rb") as f:
    b = f.read(32)
sig, ver, crc = b[0:6], b[6:8], b[8:12]
nho, nhs, nhcrc = struct.unpack_from("<QQI", b, 12)
print(f"{p}: sig={sig!r} ver={ver.hex()} next_header_offset={nho} next_header_size={nhs} next_crc={nhcrc:08x}")
print(f"   SIGNATURE_HEADER_SIZE+offset = {32+nho}")
