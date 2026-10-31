#!/usr/bin/env python3
"""T18-2 探针 —— 打包「自定义显示名」(`--name <名称>`)

设计稿 DESIGN §14.2：把包伪装成 `电影.2026.1080p.mp4` 这种名字，**用户填**。
以前引擎 `PackOptions` 没有名字字段、`cli_spec.rs` 里也 grep 不到 `name` —— 三层全缺；
现在 `--name` 只改**文件名**，目录仍由 `-o/--dest` 决定（一条规则只有一个出口）。

断言（全部真二进制、真文件）：
  1. 不给 --name → 老行为一个字都不变（产物仍是「输入名 + .mp4」）
  2. `--name 电影.2026.1080p.mp4` → 产物就是这个名字，且自家 `--list` 能认出
  3. `--name 电影.2026.1080p`（不带后缀）→ 补成 `.mp4`（1080p 不是扩展名）
  4. `--name 备份` + `--format zip` → 补成 `备份.zip`
  5. `--format zip` 却写 `--name 备份.tar.gz` → USAGE(2)：名字内容不一致会骗人
  6. `--name ../evil.mp4` → USAGE(2)，**零产物**（不许把改名变成换位置）
  7. `--name ""` → USAGE(2)
  8. `-o <已存在目录> --name x.mp4` → 落在那个目录里
  9. `-o <已存在目录>`（不给 name）→ 落在目录里 + 默认名（**顺手修掉的老 bug**：
     以前 `-o 目录` 对打包根本不管用，报「建不了输出文件：Is a directory」）
 10. `-o <完整文件名> --name y.mp4` → USAGE(2)（两个出口打架，明确拒绝）
 11. `--name 的值不会被当成待打包的文件名`（带值选项登记的老坑）

用法：python3 tests/name_cli.py [二进制路径]
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("YSHJY_BIN")
       or next((str(ROOT / "target" / rel) for rel in
                ("release/yshjy.exe", "release/yshjy", "debug/yshjy.exe", "debug/yshjy")
                if (ROOT / "target" / rel).exists()), ""))
if not BIN:
    print("❌ 找不到 yshjy 二进制（先 cargo build）")
    sys.exit(1)
# 探针会把子进程 cwd 换到临时目录：相对路径必须绝对化，否则报「引擎不存在」
# （看着像引擎没编出来，其实是探针自己的坑）。纯命令名走 PATH，不动。
if os.sep in BIN:
    BIN = os.path.abspath(BIN)

USAGE = 2

n_pass = n_fail = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✓ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ✗ {msg}")


def run(args, cwd, timeout=180):
    p = subprocess.run([BIN, *args], cwd=cwd, capture_output=True, text=True,
                       timeout=timeout)
    return p.returncode, p.stdout, p.stderr


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="yshjy_name_"))
    try:
        src = tmp / "内容"
        src.mkdir()
        (src / "a.txt").write_text("你好，这是内容\n", encoding="utf-8")
        (tmp / "outdir").mkdir()
        (tmp / "outdir2").mkdir()
        # 探针自己跑在临时目录里（产物默认落在 cwd），绝不动仓库
        work = tmp / "work"
        work.mkdir()
        src_rel = str(src)

        # ── 1. 不给 --name：老行为 ────────────────────────────────────
        rc, out, err = run(["ys", src_rel], work)
        old = work / "内容.mp4"
        if rc == 0 and old.exists():
            ok("不给 --name → 产物仍是「输入名 + .mp4」（老行为没变）")
        else:
            bad(f"不给 --name 的老行为坏了：rc={rc} err={err.strip()[:160]}")
        old.unlink(missing_ok=True)

        # ── 2. 带完整后缀的自定义名 ─────────────────────────────────
        rc, out, err = run(["ys", src_rel, "--name", "电影.2026.1080p.mp4"], work)
        want2 = work / "电影.2026.1080p.mp4"
        if rc == 0 and want2.exists():
            ok("--name 电影.2026.1080p.mp4 → 产物就叫这个名字")
            rc2, _, _ = run(["--list", str(want2)], work)
            if rc2 == 0:
                ok("自定义名的产物能被自家 --list 认出（不只是名字对了）")
            else:
                bad(f"产物名字对了但 --list 认不出（rc={rc2}）—— 解不开的包等于坏了")
        else:
            bad(f"--name 没生效：rc={rc} 产物={[p.name for p in work.iterdir()]} err={err.strip()[:160]}")

        # ── 3. 不带后缀 → 补 .mp4（1080p 不是扩展名）─────────────────
        rc, out, err = run(["ys", src_rel, "--name", "电影.2026.1080p"], work)
        want3 = work / "电影.2026.1080p.mp4"
        if rc == 0 and want3.exists():
            ok("--name 电影.2026.1080p → 补成 .mp4（没把 1080p 当扩展名吃掉）")
        else:
            bad(f"不带后缀的名字没补对：rc={rc} 产物={[p.name for p in work.iterdir()]}")

        # ── 4. 普通压缩：后缀跟着格式补 ─────────────────────────────
        rc, out, err = run(["ys", src_rel, "--format", "zip", "--name", "备份"], work)
        want4 = work / "备份.zip"
        if rc == 0 and want4.exists():
            ok("--format zip + --name 备份 → 备份.zip")
        else:
            bad(f"普通压缩补后缀失败：rc={rc} err={err.strip()[:160]}")

        # ── 5. 名字后缀与格式不一致 → 明确拒绝 ──────────────────────
        rc, out, err = run(["ys", src_rel, "--format", "zip", "--name", "备份.tar.gz"], work)
        if rc == USAGE and "扩展名要跟当前模式一致" in err:
            ok("--format zip 配 --name 备份.tar.gz → USAGE(2) 并说清为什么")
        else:
            bad(f"后缀不一致没被拒：rc={rc} err={err.strip()[:160]}")

        # ── 6. 名字里带路径 → USAGE 且零产物 ────────────────────────
        rc, out, err = run(["ys", src_rel, "--name", "../evil.mp4"], work)
        if rc == USAGE and not (work.parent / "evil.mp4").exists():
            ok("--name ../evil.mp4 → USAGE(2)，没有产物（改名不许变成换位置）")
        else:
            bad(f"带 ../ 的名字没被拒：rc={rc} 越界产物={ (work.parent / 'evil.mp4').exists() }")

        # ── 7. 空名字 ───────────────────────────────────────────────
        rc, out, err = run(["ys", src_rel, "--name", ""], work)
        if rc == USAGE:
            ok('--name "" → USAGE(2)')
        else:
            bad(f"空名字没被拒：rc={rc}")

        # ── 8. -o 目录 + --name → 落在目录里 ────────────────────────
        rc, out, err = run(["ys", src_rel, "-o", str(tmp / "outdir"), "--name", "x.mp4"], work)
        want8 = tmp / "outdir" / "x.mp4"
        if rc == 0 and want8.exists():
            ok("-o <目录> --name x.mp4 → 落在那个目录里")
        else:
            bad(f"-o 目录 + --name 没落对：rc={rc} err={err.strip()[:160]}")

        # ── 9. 只给 -o 目录（老 bug：打包时 -o 目录根本不管用）───────
        rc, out, err = run(["ys", src_rel, "-o", str(tmp / "outdir2")], work)
        want9 = tmp / "outdir2" / "内容.mp4"
        if rc == 0 and want9.exists():
            ok("-o <目录>（不给 name）→ 目录/内容.mp4（打包时 -o 目录终于管用了）")
        else:
            bad(f"-o 目录还是不管用：rc={rc} err={err.strip()[:160]}")

        # ── 10. -o 完整文件名 + --name → 两个出口打架，明确拒绝 ─────
        rc, out, err = run(["ys", src_rel, "-o", str(tmp / "冲突.mp4"), "--name", "y.mp4"], work)
        if rc == USAGE and "只能给一个" in err:
            ok("-o <完整文件名> + --name → USAGE(2) 并说清二选一")
        else:
            bad(f"-o 文件 + --name 没被拒：rc={rc} err={err.strip()[:160]}")

        # ── 11. --name 的值不会被当成输入文件 ──────────────────────
        rc, out, err = run(["ys", src_rel, "--name", "z.mp4"], work)
        only = sorted(p.name for p in work.iterdir())
        if rc == 0 and (work / "z.mp4").exists() and src_rel not in only:
            ok("--name 的值不会被当成待打包的文件名（带值选项登记对）")
        else:
            bad(f"--name 的值被误当输入：rc={rc} work 里={only}")

        shutil.rmtree(tmp, ignore_errors=True)
    finally:
        print(f"\n结果：{n_pass} 通过 / {n_fail} 失败")
    return 0 if n_fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
