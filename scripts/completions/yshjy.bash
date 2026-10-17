# bash 补全 —— yshjy（杨三海解压）
#
# 装法（用户级，不用 sudo）：
#   cp scripts/completions/yshjy.bash ~/.local/share/bash-completion/completions/yshjy
# 或者由 install.sh 自动装。
#
# 注意：`_filedir` 来自 bash-completion 包；没有它就退回 `compgen -f`。

_yshjy_complete() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local opts="-h --help --version -q --quiet -l --list --rescue --report --no-paths
        --json --show-config --tail --force --no-smart-dir -o --dest -p --password
        -j --jobs --select --lang --import-passwords jy ys"

    case "$prev" in
        -o|--dest)
            if declare -F _filedir >/dev/null; then _filedir -d; else COMPREPLY=($(compgen -d -- "$cur")); fi
            return 0
            ;;
        -p|--password)
            # 密码不补全（也不该出现在 history/日志里）
            COMPREPLY=()
            return 0
            ;;
        -j|--jobs)
            COMPREPLY=($(compgen -W "1 2 4 8" -- "$cur"))
            return 0
            ;;
        --lang)
            COMPREPLY=($(compgen -W "zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES" -- "$cur"))
            return 0
            ;;
        --select)
            # 序号 / 通配 / required，没有可枚举的候选
            COMPREPLY=()
            return 0
            ;;
        --import-passwords)
            if declare -F _filedir >/dev/null; then _filedir; else COMPREPLY=($(compgen -f -- "$cur")); fi
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
        COMPREPLY=($(compgen -f -- "$cur"))
    fi
}

complete -o filenames -F _yshjy_complete yshjy yshys
