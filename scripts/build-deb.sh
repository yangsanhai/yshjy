#!/usr/bin/env bash
# 打 .deb 安装包（不需要 sudo）
#
# 用法: bash scripts/build-deb.sh
#
# 前置：先有 release 二进制（bash scripts/release.sh 或 cargo build --release）
# 产物：dist/yshjy_<版本>_<架构>.deb
#
# 为什么不交给 cargo-deb：那是个额外的第三方工具，而 dpkg-deb 是系统自带的。
# 一个 30 行的脚本就能说清楚包里到底装了什么 —— 可审计。

set -euo pipefail
cd "$(dirname "$0")/.."

V=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(dpkg --print-architecture)
STAGE=$(mktemp -d)

cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

[ -x target/release/yshjy ] || {
  echo "!! 找不到 target/release/yshjy —— 先跑：cargo build --release" >&2
  exit 1
}

echo "==> 组装 yshjy $V（$ARCH）"

# ── 目录结构（FHS）──
mkdir -p "$STAGE/DEBIAN"
mkdir -p "$STAGE/usr/bin"
mkdir -p "$STAGE/usr/share/man/man1"
mkdir -p "$STAGE/usr/share/bash-completion/completions"
mkdir -p "$STAGE/usr/share/zsh/vendor-completions"
mkdir -p "$STAGE/usr/share/fish/vendor_completions.d"
mkdir -p "$STAGE/usr/share/doc/yshjy"

# ── 程序 ──
install -m755 target/release/yshjy "$STAGE/usr/bin/yshjy"
ln -s yshjy "$STAGE/usr/bin/yshys"   # 打包模式的别名（同一个程序，看名字决定默认行为）

# ── 手册与补全 ──
gzip -9c docs/yshjy.1 > "$STAGE/usr/share/man/man1/yshjy.1.gz"
install -m644 scripts/completions/yshjy.bash "$STAGE/usr/share/bash-completion/completions/yshjy"
install -m644 scripts/completions/_yshjy     "$STAGE/usr/share/zsh/vendor-completions/_yshjy"
install -m644 scripts/completions/yshjy.fish "$STAGE/usr/share/fish/vendor_completions.d/yshjy.fish"

# ── 文档 ──
for f in LICENSE README.md CHANGELOG.md CONTRIBUTING.md; do
  [ -f "$f" ] && install -m644 "$f" "$STAGE/usr/share/doc/yshjy/$f"
done

# ── 控制文件 ──
INSTALLED_SIZE=$(du -sk "$STAGE" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<EOF
Package: yshjy
Version: $V
Section: utils
Priority: optional
Architecture: $ARCH
Installed-Size: $INSTALLED_SIZE
Depends: libc6 (>= 2.34)
Maintainer: yangsanhai <1848409757@qq.com>
Homepage: https://github.com/yangsanhai/yshjy
Description: 杨三海解压 —— 还原伪装成视频/图片的压缩包
 网盘下载的 .mp4.zip 常常"所有工具都打不开"，因为真归档藏在视频数据中间。
 yshjy 自己定位归档起点、再交给成熟的解码库解压，支持 ZIP/ZIP64/7z/RAR/TAR
 以及 gzip/xz/zstd/bzip2/brotli/lz4 单流格式，含分卷拼接与中文密码。
 反向也能把目录打包成能播放的伪装视频/图片。不联网、不做字典爆破。
EOF

# 版权文件（Debian 政策要求可选的 copyright；有 LICENSE 就顺手放一份）
if [ -f LICENSE ]; then
  mkdir -p "$STAGE/usr/share/doc/yshjy"
  cp LICENSE "$STAGE/usr/share/doc/yshjy/copyright"
fi

mkdir -p dist
OUT="dist/yshjy_${V}_${ARCH}.deb"
if [ -e "$OUT" ]; then
  echo "!! $OUT 已存在 —— 脚本不替你删，请自己确认怎么处理" >&2
  exit 1
fi

dpkg-deb --build --root-owner-group "$STAGE" "$OUT" >/dev/null
echo "    → $OUT（$(du -h "$OUT" | cut -f1)）"
echo
echo "安装： sudo apt install ./$OUT"
echo "卸载： sudo apt remove yshjy"
