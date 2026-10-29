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
    "--decoy": ("none", None),                  # 段数，无候选
    "--decoy-size": ("none", None),             # 如 16K / 1M，无候选
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

# ── man 页用：help.text 里没有说明的选项，在这里补 ──
#
# 为什么不能直接改 help.text：`--help` 是"常用选项"清单，有些选项（--extract-cover、
# --caps、--sfx 之类）只出现在「用法」和「例子」段落里，没有选项行。
# man 页要覆盖**全部**选项，所以缺的那些在这里补齐（这些说明只在 man 里出现）。
MAN_DESC_OVERRIDES = {
    # 这几个在 help.text 里只出现在「用法」段（`yshjy --list <文件>` 那种写法），
    # 解析正则抓不到，所以在这里补齐
    "--list": "只列出包里的内容（ZIP / 7z / RAR / TAR / 单流都支持），不解压、不写盘。",
    "--rescue": "救援模式：中央目录坏了也按每个文件的头逐个抢救。",
    "--report": "生成诊断包（报障用）。包里不含文件内容，命令行里的密码会被抹掉。",
    "--no-paths": "配合 --report：把路径换成占位符，可以直接贴到公开的地方。",
    "--help": "显示这份帮助。",
    "--version": "显示版本、档位（常用版 / 全能版）与支持的格式。",
    "--extract-cover": "只把伪装包里的原版视频/图片切出来，不解压内层内容。",
    "--password-file": "密码本文件：每行一个，`#` 开头是注释。只对这次命令有效。",
    "--no-auto-password": "不自动从「说明.txt」和密码本里找解压码。",
    "--keep": "打包时保留中间 ZIP（排错用）。",
    "--caps": "显示各格式的能力表（能解 / 能打包 / 加密 / 分卷 / 续传）。",
    "--no-checksum": "打包时不生成 .sha256 校验和（默认生成）。",
    "--no-readme": "打包时不生成 .说明.txt（默认生成；自解压包本来就不生成）。",
    "--sfx": "打包成 Windows 自解压包（.exe），双击自己解开。",
    "--sfx-linux": "打包成 Linux 自解压包（.sh），双击自己解开。",
    "--no-color": "不上色（管道 / 日志友好）。",
    "--verbose": "详细输出（等同于 -v；-vv 更详细）。",
    "--config": "用指定的配置文件（默认 ~/.config/yshjy/config.toml）。",
    "--log": "把日志写进文件（报错时发给开发者）。",
    "--cover": "用自己的图片/视频当封面（默认用内置素材）。",
    "--layout": "打包布局：L1 尾部追加 / L2 藏进 free 原子（默认 L2）。",
    "--level": "打包压缩级别 0-9：0 = 仅存储（最快），9 = 最小体积（默认 6）。",
    "--split": "打包时分卷（如 2G / 500M，也认纯字节数）。",
}

# man 页的分组：遇到这些选项时插入一个 .SS 小标题
MAN_GROUPS = [
    ("--split", "打包（把目录做成伪装包）"),
    ("--json", "输出与配置"),
    ("--report", "诊断与报障"),
    ("--help", "帮助与版本"),
]


def load_version() -> str:
    """从 Cargo.toml 取版本（man 页的 .TH 行不许再手写 —— 历史上落后过三个版本）"""
    m = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M)
    return m.group(1) if m else "0.0.0"


def man_escape(s: str) -> str:
    """groff 里的行首 `.` / `'` 会被当成宏，行内的反斜杠要转义"""
    s = s.replace("\\", "\\\\")
    return ("\\&" + s) if s[:1] in (".", "'") else s


def gen_man(opts: list[dict], descs: dict[str, str], version: str, date: str) -> tuple[str, list[str]]:
    """生成 man 页。返回 (内容, 缺说明的选项列表)"""
    tmpl = (ROOT / "docs" / "yshjy.1.tmpl").read_text(encoding="utf-8")
    blocks: list[str] = []
    no_desc: list[str] = []
    public = [o for o in opts if not o["internal"]]

    for o in public:
        for marker, title in MAN_GROUPS:
            if o["long"] == marker:
                blocks.append(f".SS {title}")

        desc = descs.get(o["long"]) or MAN_DESC_OVERRIDES.get(o["long"], "")
        if not desc:
            no_desc.append(o["long"])
            desc = "（本选项暂无说明）"

        name = ", ".join(x for x in [o["short"], o["long"]] if x)
        if o["value"]:
            name += f" \\fI{o['value']}\\fR"
        blocks += [".TP", f".B {name}", man_escape(desc)]

    out = tmpl.replace("{{OPTIONS}}", "\n".join(blocks))
    out = out.replace("{{OPTIONS_COUNT}}", str(len(public)))
    out = out.replace("{{VERSION}}", version)
    out = out.replace("{{DATE}}", date)
    return out, no_desc


def norm_man(s: str | None) -> str:
    """比对 man 页前，只把 `.TH` 行里的**生成日期**抹掉。

    为什么只抹日期：早先是整行抹成 `.TH NORMALIZED`，于是**版本号也一起被抹掉** ——
    版本从 0.9.26 升到 0.9.27 后，生成器算出来「和旧的一样」直接跳过写盘，
    man 页永远停在旧版本号，而 `tests/cli_spec_match.sh` 会红（生成器却说没事）。
    日期每天变才需要归一化；版本是内容，必须参与比对。
    """
    if not s:
        return ""
    return re.sub(r'^(\.TH\s+\S+\s+\S+\s+)"[^"]*"', r'\1"NORMALIZED"', s, flags=re.M)


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
            # 这里**保留完整说明**（含「（默认：…）」）—— man 页需要它；
            # 补全里嫌长，由 short_desc() 现裁。
            out.setdefault(long, desc)
    return out


def short_desc(d: str) -> str:
    """补全里只放一句话：剥掉「（默认…）」这类尾巴"""
    return re.sub(r"\s*（默认[^）]*）", "", d).strip()


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
            '            if declare -F _filedir >/dev/null; then _filedir -d; else mapfile -t COMPREPLY < <(compgen -d -- "$cur"); fi',
            "            return 0",
            "            ;;",
        ]
    if none_:
        lines += [
            f"        {'|'.join(none_)})",
            "            # 这几项故意不给候选（密码不该进 history、序号/大小无候选）。",
            "            # 但加了 `-o default` 之后，空 COMPREPLY 会被 readline 回落成文件名补全 ——",
            "            # 于是 `--password <TAB>` 冒出一串文件名。这里显式关掉回落（bash ≥ 4.4 才有 compopt）。",
            "            compopt +o default 2>/dev/null || true",
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
            '            if declare -F _filedir >/dev/null; then _filedir; else mapfile -t COMPREPLY < <(compgen -f -- "$cur"); fi',
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
        '        mapfile -t COMPREPLY < <(compgen -f -- "$cur")',
        "    fi",
        "}",
        "",
        "# 用 mapfile 而不是 COMPREPLY=($(compgen …))：命令替换会按 IFS 再分词，",
        "# 带空格的文件名（「有 空格.zip」）被劈成两个候选。实测 `IFS=$'\\n' COMPREPLY=(…)`",
        "# 这种前缀写法虽然不劈词，但会在补全函数返回后**留下全局 IFS**，把后续命令的分词全搞坏。",
        "# -o nosort 要 bash ≥ 4.4（老版本会当成非法选项直接报错，所以要判版本）；",
        "# bashdefault/default 让「本来就没候选」以外的位置回落到默认补全，",
        "# 故意不给候选的选项（--password / --select）在函数里已 compopt +o default 关掉回落。",
        "if ((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4))); then",
        "    complete -o filenames -o nosort -o bashdefault -o default -F _yshjy_complete yshjy yshys",
        "else",
        "    complete -o filenames -o bashdefault -o default -F _yshjy_complete yshjy yshys",
        "fi",
        "",
    ]
    return "\n".join(lines)


def gen_zsh(opts: list[dict], descs: dict[str, str]) -> str:
    body = []
    for o in opts:
        if o["internal"]:
            continue
        desc = zn_comments(short_desc(descs.get(o["long"], "")))
        spec = f"[{desc}]" if desc else ""
        if o["value"]:
            action, cand = VALUE_ACTIONS.get(o["long"], ("file", None))
            if action == "dir":
                # 只补目录：`_directories` 在有些 zsh 补全主题里不认，`_files -/`
                # 是 clap 生成器用的写法，跨版本更稳（2026-10-29 抄 clap）
                tail = ":目录:_files -/"
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
                # 只补目录：fish 的 `-F` 会把文件也列进候选，`(__fish_complete_directories)`
                # 才是「只要目录」。⚠️ 必须双引号 —— 单引号里 fish 不执行命令替换
                parts.append('-r -a "(__fish_complete_directories)"')
            elif action == "file":
                parts.append("-r -F")
            elif action == "words":
                parts.append("-x -a " + q(" ".join(cand or [])))
            else:  # none
                parts.append("-x")
        desc = short_desc(descs.get(o["long"], ""))
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

    version = load_version()
    # 日期写进 .TH；--check 模式会归一化掉这一行，否则每天都会「假红」
    date = __import__("datetime").date.today().isoformat()
    man_content, no_desc = gen_man(opts, descs, version, date)
    if no_desc:
        print("⚠️  这些选项没有说明（help.text 与 MAN_DESC_OVERRIDES 里都没有）：" + " ".join(no_desc))

    # 重复计数型短选项（-v / -vv）也要进 bash 的候选
    bash = gen_bash(opts, descs)
    if repeat:
        bash = bash.replace(" jy ys\"", " " + " ".join(repeat) + ' jy ys"')

    targets = {
        OUT_DIR / "yshjy.bash": bash,
        OUT_DIR / "_yshjy": gen_zsh(opts, descs),
        OUT_DIR / "yshjy.fish": gen_fish(opts, descs),
        ROOT / "docs" / "yshjy.1": man_content,
    }

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    changed = []
    for p, content in targets.items():
        old = p.read_text(encoding="utf-8") if p.exists() else None
        # man 页的 .TH 行含生成日期 —— 比对时归一化掉，否则每天第一次跑都会「假红」
        if p.suffix == ".1":
            # 只归一化日期（版本号要参与比对，见 norm_man 的说明）
            same = norm_man(old) == norm_man(content)
        else:
            same = old == content
        if not same:
            changed.append(p.name)
            if not check:
                p.write_text(content, encoding="utf-8")

    if check:
        if changed:
            die(
                "生成物与 CLI 表不一致（" + "、".join(changed) + "）\n"
                "   跑一次：python3 scripts/gen-cli-assets.py"
            )
        print("✅ 补全脚本与 man 页都与 CLI 表一致")
        return

    if changed:
        print("✅ 已生成：" + "、".join(changed))
    else:
        print("✅ 本来就是最新的，没改动")


if __name__ == "__main__":
    main()
