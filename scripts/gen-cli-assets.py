#!/usr/bin/env python3
"""从 CLI 选项表生成 shell 补全脚本。

## 唯一真源

`src/cli_spec.rs` 的 `OPTS` 表。本脚本**不解析 Rust 源码**，而是让二进制自己
吐 JSON（`yshjy --dump-cli-spec`），解析源码是最脆的做法。

说明文字取自 `locales/zh-CN.toml` 的 `help.text`（本来就是用户看到的那份文案），
所以补全里的说明和 `--help` 永远一致，不会出现第二份文案。

## 用法

    python3 scripts/gen-cli-assets.py            # 生成到 scripts/completions/
    python3 scripts/gen-cli-assets.py --check    # 只检查是否已同步（不一致退出 1）

`tests/cli_spec_match.sh` 用 --check 模式对账，`scripts/build-deb.sh` 打包前会重生成。
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "scripts" / "completions"

# ── 「这个选项的值怎么补全」——属于用户界面知识，不在 CLI 结构表里，所以放这
#
# 缺省（没登记的带值选项）= 补文件，这是最安全的默认；登记了就用下面的动作。
VALUE_ACTIONS = {
    "--dest": ("dir", None),                    # 目录
    "--password": ("none", None),               # 密码不补全（也不该进 history）
    "--select": ("none", None),                 # 序号 / 通配 / required，无候选
    "--split": ("none", None),                  # 如 2G / 500M，无候选
    "--level": ("none", None),                  # 0-9
    "--jobs": ("words", ["1", "2", "4", "8"]),
    "--layout": ("words", ["L1", "L2"]),
    "--encoding": ("words", ["utf-8", "gbk", "big5", "sjis"]),
    "--lang": ("words", ["zh-CN", "zh-TW", "en-US", "ja-JP", "ko-KR", "ru-RU", "es-ES"]),
    "--password-file": ("file", None),
    "--import-passwords": ("file", None),
    "--cover": ("file", None),
    "--config": ("file", None),
    "--log": ("file", None),
}

HEADER = (
    "# ⚠️ 本文件由 scripts/gen-cli-assets.py 生成，**别手改** —— 改了下次打包就被覆盖。\n"
    "# 要加/改选项：改 src/cli_spec.rs 的 OPTS 表，然后跑一次生成器。\n"
)


def die(msg: str) -> NoReturn:
    sys.exit(f"❌ {msg}")


def newest_source_mtime() -> float:
    """src/ 下最新的 mtime —— 用来发现「二进制比源码旧」这个坑"""
    src = ROOT / "src"
    return max((p.stat().st_mtime for p in src.rglob("*.rs")), default=0.0)


def load_spec() -> dict:
    """跑二进制拿 JSON。优先 release；两个都试，谁支持 --dump-cli-spec 用谁"""
    src_mtime = newest_source_mtime()
    for rel in ("target/release/yshjy", "target/debug/yshjy"):
        p = ROOT / rel
        if not p.exists():
            continue
        if p.stat().st_mtime < src_mtime:
            print(f"⚠️  {rel} 比 src/ 里的源码旧，先 `cargo build` 再生成（跳过这个二进制）")
            continue
        r = subprocess.run([str(p), "--dump-cli-spec"], capture_output=True, text=True)
        if r.returncode == 0 and r.stdout.lstrip().startswith("{"):
            return json.loads(r.stdout)
        print(f"⚠️  {rel} 不支持 --dump-cli-spec（可能是旧二进制），跳过")
    die("找不到能用的 yshjy 二进制，先跑 `cargo build` 或 `cargo build --release`")


def load_help_text(lang: str = "zh-CN") -> str:
    """取 help.text（用户在 --help 里看到的同一份文案）"""
    p = ROOT / "locales" / f"{lang}.toml"
    if not p.exists():
        return ""
    txt = p.read_text(encoding="utf-8")
    m = re.search(r'^text\s*=\s*"""(.*?)"""', txt, re.S | re.M)
    return m.group(1) if m else ""


def parse_descriptions(help_text: str) -> dict[str, str]:
    """从 help.text 里抠出「选项 → 一句话说明」。

    行样例：
      `  -o, --dest <目录>               解压到哪（默认：当前目录下的同名文件夹）`
      `  --json                          按行输出事件流/结果（给脚本用）`
    跨行续述只取第一行（补全里放一句话就够）。
    """
    out: dict[str, str] = {}
    pat = re.compile(r"^\s+(?:-[A-Za-z],\s+)?(--[a-z][a-z0-9-]*)(?:\s+<[^>]+>)?\s{2,}(\S.*)$")
    for line in help_text.splitlines():
        m = pat.match(line)
        if m:
            long, desc = m.group(1), m.group(2).strip()
            # 去掉末尾的「（默认…）」之类，补全里太长
            desc = re.sub(r"\s*（默认[^）]*）", "", desc).strip()
            out.setdefault(long, desc)
    return out


def q(s: str) -> str:
    """单引号包裹（shell 单引号里除了 ' 都是字面量）"""
    return "'" + s.replace("'", "'\\''") + "'"


def zn_comments(desc: str) -> str:
    """zsh _arguments 用 [说明]，方括号里不能有未转义的 ] """
    return desc.replace("[", "\\[").replace("]", "\\]")


def gen_bash(opts: list[dict], descs: dict[str, str]) -> str:
    flags = []
    for o in opts:
        if o["internal"]:
            continue
        if o["short"]:
            flags.append(o["short"])
        flags.append(o["long"])

    lines = [
        HEADER.rstrip(),
        "# bash 补全 —— yshjy（杨三海解压）",
        "#",
        "# 装法（用户级，不用 sudo）：",
        "#   cp scripts/completions/yshjy.bash ~/.local/share/bash-completion/completions/yshjy",
        "# 或者由 install.sh / deb 包自动装。",
        "#",
        "# 注意：`_filedir` 来自 bash-completion 包；没有它就退回 `compgen -f`。",
        "",
        "_yshjy_complete() {",
        "    local cur prev",
        '    cur="${COMP_WORDS[COMP_CWORD]}"',
        '    prev="${COMP_WORDS[COMP_CWORD-1]}"',
        "",
        '    local opts="' + " ".join(flags) + ' jy ys"',
        "",
        '    case "$prev" in',
    ]

    # 值补全：先处理特殊动作，再看是否「要文件」
    dirs, files, none_, words = [], [], [], []
    for o in opts:
        if o["internal"] or not o["value"]:
            continue
        action, cand = VALUE_ACTIONS.get(o["long"], ("file", None))
        names = [x for x in (o["short"], o["long"]) if x]
        key = "|".join(names)
        if action == "dir":
            dirs.append(key)
        elif action == "file":
            files.append(key)
        elif action == "none":
            none_.append(key)
        elif action == "words":
            words.append((key, cand or []))

    if dirs:
        lines += [
            f"        {'|'.join(dirs)})",
            '            if declare -F _filedir >/dev/null; then _filedir -d; else COMPREPLY=($(compgen -d -- "$cur")); fi',
            "            return 0",
            "            ;;",
        ]
    if none_:
        lines += [
            f"        {'|'.join(none_)})",
            "            COMPREPLY=()",
            "            return 0",
            "            ;;",
        ]
    for key, cand in words:
        lines += [
            f"        {key})",
            f'            COMPREPLY=($(compgen -W "{" ".join(cand)}" -- "$cur"))',
            "            return 0",
            "            ;;",
        ]
    if files:
        lines += [
            f"        {'|'.join(files)})",
            '            if declare -F _filedir >/dev/null; then _filedir; else COMPREPLY=($(compgen -f -- "$cur")); fi',
            "            return 0",
            "            ;;",
        ]

    lines += [
        "    esac",
        "",
        '    if [[ "$cur" == -* ]]; then',
        '        COMPREPLY=($(compgen -W "$opts" -- "$cur"))',
        "        return 0",
        "    fi",
        "",
        "    # 其余位置：补文件/目录（包、目录都能解；ys 打包时目录更常见）",
        "    if declare -F _filedir >/dev/null; then",
        "        _filedir",
        "    else",
        '        COMPREPLY=($(compgen -f -- "$cur"))',
        "    fi",
        "}",
        "",
        "complete -o filenames -F _yshjy_complete yshjy yshys",
        "",
    ]
    return "\n".join(lines)


def gen_zsh(opts: list[dict], descs: dict[str, str]) -> str:
    body = []
    for o in opts:
        if o["internal"]:
            continue
        desc = zn_comments(descs.get(o["long"], ""))
        spec = f"[{desc}]" if desc else ""
        if o["value"]:
            action, cand = VALUE_ACTIONS.get(o["long"], ("file", None))
            if action == "dir":
                tail = ":目录:_directories"
            elif action == "none":
                tail = f":{o['value']}:"
            elif action == "words":
                tail = f":{o['value']}:({' '.join(cand or [])})"
            else:
                tail = f":{o['value']}:_files"
        else:
            tail = ""
        if o["short"]:
            body.append(f"        '({o['short']} {o['long']})'{{{o['short']},{o['long']}}}'{spec}{tail}' \\")
        else:
            body.append(f"        '{o['long']}{spec}{tail}' \\")

    lines = [
        "#compdef yshjy yshys",
        HEADER.rstrip(),
        "# zsh 补全 —— yshjy（杨三海解压）",
        "#",
        "# 装法（用户级，不用 sudo）：",
        "#   cp scripts/completions/_yshjy ~/.local/share/zsh/site-functions/_yshjy",
        "",
        "_yshjy() {",
        "    _arguments -s -S \\",
        *body,
        "        '1:要处理的文件或目录（jy=解压 / ys=打包）:_files' \\",
        "        '*:更多文件或目录:_files'",
        "}",
        "",
        '_yshjy "$@"',
        "",
    ]
    return "\n".join(lines)


def gen_fish(opts: list[dict], descs: dict[str, str]) -> str:
    lines = [
        HEADER.rstrip(),
        "# fish 补全 —— yshjy（杨三海解压）",
        "#",
        "# 装法（用户级，不用 sudo）：",
        "#   cp scripts/completions/yshjy.fish ~/.config/fish/completions/yshjy.fish",
        "",
        "# 不自动补全选项的值（文件名由下面的规则补）",
        "complete -c yshjy -f",
        "complete -c yshys -f",
        "",
        "for bin in yshjy yshys",
    ]
    for o in opts:
        if o["internal"]:
            continue
        parts = ["    complete -c $bin"]
        if o["short"]:
            parts.append(f"-s {o['short'].lstrip('-')}")
        parts.append(f"-l {o['long'].lstrip('-')}")
        if o["value"]:
            action, cand = VALUE_ACTIONS.get(o["long"], ("file", None))
            if action == "dir":
                parts.append("-r -F")
            elif action == "file":
                parts.append("-r -F")
            elif action == "words":
                parts.append("-x -a " + q(" ".join(cand or [])))
            else:  # none
                parts.append("-x")
        desc = descs.get(o["long"], "")
        if desc:
            parts.append("-d " + q(desc))
        lines.append(" ".join(parts))
    lines += [
        "    complete -c $bin -f -a 'jy' -d '解压（完整写法）'",
        "    complete -c $bin -f -a 'ys' -d '打包成伪装文件'",
        "    # 位置参数：文件或目录",
        "    complete -c $bin -F",
        "end",
        "",
    ]
    return "\n".join(lines)


def main() -> None:
    check = "--check" in sys.argv
    spec = load_spec()
    opts = spec["options"]
    repeat = spec.get("repeat_shorts", [])
    descs = parse_descriptions(load_help_text())

    # 重复计数型短选项（-v / -vv）也要进 bash 的候选
    targets = {
        "yshjy.bash": gen_bash(opts, descs),
        "_yshjy": gen_zsh(opts, descs),
        "yshjy.fish": gen_fish(opts, descs),
    }
    if repeat:
        targets["yshjy.bash"] = targets["yshjy.bash"].replace(
            " jy ys\"", " " + " ".join(repeat) + ' jy ys"'
        )

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    changed = []
    for name, content in targets.items():
        p = OUT_DIR / name
        old = p.read_text(encoding="utf-8") if p.exists() else None
        if old != content:
            changed.append(name)
            if not check:
                p.write_text(content, encoding="utf-8")

    if check:
        if changed:
            die(
                "补全脚本与 CLI 表不一致（" + "、".join(changed) + "）\n"
                "   跑一次：python3 scripts/gen-cli-assets.py"
            )
        print("✅ 补全脚本与 CLI 表一致")
        return

    if changed:
        print("✅ 已生成：" + "、".join(changed))
    else:
        print("✅ 本来就是最新的，没改动")


if __name__ == "__main__":
    main()
