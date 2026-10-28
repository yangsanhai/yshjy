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

    local opts="-l --list --rescue --extract-cover -o --dest -p --password --password-file --no-auto-password --encoding --select -j --jobs -f --force --no-smart-dir --keep --split --level --layout --cover --tail --no-checksum --no-readme --sfx --sfx-linux --json -q --quiet --no-color --verbose --log --lang --config --show-config --caps --import-passwords --report --no-paths -h --help -V --version -v -vv jy ys"

    case "$prev" in
        -o|--dest)
            if declare -F _filedir >/dev/null; then _filedir -d; else COMPREPLY=($(compgen -d -- "$cur")); fi
            return 0
            ;;
        -p|--password|--select|--split|--level)
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
        --layout)
            COMPREPLY=($(compgen -W "L1 L2" -- "$cur"))
            return 0
            ;;
        --lang)
            COMPREPLY=($(compgen -W "zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES" -- "$cur"))
            return 0
            ;;
        --password-file|--cover|--log|--config|--import-passwords)
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
