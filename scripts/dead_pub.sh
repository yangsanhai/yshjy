#!/usr/bin/env bash
# 找出「声明成 pub 但没人用」的条目 —— 只报告，不自动删。
#
# 为什么需要这个：Rust 里 `pub` 会**压住** dead_code 检查，所以
# `cargo clippy --all-targets -D warnings` 全绿**挡不住**这类烂账
# （见 docs/IMPL-PROVENANCE.md「死代码」那节）。
#
# 用法：
#   bash scripts/dead_pub.sh            # 人看
#   bash scripts/dead_pub.sh --count    # 只报数量（给探针用）
#
# 退出码永远是 0 —— 这是**审计辅助**不是门禁：报告里的人必须逐条判断
# 「真没人用」还是「给 FFI / 外部 / 未来留的」。删之前先 grep 全仓（含 gui/、tests/）。

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

count_only=0
[ "${1:-}" = "--count" ] && count_only=1

# 候选：行首的 pub 声明。名字允许非 ASCII（本项目函数名是中文），
# 所以名字部分取「到空白或括号/冒号为止」而不是 [A-Za-z0-9_]。
candidates=$(grep -rnE '^[[:space:]]*pub[[:space:]]+(fn|struct|enum|const|static|type|trait|mod)[[:space:]]+' \
  --include='*.rs' src 2>/dev/null | awk -F: '
  {
    file = $1; line = $2; code = "";
    for (i = 3; i <= NF; i++) code = code (i == 3 ? "" : ":") $i;
    if (match(code, /pub[ \t]+(fn|struct|enum|const|static|type|trait|mod)[ \t]+[^(<:{ \t]+/)) {
      decl = substr(code, RSTART, RLENGTH);
      k = split(decl, parts, /[ \t]+/);
      name = parts[k];
      # `pub mod foo;` 是模块声明，不是「没人用的 API」，别混进来
      if (name !~ /;$/) print file "\t" line "\t" name;
    }
  }')

dead=0
lines=()
while IFS=$'\t' read -r file line name; do
  [ -z "${name:-}" ] && continue
  # 已经显式声明「我知道没人用，但留着」的（兼容名 / 测试/生成器专用）不算烂账
  if sed -n "$((line > 3 ? line - 3 : 1)),$((line - 1))p" "$file" 2>/dev/null |
       grep -qE '#\[allow\(dead_code\)\]|#\[deprecated'; then
    continue
  fi
  # 引用次数：整个仓库（引擎 + 界面 + 测试 + 示例）里作为**单词**出现的次数
  refs=$(grep -rwF "$name" --include='*.rs' --include='*.dart' \
           src gui/lib gui/test tests examples 2>/dev/null | wc -l)
  # 只有定义本身一处 = 疑似死代码
  if [ "${refs:-0}" -le 1 ]; then
    lines+=("$(printf '%-44s %-24s %s:%s' "$name" "$(basename "$file")" "$file" "$line")")
    dead=$((dead + 1))
  fi
done <<< "$candidates"

if [ "$count_only" -eq 1 ]; then
  echo "$dead"
  exit 0
fi

echo "═══ pub 但疑似没人用：$dead 个 ═══"
if [ "$dead" -eq 0 ]; then
  echo "（没有 —— 也可能说明判据太松，别当成万事大吉）"
else
  printf '%s\n' "${lines[@]}"
  echo
  echo "逐条判断："
  echo "  ① 真死 → 删掉（删完必须 cargo test + flutter test 全过）"
  echo "  ② 给 FFI / 外部用的 → 加 #[allow(dead_code)] + 一行注释说明谁在用"
  echo "  ③ 给未来留的接口 → 删掉，用到时再写（YAGNI）"
fi
exit 0
