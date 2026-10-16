import sys, zlib, os
for p in sys.argv[1:]:
    if os.path.isfile(p):
        d = open(p, 'rb').read()
        print(f"{zlib.crc32(d) & 0xFFFFFFFF:08x}  {len(d):>8}  {p}")
        if len(d) <= 40:
            print(f"          内容: {d!r}")
