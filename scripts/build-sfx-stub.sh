#!/usr/bin/env bash
# 交叉编译 Windows 自解压存根 → assets/sfx/yshjy-sfx.exe
#
# 为什么是独立脚本：存根只在改 sfx/ 时才需要重编，日常 cargo build 不应该
# 依赖 Windows 工具链。产物入库（约 0.5 MB），这样别人 clone 下来直接能编。
#
# 依赖（Debian/Ubuntu，需要 sudo）：
#     sudo apt install -y gcc-mingw-w64-x86-64
#
# 用法：bash scripts/build-sfx-stub.sh
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
SRC="sfx/target/$TARGET/release/yshjy-sfx.exe"
DST="assets/sfx/yshjy-sfx.exe"

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    cat >&2 <<'EOF'
❌ 找不到 x86_64-w64-mingw32-gcc（Windows 交叉链接器）

装一下（需要 sudo）：
    sudo apt install -y gcc-mingw-w64-x86-64

装完重跑本脚本即可。存根不会编进日常 Linux 构建，只有生成自解压包时用。
EOF
    exit 1
fi

rustup target add "$TARGET"
cargo build --release --manifest-path sfx/Cargo.toml --target "$TARGET"

mkdir -p "$(dirname "$DST")"
cp "$SRC" "$DST"

# 自检：必须是 PE（MZ），并且不是空壳
printf '产物：%s（%s 字节）\n' "$DST" "$(stat -c%s "$DST")"
head -c 2 "$DST" | od -An -c | grep -q 'M   Z' || {
    echo "❌ 产物不是 Windows PE（MZ 头）" >&2
    exit 1
}
echo "✅ 存根已更新。接着重新编主程序：cargo build --release"
