#!/usr/bin/env bash
# bash 补全脚本的**运行时行为**（不只是 `bash -n` 语法）
#
# 由来（2026-10-29）：`tests/cli_spec_match.sh` 只验「选项集合对不对 + 语法过不过」，
# 于是漏掉了真 bug —— fallback 写成 `COMPREPLY=($(compgen -f -- "$cur"))`，
# 命令替换会按默认 IFS 再分词，带空格的文件名「有 空格.zip」被劈成两个候选
# （没装 bash-completion 包时必现；装了才走 `_filedir` 躲过去）。
#
# 顺带记一条实测结论：换成 `IFS=$'\n' COMPREPLY=(…)` 这种**前缀写法**虽然不劈词，
# 但会把全局 IFS 改掉并泄漏到函数返回之后（实测随后 `set -- $(echo "a b")` 只有 1 个词），
# 会把用户交互 shell 里后续命令的分词全搞坏 —— 所以本仓库用 `mapfile -t COMPREPLY < <(...)`。
#
# 断言：
#   1. 没装 bash-completion（无 _filedir）时，带空格文件名只出**一个**候选
#   2. 装了 bash-completion（有 _filedir）时同样只出一个候选
#   3. `-o` / `--dest` 的目录补全对带空格目录名也不劈（-d 分支同一族）
#   4. `--password` 这类「故意不给候选」的位置仍然**零候选**（加 -o default 后靠 compopt 关回落）
#   5. 补全函数跑完不污染 IFS
#   6. 值候选按前缀过滤（`--enc` → utf-8 …）
#   7. `complete -p` 注册里带 `-o nosort` 与 `-F _yshjy_complete`
#   8. 补全函数不改动调用者的 COMP_WORDS / COMP_CWORD
#
# 用法：bash tests/completions.sh
set -u
cd "$(dirname "$0")/.." || exit 1

n_pass=0
n_fail=0
ok()  { echo "  ✓ $1"; n_pass=$((n_pass+1)); }
bad() { echo "  ✗ $1"; n_fail=$((n_fail+1)); }

COMP="scripts/completions/yshjy.bash"
if [ ! -r "$COMP" ]; then
    echo "❌ 找不到 $COMP"
    exit 1
fi

# 探针现场：带空格的文件名与目录名（绝不碰用户目录，全在临时目录里造）
D="${TMPDIR:-/tmp}/yshjy-comp-$$"
mkdir -p "$D/workdir 带空格" "$D/普通目录"
# 文件放在 $D 根：兜底分支是在「当前工作目录」里 compgen 的，放子目录里测不到
: > "$D/有 空格.zip"
: > "$D/另一个 带空格 的.7z"
: > "$D/plain.zip"

# runner：在干净的 bash 子进程里 source 补全脚本、手工摆好 COMP_*、调补全函数，
# 把结果用 0x1f（单元分隔符，文件名里不会有）拼成一行吐出来。
RUNNER="$D/runner.sh"
cat > "$RUNNER" <<'RUNNER_EOF'
#!/usr/bin/env bash
# 用法：runner.sh <补全脚本> <0|1 是否加载 bash-completion> <工作目录> <词...>
set +u
comp=$1; want_fd=$2; cdir=$3; shift 3

if [ "$want_fd" = 1 ]; then
    for f in /usr/share/bash-completion/bash_completion /etc/bash_completion; do
        [ -r "$f" ] && { source "$f" >/dev/null 2>&1; break; }
    done
fi
source "$comp"

cd "$cdir" || exit 1
COMP_WORDS=("$@")
COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))
COMP_LINE="${COMP_WORDS[*]}"
COMP_POINT=${#COMP_LINE}

_yshjy_complete
rc=$?

US=$'\x1f'
out="${#COMPREPLY[@]}"
for c in "${COMPREPLY[@]}"; do out+="$US$c"; done
printf '%s\n' "$out"
# IFS 是否被污染：默认 IFS 下 "a b" 必须分成 2 个词
set -- $(echo "a b")
printf 'ifs_words=%s\n' "$#"
printf 'rc=%s\n' "$rc"
RUNNER_EOF

# probe <是否加载 bash-completion 0|1> <词...> → stdout 三行：候选行 / ifs_words / rc
probe() {
    local fd="$1"; shift
    bash "$RUNNER" "$COMP" "$fd" "$D" "$@"
}

# 从候选行里取元素数与第 N 个候选（用 python 切 0x1f，避免再踩 IFS 的坑）
field() { # field <候选行> <索引：0=计数，1..=第 N 个>
    python3 -c 'import sys; p=sys.argv[1].split("\x1f"); i=int(sys.argv[2]); print(p[i] if i < len(p) else "")' "$1" "$2"
}

echo "── 1. 没装 bash-completion（走 fallback）：带空格的文件名不许劈开 ──"
out=$(probe 0 yshjy "有")
line=$(printf '%s\n' "$out" | sed -n 1p)
cnt=$(field "$line" 0)
c1=$(field "$line" 1)
if [ "$cnt" = "1" ] && [ "$c1" = "有 空格.zip" ]; then
    ok "「有 空格.zip」只出一个候选（count=$cnt）"
else
    bad "候选被劈开了：count=$cnt 第一个=[$c1]（期望 1 个 / 有 空格.zip）"
    printf '%s\n' "$line" | tr '\037' '\n' | sed 's/^/      /'
fi

echo "── 2. 装了 bash-completion（走 _filedir）：同样不许劈 ──"
out=$(probe 1 yshjy "有")
line=$(printf '%s\n' "$out" | sed -n 1p)
cnt=$(field "$line" 0)
c1=$(field "$line" 1)
if [ "$cnt" = "1" ] && [ "$c1" = "有 空格.zip" ]; then
    ok "「有 空格.zip」只出一个候选"
else
    bad "候选被劈开或内容不对：count=$cnt 第一个=[$c1]"
fi

echo "── 3. -o / --dest 的目录补全：带空格的目录名不许劈 ──"
out=$(probe 0 yshjy -o "")
line=$(printf '%s\n' "$out" | sed -n 1p)
has_full=$(python3 -c 'import sys; p=sys.argv[1].split("\x1f"); print("1" if "workdir 带空格" in p else "0")' "$line")
has_split=$(python3 -c 'import sys; p=sys.argv[1].split("\x1f"); print("1" if "workdir" in p else "0")' "$line")
if [ "$has_full" = "1" ] && [ "$has_split" = "0" ]; then
    ok "目录名「workdir 带空格」完整、没有被劈成 workdir"
else
    bad "目录补全劈开了（完整项=$has_full 劈开项=$has_split）"
fi

echo "── 4. --password / --select：故意不给候选的位置必须零候选 ──"
for opt in --password --select --split --level; do
    out=$(probe 0 yshjy "$opt" "")
    cnt=$(field "$(printf '%s\n' "$out" | sed -n 1p)" 0)
    if [ "$cnt" = "0" ]; then
        ok "$opt 零候选（文件名的 readline 回落在函数里已 compopt 关掉）"
    else
        bad "$opt 出了 $cnt 个候选（应为 0）"
    fi
done

echo "── 5. 补全函数不许污染 IFS ──"
out=$(probe 0 yshjy "有")
ifs_words=$(printf '%s\n' "$out" | sed -n 's/^ifs_words=//p')
if [ "$ifs_words" = "2" ]; then
    ok "跑完补全后默认 IFS 分词仍正常（\"a b\" → 2 个词）"
else
    bad "IFS 被改坏了：\"a b\" 分成 ${ifs_words:-?} 个词（应为 2）"
fi

echo "── 6. 值候选按前缀过滤（case 认完整选项名，不认前缀）──"
out=$(probe 0 yshjy --encoding "")
line=$(printf '%s\n' "$out" | sed -n 1p)
if python3 -c 'import sys; p=sys.argv[1].split("\x1f"); sys.exit(0 if "utf-8" in p and all("--" not in x for x in p[1:]) else 1)' "$line"; then
    ok "--encoding<TAB> 补出 utf-8 这类候选（没被文件名兜底抢走）"
else
    bad "--encoding<TAB> 没补出预期的候选"
    printf '%s\n' "$line" | tr '\037' '\n' | sed 's/^/      /'
fi

echo "── 7. complete 注册项（nosort / 函数名）──"
reg=$(bash -c "source '$COMP' && complete -p yshjy" 2>&1)
case "$reg" in
    *"-o nosort"*"-F _yshjy_complete"*) ok "注册带 -o nosort 与 -F _yshjy_complete" ;;
    *) bad "注册项不对：$reg" ;;
esac

echo "── 8. 补全函数不改调用者的 COMP_WORDS / COMP_CWORD ──"
check=$(bash -c "
    source '$COMP'
    COMP_WORDS=(yshjy \"有\"); COMP_CWORD=1; COMP_LINE='yshjy 有'; COMP_POINT=7
    _yshjy_complete
    printf '%s|%s|%s\n' \"\${#COMP_WORDS[@]}\" \"\${COMP_WORDS[1]}\" \"\$COMP_CWORD\"
" 2>/dev/null)
if [ "$check" = "2|有|1" ]; then
    ok "COMP_WORDS / COMP_CWORD 原样"
else
    bad "COMP_WORDS / COMP_CWORD 被改动了：[$check]（期望 2|有|1）"
fi

echo
echo "── 结果：通过 $n_pass · 失败 $n_fail ──"
[ "$n_fail" -eq 0 ] || exit 1
