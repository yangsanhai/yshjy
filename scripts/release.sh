#!/usr/bin/env bash
# 打发布包 —— 一次出两个版本
#
#   常用版  yshjy-<版本>-linux-<架构>.tar.gz        日常下载会遇到的格式全都在
#   全能版  yshjy-full-<版本>-linux-<架构>.tar.gz   冷门格式也带上
#
# 用法： bash scripts/release.sh
#
# 说明：本脚本**不会删除任何已有目录**（怕误删）—— 如果 dist/ 下的目标目录
#       已存在，它会直接报错让你自己决定怎么处理。

set -euo pipefail
cd "$(dirname "$0")/.."

V=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
# 版本一致性：代码和界面必须同版本，否则打出来的包里两半对不上
V_GUI=$(grep -m1 '^version:' gui/pubspec.yaml | awk '{print $2}')
if [ "$V" != "$V_GUI" ]; then
  echo "!! 版本不一致，拒绝打包：" >&2
  echo "   Cargo.toml       = $V" >&2
  echo "   gui/pubspec.yaml = $V_GUI" >&2
  exit 1
fi
ARCH=$(uname -m)
DOCS=(CHANGELOG.md CONTRIBUTING.md DEVLOG.md HANDOFF.md install.sh LICENSE README.md)

echo "==> yshjy $V（$ARCH）"

# 打一个档位：$1=包名后缀 $2=cargo feature（空=默认） $3=target 目录
# 注意：变量名用英文 —— bash 的 local 不认中文标识符（踩过一次）
build_one() {
  local suffix="$1" feat="$2" tdir="$3"
  local pkg="yshjy${suffix}"
  local dir="dist/${pkg}-${V}"
  local tarball="${pkg}-${V}-linux-${ARCH}.tar.gz"

  if [ -e "$dir" ] || [ -e "dist/${tarball}" ]; then
    echo "!! dist/ 下已有 ${pkg}-${V} 的产物 —— 脚本不替你删，请自己确认怎么处理" >&2
    exit 1
  fi

  echo "==> 编译 ${pkg}"
  if [ -n "$feat" ]; then
    cargo build --release --features "$feat" --target-dir "$tdir"
  else
    cargo build --release --target-dir "$tdir"
  fi

  mkdir -p "$dir"
  cp "$tdir/release/yshjy" "$dir/"
  cp "${DOCS[@]}" "$dir/"
  chmod 755 "$dir/yshjy" "$dir/install.sh"

  tar -czf "dist/${tarball}" -C dist "${pkg}-${V}"
  (cd dist && sha256sum "${tarball}" > "${tarball}.sha256")

  printf '    → dist/%s（%s）\n' "${tarball}" "$(du -h "dist/${tarball}" | cut -f1)"
}

build_one "" "" "target"
build_one "-full" "full" "target-full"

echo
echo "==> 产物"
ls -l dist/*.tar.gz dist/*.sha256 2>/dev/null | tail -4
echo
echo "    常用版：普通用户装这个"
echo "    全能版：要给冷门格式时再装（当前覆盖与常用版一致，等冷门格式补进来才拉开差距）"
