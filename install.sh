#!/usr/bin/env bash
# yshjy 安装脚本 —— 装到用户目录，不需要 sudo
#
# 用法:
#   bash install.sh              # 装到 ~/.local/bin
#   PREFIX=/usr/local bash install.sh   # 装到别处（可能要 sudo）
#   bash install.sh --uninstall  # 卸载

set -eu

PREFIX="${PREFIX:-$HOME/.local}"
BINDIR="$PREFIX/bin"
NAME="yshjy"
ALT="yshys"          # 打包模式的名字（软链接到同一个程序）

say()  { printf '  %s\n' "$1"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
die()  { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; exit 1; }

# ── 卸载 ──
if [ "${1:-}" = "--uninstall" ]; then
  printf '\n\033[1m卸载 yshjy\033[0m\n'
  rm -f "$BINDIR/$NAME" "$BINDIR/$ALT"
  ok "已从 $BINDIR 移除"
  exit 0
fi

printf '\n\033[1m安装 yshjy —— 杨三海解压\033[0m\n\n'

# ── 找到二进制 ──
SRC=""
for cand in "target/release/$NAME" "./$NAME" "$(dirname "$0")/target/release/$NAME"; do
  if [ -x "$cand" ]; then SRC="$cand"; break; fi
done
if [ -z "$SRC" ]; then
  say "没找到已编译的程序，先编译…"
  command -v cargo >/dev/null 2>&1 || die "没装 Rust（cargo）—— 请先装：https://rustup.rs"
  cargo build --release || die "编译失败"
  SRC="target/release/$NAME"
fi
[ -x "$SRC" ] || die "找不到可执行文件"
ok "找到程序：$SRC（$(du -h "$SRC" | cut -f1)）"

# ── 安装 ──
mkdir -p "$BINDIR" || die "建不了目录 $BINDIR"
install -m755 "$SRC" "$BINDIR/$NAME" || die "复制失败（可能需要 sudo）"
ok "已装到 $BINDIR/$NAME"

# 打包模式的别名（同一个程序，靠名字区分默认行为）
ln -sf "$NAME" "$BINDIR/$ALT" 2>/dev/null && ok "已建别名 $ALT（打包模式）"

# ── 检查 PATH ──
case ":$PATH:" in
  *":$BINDIR:"*) ok "PATH 里已有 $BINDIR" ;;
  *)
    warn "$BINDIR 不在 PATH 里，请把下面这行加到 ~/.zshrc 或 ~/.bashrc："
    printf '\n      export PATH="%s:$PATH"\n\n' "$BINDIR"
    ;;
esac

# ── 验证 ──
if command -v "$NAME" >/dev/null 2>&1 || [ -x "$BINDIR/$NAME" ]; then
  printf '\n'
  "$BINDIR/$NAME" --help >/dev/null 2>&1 && ok "自检通过：程序能跑" || warn "程序跑不起来，请反馈"
fi

printf '\n\033[1m装好了！试试看：\033[0m\n\n'
printf '    %s 你的视频.mp4          # 解压\n' "$NAME"
printf '    %s --list 你的视频.mp4   # 只看里面有什么\n' "$NAME"
printf '    %s ys 文件夹/            # 打包成伪装视频\n\n' "$NAME"
printf '    卸载：bash install.sh --uninstall\n\n'
