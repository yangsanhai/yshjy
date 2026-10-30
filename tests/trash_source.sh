#!/usr/bin/env bash
# --trash-source / --delete-source 探针
#
# 为什么单独一个探针：这两个选项会**动用户的原始文件**，是所有功能里唯一不可逆的
# 一类。断言必须覆盖四种情况，少一条就可能出「以为它进回收站、其实删了」的事故：
#   ① --trash-source   → 原包进回收站（files/ + info/ 都在），原位置消失
#   ② --delete-source 非交互（stdin 不是终端）→ **拒绝**，退出码 2，原包不动
#   ③ --delete-source 交互输 y → 真删，打印「已永久删除」
#   ④ --delete-source 交互输 n → 取消，原包不动
#
# 全程在 mktemp 里造样本，绝不碰用户的真实文件；回收站也用 XDG_DATA_HOME 隔离。
#
# 用法：bash tests/trash_source.sh [引擎路径]

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
  for c in target/release/yshjy target/debug/yshjy; do
    [ -x "$c" ] && BIN="$(pwd)/$c" && break
  done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "失败：找不到引擎（先 cargo build --release）"
  exit 1
fi

S=$(mktemp -d "${TMPDIR:-/tmp}/yshjy_trashsrc.XXXXXX")
mkdir -p "$S/data"
export XDG_DATA_HOME="$S/data"

PASS=0
FAIL=0
FAILED=()
ok()  { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }

mk() { python3 -c "import zipfile,sys;z=zipfile.ZipFile(sys.argv[1],'w');z.writestr('a.txt','hi');z.close()" "$1"; }
# 夹具用 ASCII 名：Windows 的验证要过 powershell（COM 查回收站），
# 中文参数在 LANG=C 的代码页转换下会搅（W7 实测）；Linux 同名不受影响
mk "$S/pkg1.zip"; mk "$S/pkg2.zip"; mk "$S/pkg3.zip"; mk "$S/pkg4.zip"

# ① 移回收站
out=$("$BIN" jy "$S/pkg1.zip" --trash-source 2>&1)
if [ ! -e "$S/pkg1.zip" ]; then ok "--trash-source：原位置已消失"; else bad "--trash-source：原包还在原地"; fi
if uname -s | grep -q '^MINGW'; then
  # Windows 走系统回收站（COM IFileOperation），XDG_DATA_HOME 隔离无效 ——
  # 改验 shell 命名空间里真有这条目（能在资源管理器里还原的那份账）
  n=$(powershell -NoProfile -Command "(New-Object -ComObject Shell.Application).Namespace(0xA).Items() | Where-Object { \$_.Name -eq 'pkg1.zip' } | Measure-Object | Select-Object -ExpandProperty Count" 2>/dev/null)
  if [ "${n:-0}" -ge 1 ]; then
    ok "--trash-source：系统回收站里有条目（可还原）"
  else
    bad "--trash-source：系统回收站里没找到 pkg1.zip"
  fi
elif [ -f "$S/data/Trash/files/pkg1.zip" ] && [ -f "$S/data/Trash/info/pkg1.zip.trashinfo" ]; then
  ok "--trash-source：回收站里有 files/ + info/（可还原）"
else
  bad "--trash-source：回收站里没有完整条目（$(find "$S/data/Trash" -type f 2>/dev/null | wc -l) 个文件）"
fi
if printf '%s' "$out" | grep -q "已移入回收站"; then
  ok "--trash-source：打印了「已移入回收站」"
else
  bad "--trash-source：没打印结果提示"
fi

# ② 非交互拒绝（stdin 重定向到 /dev/null，不是终端）
"$BIN" jy "$S/pkg2.zip" --delete-source </dev/null >"$S/o2" 2>&1
rc=$?
if [ "$rc" -eq 2 ]; then ok "--delete-source 非交互：退出码 2（用法错误）"; else bad "--delete-source 非交互：退出码 $rc（期望 2）"; fi
if [ -e "$S/pkg2.zip" ]; then ok "--delete-source 非交互：原包没动"; else bad "--delete-source 非交互：原包被删了"; fi
if grep -q "只能在交互终端里用" "$S/o2"; then
  ok "--delete-source 非交互：提示改用 --trash-source"
else
  bad "--delete-source 非交互：没给出人话提示"
fi

# ③ 交互输 y → 真删（script 给命令一个 pty，让 IsTerminal 成立）
if command -v script >/dev/null 2>&1; then
  printf 'y\n' | script -qec "$BIN jy $S/pkg3.zip --delete-source" /dev/null >"$S/o3" 2>&1
  if [ ! -e "$S/pkg3.zip" ]; then ok "--delete-source 输 y：原包已永久删除"; else bad "--delete-source 输 y：原包还在"; fi
  if grep -q "已永久删除" "$S/o3"; then
    ok "--delete-source 输 y：打印了「已永久删除」"
  else
    bad "--delete-source 输 y：没打印结果提示"
  fi

  # ④ 交互输 n → 取消
  printf 'n\n' | script -qec "$BIN jy $S/pkg4.zip --delete-source" /dev/null >"$S/o4" 2>&1
  if [ -e "$S/pkg4.zip" ]; then ok "--delete-source 输 n：原包没动（取消生效）"; else bad "--delete-source 输 n：原包被删了"; fi
  if grep -q "已取消" "$S/o4"; then
    ok "--delete-source 输 n：打印了「已取消」"
  else
    bad "--delete-source 输 n：没打印取消提示"
  fi
else
  printf '  \033[33m–\033[0m %s\n' "交互两态（跳过：没装 script，来自 util-linux）"
fi

echo
printf '  --trash-source / --delete-source：通过 %d · 失败 %d（现场：%s）\n' "$PASS" "$FAIL" "$S"
if [ "$FAIL" -gt 0 ]; then
  for c in "${FAILED[@]}"; do printf '    · %s\n' "$c"; done
  exit 1
fi
exit 0
