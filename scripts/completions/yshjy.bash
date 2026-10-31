# ⚠️ 本文件由 scripts/gen-cli-assets.py 生成，**别手改** —— 改了下次打包就被覆盖。
# 要加/改选项：改 src/cli_spec.rs 的 OPTS 表，然后跑一次生成器。
# bash 补全 —— yshjy（杨三海解压）
#
# 装法（用户级，不用 sudo）：
#   cp scripts/completions/yshjy.bash ~/.local/share/bash-completion/completions/yshjy
# 或者由 install.sh / deb 包自动装。
#
# 注意：`_filedir` 来自 bash-completion 包；没有它就退回 `compgen -f`。

_yshjy_complete() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local opts="-l --list --rescue --extract-cover -o --dest -p --password --password-file --no-auto-password --no-auto-password-notes --no-auto-password-book --encoding --select --candidate --pick -j --jobs -f --force --test --no-smart-dir --keep --trash-source --delete-source --split --format --tweak-time --level --layout --decoy --decoy-size --decoy-signatures --cover --tail --no-checksum --no-readme --sfx --sfx-linux --json -q --quiet --no-color --verbose --log --lang --config --show-config --caps --import-passwords --report --no-paths -h --help -V --version -v -vv jy ys"

    case "$prev" in
        -o|--dest)
            if declare -F _filedir >/dev/null; then _filedir -d; else mapfile -t COMPREPLY < <(compgen -d -- "$cur"); fi
            return 0
            ;;
        -p|--password|--select|--split|--level|--decoy|--decoy-size)
            # 这几项故意不给候选（密码不该进 history、序号/大小无候选）。
            # 但加了 `-o default` 之后，空 COMPREPLY 会被 readline 回落成文件名补全 ——
            # 于是 `--password <TAB>` 冒出一串文件名。这里显式关掉回落（bash ≥ 4.4 才有 compopt）。
            compopt +o default 2>/dev/null || true
            COMPREPLY=()
            return 0
            ;;
        --encoding)
            COMPREPLY=($(compgen -W "utf-8 gbk big5 sjis" -- "$cur"))
            return 0
            ;;
        -j|--jobs)
            COMPREPLY=($(compgen -W "1 2 4 8" -- "$cur"))
            return 0
            ;;
        --format)
            COMPREPLY=($(compgen -W "zip tar.gz tar.zst" -- "$cur"))
            return 0
            ;;
        --layout)
            COMPREPLY=($(compgen -W "L1 L2" -- "$cur"))
            return 0
            ;;
        --lang)
            COMPREPLY=($(compgen -W "zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES" -- "$cur"))
            return 0
            ;;
        --password-file|--candidate|--cover|--log|--config|--import-passwords)
            if declare -F _filedir >/dev/null; then _filedir; else mapfile -t COMPREPLY < <(compgen -f -- "$cur"); fi
            return 0
            ;;
    esac

    if [[ "$cur" == -* ]]; then
        COMPREPLY=($(compgen -W "$opts" -- "$cur"))
        return 0
    fi

    # 其余位置：补文件/目录（包、目录都能解；ys 打包时目录更常见）
    if declare -F _filedir >/dev/null; then
        _filedir
    else
        mapfile -t COMPREPLY < <(compgen -f -- "$cur")
    fi
}

# 用 mapfile 而不是 COMPREPLY=($(compgen …))：命令替换会按 IFS 再分词，
# 带空格的文件名（「有 空格.zip」）被劈成两个候选。实测 `IFS=$'\n' COMPREPLY=(…)`
# 这种前缀写法虽然不劈词，但会在补全函数返回后**留下全局 IFS**，把后续命令的分词全搞坏。
# -o nosort 要 bash ≥ 4.4（老版本会当成非法选项直接报错，所以要判版本）；
# bashdefault/default 让「本来就没候选」以外的位置回落到默认补全，
# 故意不给候选的选项（--password / --select）在函数里已 compopt +o default 关掉回落。
if ((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4))); then
    complete -o filenames -o nosort -o bashdefault -o default -F _yshjy_complete yshjy yshys
else
    complete -o filenames -o bashdefault -o default -F _yshjy_complete yshjy yshys
fi
