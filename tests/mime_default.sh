#!/usr/bin/env bash
# 「默认打开方式」探针（阶段 1 梯队四）
#
# 要验的事：装完之后双击 zip/7z/rar 打开的是不是杨三海解压。
#
# 关键坑（这轮踩了两次）：desktop 文件里的 Exec 必须指向**真实存在的可执行文件**——
# Exec 是个不存在的路径时，xdg-mime / GLib 会把整个 desktop 判无效，然后静默回退到
# 文件管理器，mimeapps.list 里写得再对也没用。所以探针里用 /usr/bin/true 冒充。
#
# 全程用隔离的 XDG_DATA_HOME / XDG_CONFIG_HOME，不碰用户真实配置。
#
# ⚠️ bash 变量名一律英文
set -u

DESKTOP="${1:-gui/linux/yshjy.desktop}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
fail=0

check() {
  local name="$1"
  local ok="$2"
  if [ "$ok" = "0" ]; then
    echo "  ✓ $name"
  else
    echo "  ✗ $name"
    fail=$((fail + 1))
  fi
}

if ! command -v xdg-mime >/dev/null 2>&1; then
  echo "  -- 没有 xdg-mime，跳过"
  exit 0
fi
if [ ! -f "$DESKTOP" ]; then
  echo "  -- 没有 $DESKTOP，跳过"
  exit 0
fi

mkdir -p "$TMP/data/applications" "$TMP/config"
# Exec 必须是真的可执行文件，否则 desktop 会被判无效（见上面注释）
sed 's|__EXEC__|/usr/bin/true|' "$DESKTOP" > "$TMP/data/applications/yshjy.desktop"
export XDG_DATA_HOME="$TMP/data"
export XDG_CONFIG_HOME="$TMP/config"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$TMP/data/applications" >/dev/null 2>&1 || true
fi

# ① desktop 里该声明我们的格式（少一个，双击这种格式就不是我们）
for m in application/zip application/x-7z-compressed application/vnd.rar; do
  check "desktop 声明了 $m" \
    "$(grep -q "$m" "$DESKTOP" && echo 0 || echo 1)"
done

# ② 设默认后，xdg-mime / gio 都要认
bash scripts/set-default-mime.sh >"$TMP/set.log" 2>&1
for m in application/zip application/x-7z-compressed application/vnd.rar; do
  got=$(xdg-mime query default "$m" 2>/dev/null)
  check "$m 默认是杨三海解压（实际 $got）" \
    "$([ "$got" = "yshjy.desktop" ] && echo 0 || echo 1)"
done

# ③ 还原要真的退回去（用户卸之前能自救）
bash scripts/set-default-mime.sh --restore >"$TMP/restore.log" 2>&1
got=$(xdg-mime query default application/zip 2>/dev/null)
check " --restore 后不再是我们（实际 $got）" \
  "$([ "$got" != "yshjy.desktop" ] && echo 0 || echo 1)"

echo
if [ "$fail" = "0" ]; then
  echo "  默认打开方式：全部符合预期"
  exit 0
fi
echo "  默认打开方式：$fail 项不符合预期"
exit 1
