#!/usr/bin/env bash
# 把常见压缩格式的「默认打开方式」设成杨三海解压（或还原回去）
#
# 为什么需要它：光在 .desktop 里写 MimeType 是不够的 —— GNOME 的文件管理器
# （Nautilus）默认就抢着处理 zip，用户装完双击压缩包，出来的是「解压到文件夹」，
# 而不是我们的界面。
#
# 用法：
#   scripts/set-default-mime.sh            设成杨三海解压
#   scripts/set-default-mime.sh --restore  还原成系统默认（卸掉这个工具前想恢复就用它）
#   scripts/set-default-mime.sh --status   只看现在是谁
#
# ⚠️ bash 变量名一律英文（bash 不认中文）
set -u

APP="yshjy.desktop"
MIMES="application/zip application/x-zip-compressed
application/x-7z-compressed application/vnd.rar
application/x-rar application/x-rar-compressed
application/x-tar application/x-compressed-tar
application/gzip application/x-xz application/zstd
application/x-bzip2 application/x-brotli application/x-lz4"

if ! command -v xdg-mime >/dev/null 2>&1; then
  echo "缺 xdg-mime（装 xdg-utils）：sudo apt install xdg-utils" >&2
  exit 1
fi

case "${1:-}" in
  --status)
    for m in $MIMES; do
      printf '%-34s %s\n' "$m" "$(xdg-mime query default "$m" 2>/dev/null)"
    done
    exit 0
    ;;
  --restore)
    # 还原成「文件管理器自己处理」—— 这也是 Debian 上没装我们时的默认状态
    for m in $MIMES; do
      xdg-mime default org.gnome.Nautilus.desktop "$m" 2>/dev/null || true
    done
    echo "已还原：常见压缩包交回文件管理器处理"
    exit 0
    ;;
  "")
    ;;
  *)
    echo "用法: $0 [--status|--restore]" >&2
    exit 2
    ;;
esac

# 确认我们的 desktop 文件确实装上了，否则设了也是空指向
if ! command -v desktop-file-validate >/dev/null 2>&1; then
  : # 校验工具不是必须的
fi
found=""
for d in "${XDG_DATA_HOME:-$HOME/.local/share}/applications" /usr/share/applications; do
  if [ -f "$d/$APP" ]; then
    found="$d/$APP"
    break
  fi
done
if [ -z "$found" ]; then
  echo "没找到 $APP —— 先把杨三海解压装上（deb 或 scripts/install-gui.sh）" >&2
  exit 1
fi

set_ok=0
for m in $MIMES; do
  if xdg-mime default "$APP" "$m" 2>/dev/null; then
    set_ok=$((set_ok + 1))
  fi
done

echo "已把 $set_ok 种压缩格式的默认打开方式设为杨三海解压"
echo "（desktop 文件：$found）"
echo "想看结果：$0 --status"
echo "想还原：  $0 --restore"
