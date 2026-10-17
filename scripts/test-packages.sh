#!/usr/bin/env bash
# 安装包验收：sha256 / tar.gz 解包运行 / deb 结构与解包运行 / 实际解压+打包功能
set -u
cd "$(dirname "$0")/../.." 2>/dev/null || true
PROJ="$HOME/hermes/yshjy"
cd "$PROJ/dist" || exit 1

echo "═══ 1. 校验和 ═══"
for f in yshjy-0.9.21-c-linux-x86_64.tar.gz yshjy-full-0.9.21-c-linux-x86_64.tar.gz; do
  if sha256sum -c "$f.sha256" >/dev/null 2>&1; then echo "PASS $f"; else echo "FAIL $f"; fi
done

T=$(mktemp -d "$HOME/.hermes/cache/scratch/pkgtest_XXXX")

echo "═══ 2. tar.gz 解包 ═══"
tar -xzf yshjy-0.9.21-c-linux-x86_64.tar.gz -C "$T"
BIN=$(find "$T" -type f -name yshjy | head -1)
echo "二进制：$BIN"
"$BIN" --version | head -1
echo "包内文件："
find "$T" -maxdepth 2 -type f | sed "s|$T/||" | head -10

echo "═══ 3. 安装包功能实测 ═══"
mkdir -p "$T/src" && echo "打包测试内容" > "$T/src/a.txt"
if "$BIN" ys -o "$T/pkg.mp4" "$T/src" >/dev/null 2>&1; then echo "PASS 打包"; else echo "FAIL 打包"; fi
if "$BIN" -o "$T/out" "$T/pkg.mp4" >/dev/null 2>&1; then echo "PASS 解压"; else echo "FAIL 解压"; fi
diff -q "$T/src/a.txt" "$T/out/src/a.txt" >/dev/null 2>&1 && echo "PASS 内容一致" || echo "FAIL 内容不一致"

echo "═══ 4. deb 结构 ═══"
dpkg-deb -I yshjy_0.9.21-c_amd64.deb 2>/dev/null | grep -E "Package|Version|Architecture|Installed-Size|Description" | head -6
echo "包内文件："
dpkg-deb -c yshjy_0.9.21-c_amd64.deb 2>/dev/null | awk '{print $6}' | head -12

echo "═══ 5. deb 解包运行（不装系统）═══"
D=$(mktemp -d "$HOME/.hermes/cache/scratch/debtest_XXXX")
dpkg-deb -x yshjy_0.9.21-c_amd64.deb "$D"
"$D/usr/bin/yshjy" --version | head -1 && echo "PASS deb 内二进制可运行"

echo
echo "测试目录：$T"
echo "deb 解包目录：$D"
