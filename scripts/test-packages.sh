#!/usr/bin/env bash
# 安装包验收：拿 dist/ 下**最新**的 deb 与 tar.gz 逐项过一遍
#
# 老版本把版本号写死成 0.9.21-c，早就过期 —— 跑起来验的是三个月前的老包，
# 看着「全 PASS」其实什么都没保证。现在自动取 dist/ 里最新的产物，
# 并把 0.9.25-c 那两个真事故（desktop 只 7 种格式、postinst 把默认项写进 /root）固化成断言。
#
# 用法： bash scripts/test-packages.sh
# 注意：变量名一律英文 —— bash 的 local 不认中文标识符（踩过）。

set -u
PROJ="${PROJ:-$HOME/hermes/yshjy}"
DIST="$PROJ/dist"
cd "$DIST" || { echo "!! 进不去 $DIST"; exit 1; }

PASS=0; FAIL=0
ok()  { echo "  PASS $*"; PASS=$((PASS+1)); }
no()  { echo "  FAIL $*"; FAIL=$((FAIL+1)); }
chk() { if [ "$1" -eq 0 ]; then ok "$2"; else no "$2"; fi; }

DEB=$(ls -t yshjy_*.deb 2>/dev/null | head -1)
TAR=$(ls -t yshjy-*-linux-*.tar.gz 2>/dev/null | head -1)
[ -n "$DEB" ] || { echo "!! dist/ 里没有 .deb —— 先跑 bash scripts/build-deb.sh"; exit 1; }

V=$(dpkg-deb -f "$DEB" Version 2>/dev/null)
echo "════ 被测产物 ════"
echo "  deb : $DEB  (版本 $V)"
[ -n "$TAR" ] && echo "  tar : $TAR"
[ -n "$TAR" ] || echo "  tar : （dist/ 里没有 tar.gz，跳过那部分）"
echo

T=$(mktemp -d "$HOME/.hermes/cache/scratch/pkgtest_XXXX")
trap 'rm -rf "$T"' EXIT

echo "═══ 1. deb 元数据 ═══"
META=$(dpkg-deb -f "$DEB" Package Version Architecture Depends Installed-Size Maintainer 2>/dev/null)
for key in Package Version Architecture Depends Installed-Size Maintainer; do
  if printf '%s\n' "$META" | grep -qE "^$key: .+"; then ok "元数据有 $key"; else no "元数据缺 $key"; fi
done
[ "$(dpkg-deb -f "$DEB" Package)" = "yshjy" ] && ok "包名 yshjy" || no "包名不是 yshjy"
[ "$(dpkg-deb -f "$DEB" Architecture)" = "$(dpkg --print-architecture)" ] \
  && ok "架构与本机一致" || no "架构与本机不一致"
printf '%s\n' "$(dpkg-deb -f "$DEB" Depends)" | grep -q 'libc6' \
  && ok "依赖里有 libc6" || no "依赖里没有 libc6"
printf '%s\n' "$(dpkg-deb -f "$DEB" Depends)" | grep -q 'libgtk-3' \
  && ok "依赖里有 libgtk-3（界面要用）" || no "依赖里没有 libgtk-3"

echo
echo "═══ 2. deb 文件表 ═══"
dpkg-deb -x "$DEB" "$T/root" 2>/dev/null
LIST=$(dpkg-deb -c "$DEB" 2>/dev/null | awk '{print $6}' | sed 's|^\./||')
for f in usr/bin/yshjy \
         usr/lib/yshjy/gui/yshjy_gui \
         usr/lib/yshjy/gui/lib/libapp.so \
         usr/lib/yshjy/gui/data/icudtl.dat \
         usr/share/applications/yshjy.desktop \
         usr/share/icons/hicolor/256x256/apps/yshjy.png \
         usr/share/man/man1/yshjy.1.gz \
         usr/share/bash-completion/completions/yshjy \
         usr/share/zsh/vendor-completions/_yshjy \
         usr/share/fish/vendor_completions.d/yshjy.fish \
         usr/share/nautilus/scripts/用杨三海解压; do
  printf '%s\n' "$LIST" | grep -qx "$f" && ok "包内有 $f" || no "包内缺 $f"
done

echo
echo "═══ 3. 那两起真事故的回归断言 ═══"
# ① 桌面项必须认领 14 种格式（0.9.25-c 只 7 种，装完双击仍归 Nautilus）
MIME_N=$(grep '^MimeType' "$T/root/usr/share/applications/yshjy.desktop" 2>/dev/null | tr ';' '\n' | sed '/^$/d' | wc -l)
[ "$MIME_N" = "14" ] && ok "桌面项认领 14 种格式" || no "桌面项只认领 $MIME_N 种（应 14）"
# ② postinst 里设默认必须显式带 HOME（否则写进 /root/.config/mimeapps.list）
dpkg-deb -e "$DEB" "$T/ctl" 2>/dev/null
if [ -f "$T/ctl/postinst" ] && grep -q 'xdg-mime default' "$T/ctl/postinst"; then
  grep -q 'HOME=' "$T/ctl/postinst" && ok "postinst 设默认带了 HOME" \
    || no "postinst 设默认没带 HOME（会写进 /root，用户双击还是 Nautilus）"
fi
# ③ man 页必须是 644（gzip 重定向曾按 umask 出 664）
MODE=$(dpkg-deb -c "$DEB" 2>/dev/null | awk '$6 ~ /man1\/yshjy\.1\.gz$/ {print $1}' | head -1)
[ "$MODE" = "-rw-r--r--" ] && ok "man 页权限 644" || no "man 页权限是 $MODE（应 -rw-r--r--）"
# ③b prerm 必须在、权限 755、语法过、且升级分支早退（别在 upgrade 时杀用户的解压）
if [ -f "$T/ctl/prerm" ]; then
  ok "包内有 prerm（卸载前钩子）"
  [ "$(stat -c %a "$T/ctl/prerm")" = "755" ] && ok "prerm 权限 755" || no "prerm 权限不是 755"
  sh -n "$T/ctl/prerm" 2>/dev/null && ok "prerm 语法通过（sh -n）" || no "prerm 语法错误"
  grep -q 'upgrade' "$T/ctl/prerm" && ok "prerm 区分了 upgrade（升级不杀进程）" \
    || no "prerm 没区分 upgrade —— 升级会干掉用户正在跑的解压"
else
  no "包内缺 prerm"
fi
# ④ 属性基准：属主必须 root/root（dpkg-deb --root-owner-group）
if dpkg-deb -c "$DEB" 2>/dev/null | awk '{print $2}' | grep -qv '^root/root$'; then
  no "有文件属主不是 root/root"
else
  ok "全部文件属主 root/root"
fi

echo
echo "═══ 4. deb 内二进制能不能跑、版本对不对 ═══"
EB="$T/root/usr/bin/yshjy"
if [ -x "$EB" ]; then
  # 版本号后面紧跟中文括号，必须正则抠，不能用 awk 取第 2 列
  V_BIN=$("$EB" --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.-]*' | head -1)
  ok "deb 内二进制可运行（--version 有输出）"
  [ "$V_BIN" = "$V" ] && ok "deb 内引擎版本 $V_BIN == 包版本 $V" \
    || no "deb 内引擎版本 $V_BIN ≠ 包版本 $V（拿了旧二进制？）"
else
  no "deb 内没有可执行的 usr/bin/yshjy"
fi

echo
echo "═══ 5. 功能实测（用包里的二进制真打包 / 真解压）═══"
if [ -x "$EB" ]; then
  mkdir -p "$T/src" && printf '打包测试内容\n' > "$T/src/a.txt"
  "$EB" ys -o "$T/pkg.mp4" "$T/src" >/dev/null 2>&1 && ok "打包成伪装视频" || no "打包失败"
  "$EB" -o "$T/out" "$T/pkg.mp4" >/dev/null 2>&1 && ok "解压回来" || no "解压失败"
  cmp -s "$T/src/a.txt" "$T/out/src/a.txt" && ok "内容逐字节一致" || no "内容不一致"
fi

echo
echo "═══ 6. tar.gz（若有）═══"
if [ -n "$TAR" ]; then
  if [ -f "$TAR.sha256" ] && (sha256sum -c "$TAR.sha256" >/dev/null 2>&1); then
    ok "sha256 校验通过"
  elif [ -f "$TAR.sha256" ]; then
    no "sha256 校验失败"
  fi
  tar -xzf "$TAR" -C "$T" 2>/dev/null
  TB=$(find "$T" -type f -name yshjy -newer "$T" 2>/dev/null | head -1)
  [ -z "$TB" ] && TB=$(find "$T" -maxdepth 3 -type f -name yshjy | head -1)
  if [ -n "$TB" ] && [ -x "$TB" ]; then
    V_TAR=$("$TB" --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.-]*' | head -1)
    ok "tar.gz 解包后二进制可运行"
    [ "$V_TAR" = "$V" ] && ok "tar.gz 里版本 $V_TAR == deb 版本 $V" \
      || no "tar.gz 里版本 $V_TAR ≠ deb 版本 $V（两个包不是同一次构建）"
  else
    no "tar.gz 解包后找不到可执行的 yshjy"
  fi
fi

echo
echo "════════════════════════════════════"
printf '  通过 %d · 失败 %d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] && echo "  全绿：这个包可以给人装了。" || echo "  有失败项，别发布。"
exit "$([ "$FAIL" -eq 0 ] && echo 0 || echo 1)"
