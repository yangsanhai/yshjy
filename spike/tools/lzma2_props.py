import sys
# LZMA2 props -> dictionary size（sevenz-rust2 decoder.rs 里的算法）
p = int(sys.argv[1])
if p > 40:
    d = 0xFFFFFFFF
else:
    d = (2 | (p & 1)) << (p // 2 + 11)
print(f"LZMA2 props={p} -> dict={d} ({d/1024/1024:.1f} MiB)")
