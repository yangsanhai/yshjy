#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 yangsanhai（杨三海解压）
"""发布前泄漏扫描：按「内容模式」判定，不靠文件名单。

为什么有它（2026-11-17）：
  之前三次清理都靠手写名单 —— 「这份文件该不该公开」是人类凭记忆列的。
  名单天然只挡得住被想到的：AGENTS.md、项目文档、GUI-PLAN 这些名字长得
  像正经项目文档的内部材料，三次都没被列进去，于是三次都「清理完了」还漏。
  改成反过来：把内部用语、对话痕迹、本机路径写成模式表，扫内容。

词表在库外（私有）：项目目录/.private/yshjy-leak-patterns.txt
  格式：每行 `HARD<TAB>关键词` 或 `WARN<TAB>关键词`，`#` 开头是注释。
  词表刻意不进仓库 —— 它列的那些词本身就是不该公开的内容。

用法：
  python3 scripts/leak-scan.py <目录>              # 扫一遍，有 HARD 命中则退出码 1
  python3 scripts/leak-scan.py <目录> --warn       # 连 WARN 也打印（人工确认用）
  python3 scripts/leak-scan.py <目录> --files      # 只列命中文件，不列行内容

退出码：0 = 无 HARD 命中；1 = 有 HARD 命中；2 = 用法错或词表缺失。
"""
import os
import pathlib
import re
import sys

BINARY_SUFFIXES = {
    '.png', '.jpg', '.jpeg', '.gif', '.ico', '.webp', '.bmp',
    '.zip', '.gz', '.xz', '.zst', '.bz2', '.7z', '.rar', '.tar',
    '.exe', '.dll', '.so', '.dylib', '.a', '.rlib', '.pdb',
    '.apk', '.aar', '.jar', '.class', '.wasm',
    '.ttf', '.otf', '.woff', '.woff2', '.mp3', '.mp4', '.wav',
    '.pdf', '.bin', '.dat', '.lock',
}

PATTERNS_PATH = pathlib.Path.home() / 'hermes' / '.private' / 'yshjy-leak-patterns.txt'


def load_patterns(path):
    if not path.exists():
        print(f'词表不存在：{path}', file=sys.stderr)
        print('（它是库外私有文件，别提交进仓库；找不到就先建一份）', file=sys.stderr)
        sys.exit(2)
    hard, warn = [], []
    for raw in path.read_text(encoding='utf-8').splitlines():
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        level, _, word = line.partition('\t')
        level = level.strip().upper()
        word = word.strip()
        if not word:
            continue
        (hard if level == 'HARD' else warn).append(word)
    return hard, warn


def tracked_files(root):
    """在 git 仓库里只扫「会被发布的东西」= git 跟踪的文件。

    工作区里常有参照源码（refs/）、编译产物、机器生成的 xcconfig —— 它们不进库，
    扫它们只会造出一堆与发布无关的告警（实测 refs/ 下十几个第三方仓库报了 200+ 处）。
    """
    import subprocess
    try:
        out = subprocess.run(['git', '-C', str(root), 'ls-files', '-z'],
                             capture_output=True, check=True).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    return [root / p for p in out.decode('utf-8', 'replace').split('\0') if p]


def scan(root, hard, warn):
    hits = []          # (path, level, word, lineno, text)
    root = pathlib.Path(root).resolve()
    tracked = tracked_files(root)
    if tracked is not None:
        candidates = tracked
    else:
        candidates = []
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ('.git', 'node_modules', 'target')]
            for name in filenames:
                candidates.append(pathlib.Path(dirpath) / name)
    for p in candidates:
        if p.suffix.lower() in BINARY_SUFFIXES:
            continue
        try:
            text = p.read_text(encoding='utf-8', errors='strict')
        except (UnicodeDecodeError, OSError):
            continue
        rel = p.relative_to(root)
        for lineno, line in enumerate(text.splitlines(), 1):
            for word in hard:
                if word in line:
                    hits.append((str(rel), 'HARD', word, lineno, line.strip()[:120]))
            for word in warn:
                if word in line:
                    hits.append((str(rel), 'WARN', word, lineno, line.strip()[:120]))
    return hits


DOC_REF = re.compile(r'(docs/[A-Za-z0-9_\u4e00-\u9fff.-]+\.(?:md|txt|rs|py|sh))')


def dangling_refs(root):
    """找出「指向仓库里不存在的文件」的引用。

    公开库会删掉一部分不随仓库发布的文档，代码注释和历史档案里那些 `项目文档`
    就变成悬空指针 —— 贡献者照着去找会扑空，也会让人以为漏了文件。发布前必须为 0。
    """
    root = pathlib.Path(root).resolve()
    tracked = tracked_files(root) or []
    missing = {}
    for p in tracked:
        if p.suffix.lower() in BINARY_SUFFIXES:
            continue
        try:
            text = p.read_text(encoding='utf-8')
        except (UnicodeDecodeError, OSError):
            continue
        rel = p.relative_to(root)
        for ref in set(DOC_REF.findall(text)):
            if not (root / ref).exists():
                missing.setdefault(ref, set()).add(str(rel))
    return missing


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    root = argv[1]
    if '--refs' in argv:
        missing = dangling_refs(root)
        if not missing:
            print('通过：悬空引用 0 处')
            return 0
        print(f'不通过：{len(missing)} 个文件引用不存在的东西')
        for ref, fs in sorted(missing.items(), key=lambda kv: -len(kv[1])):
            print(f'  {ref}  <- {len(fs)} 个文件：' + ', '.join(sorted(fs)[:4]))
        return 1
    show_warn = '--warn' in argv
    only_files = '--files' in argv
    hard, warn = load_patterns(PATTERNS_PATH)
    hits = scan(root, hard, warn)

    hard_hits = [h for h in hits if h[1] == 'HARD']
    warn_hits = [h for h in hits if h[1] == 'WARN']

    def dump(items, title):
        if not items:
            return
        print(f'\n{title}：{len(items)} 处，涉及 {len({i[0] for i in items})} 个文件')
        if only_files:
            for f in sorted({i[0] for i in items}):
                n = sum(1 for i in items if i[0] == f)
                print(f'  {f}  ({n})')
            return
        by_file = {}
        for f, lv, w, ln, tx in items:
            by_file.setdefault(f, []).append((lv, w, ln, tx))
        for f in sorted(by_file):
            print(f'\n  {f}')
            for lv, w, ln, tx in by_file[f][:6]:
                print(f'    {ln:>5}  [{lv}] {w}  | {tx}')
            if len(by_file[f]) > 6:
                print(f'    … 另有 {len(by_file[f]) - 6} 处')

    dump(hard_hits, '❌ HARD 命中（不许发布）')
    if show_warn:
        dump(warn_hits, '⚠️  WARN 命中（人工确认）')

    if hard_hits:
        print(f'\n结论：不通过 —— HARD {len(hard_hits)} 处。')
        return 1
    print(f'\n结论：通过 —— HARD 0 处'
          + (f'；WARN {len(warn_hits)} 处（人工看过再发）' if show_warn else ''))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
