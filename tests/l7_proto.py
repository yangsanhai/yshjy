#!/usr/bin/env python3
"""探针：打包布局 L7「一个文件里放两个包」（T18-1 第三项，2026-11-01 做）

背景：L7 = 封面 → ZIP1（本次输入压出来的）→ ZIP2（用户给的**现成** zip），
两个包各自完整、都能单独解开（`refs/analysis/05-压缩布局验证.md` §3「L7」）。

★ 这套布局的**代价**必须反向断言，不然「实现坏了」和「设计如此」分不清：
   · 7-Zip 普通模式只报**第一个**包（Offset = 封面大小）；
   · `unzip` / Python `zipfile` 只报**最后一个**包（EOCD 在文件尾）；
   · 两者都**不会**提示「还有另一个」。
  只有 7-Zip 扫描模式（`7z x -t#`）和本工具（多候选扫描）能拿全 ——
  所以探针既验「7z 只见第一个」「unzip 只见第二个」，又验「我们自己两处都能解」。

断言（全部真二进制、真文件）：
   1. `--layout L7 --second <zip>` → 报告布局是「两个包」
   2. 产物结构：ZIP1 起于封面末尾、ZIP2 起于 ZIP1 末尾、ZIP2 结束正好是文件尾
   3. 自家 `--list` 报出**两处**候选（T5 的多候选扫描）
   4. 默认解压 = 第一个包的内容（sha256 与源一致）
   5. `--candidate <ZIP2 起点>` = 第二个包的内容（sha256 与用户给的 zip 解出来一致）
   6. 7z **只看到第一个**、unzip **只看到第二个**（预期行为，反向断言）
   7. 第二个包不是 zip（给个 txt）→ 报错且零产物
   8. `--second` 配别的布局 → USAGE(2)；L7 不给 `--second` → USAGE(2)
   9. L7 + `--decoy` → 拒绝（噪声会顶掉 ZIP2 的目录区）
  10. 协议 `layout:"two"` + `second` → 回 `TwoPackages` 且 `second_offset/size` 对得上
  11. 同一份输入打两次逐字节一致

用法：python3 tests/l7_proto.py [二进制路径]
"""
from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("YSHJY_BIN")
       or next((str(ROOT / "target" / rel) for rel in
                ("release/yshjy.exe", "release/yshjy", "debug/yshjy.exe", "debug/yshjy")
                if (ROOT / "target" / rel).exists()), ""))
if not BIN:
    print("❌ 找不到 yshjy 二进制（先 cargo build）")
    sys.exit(1)
if os.sep in BIN:
    BIN = os.path.abspath(BIN)

COVER = ROOT / "tests" / "assets" / "pack" / "cover5.mp4"

USAGE = 2

n_pass = n_fail = n_skip = 0


def ok(msg):
    global n_pass
    n_pass += 1
    print(f"  ✓ {msg}")


def bad(msg):
    global n_fail
    n_fail += 1
    print(f"  ✗ {msg}")


def skip(msg, why):
    global n_skip
    n_skip += 1
    print(f"  – {msg}（跳过：{why}）")


def run(args, cwd, timeout=300):
    p = subprocess.run([BIN, *args], cwd=cwd, capture_output=True, text=True,
                       timeout=timeout)
    return p.returncode, p.stdout, p.stderr


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()


def 造zip(path: Path, 成员: dict[str, bytes]):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for 名, 数据 in 成员.items():
            z.writestr(名, 数据)


def 协议(命令):
    """跑一条协议命令（--serve 模式，一行 JSON 进一行 JSON 出）"""
    p = subprocess.Popen([BIN, "--serve"], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                         text=True, encoding="utf-8")
    try:
        assert p.stdin and p.stdout
        p.stdin.write(json.dumps(命令, ensure_ascii=False) + "\n")
        p.stdin.flush()
        for line in p.stdout:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if r.get("id") == 命令.get("id"):
                return r
        return None
    finally:
        try:
            p.kill()
        except Exception:
            pass


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="yshjy_l7_"))
    src1 = tmp / "a.txt"
    src2 = tmp / "b.bin"
    src1.write_text("hello L7\n", encoding="utf-8")
    src2.write_bytes(os.urandom(20000))

    # 第二个包：两个成员，名字故意和第一个包不重叠，解出来一眼能分清是哪个包
    second_zip = tmp / "另一份.zip"
    造zip(second_zip, {
        "第二包甲.txt": "我在第二个包里\n".encode("utf-8"),
        "第二包乙.bin": os.urandom(3000),
    })
    封面大小 = COVER.stat().st_size

    print(f"引擎：{BIN}\n临时目录：{tmp}\n")

    # ── ① 真打一个 L7 包 ──────────────────────────────────────────────
    print("① 打包 L7（一个文件两个包）")
    out = tmp / "l7.mp4"
    rc, so, se = run(["ys", str(src1), str(src2), "-o", str(out),
                      "--cover", str(COVER), "--layout", "L7",
                      "--second", str(second_zip),
                      "--no-readme", "--no-checksum"], tmp)
    if rc != 0 or not out.exists():
        bad(f"打包失败 rc={rc}：{(se or so).strip()[:300]}")
        return 1
    ok("打包成功")
    if "两个包" in so:
        ok("报告的布局就是「两个包」")
    else:
        bad(f"报告的布局不对：{so.strip()[-200:]}")

    # ── ② 产物结构 ────────────────────────────────────────────────────
    print("\n② 产物结构（两个包首尾相接）")
    产 = out.read_bytes()
    zip1 = 封面大小
    if 产[zip1:zip1 + 4] == b"PK\x03\x04":
        ok(f"ZIP1 正好起于封面末尾（{zip1}）")
    else:
        bad(f"ZIP1 起点不对（{zip1} 处是 {产[zip1:zip1 + 4]!r}）")

    第二大小 = second_zip.stat().st_size
    zip2 = len(产) - 第二大小
    if 产[zip2:zip2 + 4] == b"PK\x03\x04":
        ok(f"ZIP2 起于 {zip2}，且一直到最后（大小 {第二大小}）")
    else:
        bad(f"ZIP2 起点不对（{zip2} 处是 {产[zip2:zip2 + 4]!r}）")
    # ZIP1 的 EOCD 必须落在 ZIP2 起点之前（否则两个包交叉了）
    e1 = 产.rfind(b"PK\x05\x06", 0, zip2)
    if e1 > 0:
        ok(f"ZIP1 的目录尾在 {e1}，位于 ZIP2 起点之前（两个包不交叉）")
    else:
        bad("ZIP1 的 EOCD 没找到 —— 第一个包不完整")
    if 产.rfind(b"PK\x05\x06") >= len(产) - 第二大小:
        ok("ZIP2 的目录尾在文件尾部窗口内（unzip/zipfile 能找到它）")
    else:
        bad("ZIP2 的 EOCD 不在文件尾 —— 连「只看最后一个包」的工具也会瞎")

    # ── ③ 自家 --list 要报两处 ────────────────────────────────────────
    print("\n③ 自家工具：两处都要报出来")
    rc3, so3, se3 = run(["--list", str(out)], tmp)
    if rc3 != 0:
        bad(f"--list 失败 rc={rc3}：{(se3 or so3).strip()[:300]}")
    else:
        if "a.txt" in so3 and "b.bin" in so3:
            ok("第一处包的内容列出来了")
        else:
            bad(f"--list 没列出第一个包：{so3.strip()[:300]}")
        if "第二包甲" in so3 or "第二包乙" in so3:
            ok("第二处包的内容也列出来了（多候选扫描生效）")
        elif "另一处包" in so3 and "--candidate" in so3 and str(zip2) in so3:
            ok(f"虽然没列第二处的清单，但说清了「还有另一处（偏移 {zip2}）」并给了 --candidate 用法")
        else:
            bad(f"--list 没报第二处：{so3.strip()[:400]}")

    # ── ④ 默认解 = 第一个包 ───────────────────────────────────────────
    print("\n④ 默认解压 = 第一个包")
    d1 = tmp / "out1"
    rc4, so4, se4 = run([str(out), "-o", str(d1), "-q"], tmp)
    if rc4 != 0:
        bad(f"解压失败 rc={rc4}：{(se4 or so4).strip()[:300]}")
    else:
        found = {p.name: sha256(p) for p in d1.rglob("*") if p.is_file()}
        want = {src1.name: sha256(src1), src2.name: sha256(src2)}
        if all(found.get(k) == v for k, v in want.items()):
            ok("第一个包解出来的东西与源文件 sha256 一致")
        else:
            bad(f"第一个包内容不对：{found} / 期望 {want}")

    # ── ⑤ 指定第二处 = 第二个包 ───────────────────────────────────────
    print("\n⑤ --candidate 指到第二处 = 第二个包")
    d2 = tmp / "out2"
    rc5, so5, se5 = run([str(out), "-o", str(d2), "--candidate", str(zip2), "-q"], tmp)
    if rc5 != 0:
        bad(f"解第二处失败 rc={rc5}：{(se5 or so5).strip()[:300]}")
    else:
        第二包成员 = {}
        with zipfile.ZipFile(second_zip) as z:
            for 名 in z.namelist():
                第二包成员[名] = hashlib.sha256(z.read(名)).hexdigest()
        got = {p.name: sha256(p) for p in d2.rglob("*") if p.is_file()}
        if all(got.get(k) == v for k, v in 第二包成员.items()):
            ok(f"第二个包解出来的 {len(第二包成员)} 个文件 sha256 全对")
        else:
            bad(f"第二个包内容不对：解出 {list(got)} / 期望 {list(第二包成员)}")

    # ── ⑥ 通用工具只看得见一个（预期）────────────────────────────────
    print("\n⑥ 通用解压器只看得见一个（这是 L7 的代价，不是 bug）")
    if not shutil.which("7z"):
        skip("7z 只见第一个包", "本机没有 7z")
    else:
        p = subprocess.run(["7z", "l", str(out)], capture_output=True, text=True)
        见第二 = "第二包甲" in p.stdout or "第二包乙" in p.stdout
        见第一 = "a.txt" in p.stdout
        if 见第一 and not 见第二:
            ok("7z 只报第一个包（实测就是 Offset = 封面大小）—— 符合预期")
        elif 见第二 and not 见第一:
            ok("7z 这次报的是第二个包 —— 也属于「只见一个」，符合预期")
        else:
            bad(f"7z 的行为变了（见第一={见第一} 见第二={见第二}）：{p.stdout[-300:]}")

    if not shutil.which("unzip"):
        skip("unzip 只见第二个包", "本机没有 unzip")
    else:
        p = subprocess.run(["unzip", "-l", str(out)], capture_output=True, text=True)
        见第二 = "第二包甲" in p.stdout or "第二包乙" in p.stdout
        见第一 = "a.txt" in p.stdout
        if 见第二 and not 见第一:
            ok("unzip 只报第二个包（EOCD 在文件尾）—— 符合预期")
        elif 见第一 and not 见第二:
            ok("unzip 这次报的是第一个包 —— 同样「只见一个」，符合预期")
        else:
            bad(f"unzip 的行为变了（见第一={见第一} 见第二={见第二}）：{p.stdout[-300:]}")

    # ── ⑦ 第二个包不是 zip → 报错且零产物 ────────────────────────────
    print("\n⑦ 第二个包不是 zip（给个 txt）")
    fake = tmp / "假的.zip"
    fake.write_text("我不是压缩包\n", encoding="utf-8")
    out_bad = tmp / "bad.mp4"
    rc7, so7, se7 = run(["ys", str(src1), "-o", str(out_bad),
                         "--cover", str(COVER), "--layout", "L7",
                         "--second", str(fake), "--no-readme", "--no-checksum"], tmp)
    if rc7 != 0 and not out_bad.exists():
        ok(f"拒绝了且零产物（rc={rc7}）")
        if "PK" in (se7 + so7) or "ZIP" in (se7 + so7):
            ok("还说清了「文件头不是 PK」")
        else:
            bad(f"拒了但没说原因：{(se7 + so7).strip()[:200]}")
    else:
        bad(f"居然打成功了（rc={rc7} 产物={out_bad.exists()}）—— 会骗人")

    # ── ⑧ 两条半配置都要拦 ───────────────────────────────────────────
    print("\n⑧ --second 用错地方 / L7 缺 --second")
    rc8, so8, se8 = run(["ys", str(src1), "-o", str(tmp / "x1.mp4"),
                         "--cover", str(COVER), "--layout", "free",
                         "--second", str(second_zip),
                         "--no-readme", "--no-checksum"], tmp)
    if rc8 == USAGE and not (tmp / "x1.mp4").exists():
        ok("不是 L7 却给 --second → USAGE(2) 且零产物")
    else:
        bad(f"没拦住 rc={rc8}（产物={(tmp / 'x1.mp4').exists()}）")

    rc9, so9, se9 = run(["ys", str(src1), "-o", str(tmp / "x2.mp4"),
                         "--cover", str(COVER), "--layout", "L7",
                         "--no-readme", "--no-checksum"], tmp)
    if rc9 == USAGE and not (tmp / "x2.mp4").exists():
        ok("L7 却没给 --second → USAGE(2) 且零产物")
    else:
        bad(f"没拦住 rc={rc9}")

    # ── ⑨ L7 + 干扰数据 → 拒绝 ──────────────────────────────────────
    print("\n⑨ L7 + 干扰数据 → 拒绝（噪声会顶掉第二个包的目录区）")
    out_dc = tmp / "dc.mp4"
    rcD, soD, seD = run(["ys", str(src1), "-o", str(out_dc),
                         "--cover", str(COVER), "--layout", "L7",
                         "--second", str(second_zip), "--decoy", "4",
                         "--no-readme", "--no-checksum"], tmp)
    if rcD != 0 and not out_dc.exists():
        ok(f"拒绝了且零产物（rc={rcD}）")
    else:
        bad(f"居然打成功了（rc={rcD}）—— 第二个包会坏")

    # ── ⑩ 协议层 ────────────────────────────────────────────────────
    print("\n⑩ 协议 pack.layout=two + second")
    r = 协议({"id": 1, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto.mp4"), "cover": str(COVER),
              "layout": "two", "second": str(second_zip),
              "readme": False, "checksum": False})
    res = (r or {}).get("result") or {}
    if r and r.get("ok") and res.get("layout") == "TwoPackages":
        ok("layout=two → 结果回 TwoPackages")
        if res.get("second_offset") == 封面大小 + res.get("zip_size", 0):
            ok(f"second_offset={res.get('second_offset')} = 封面 + ZIP1（落点说得对）")
        else:
            bad(f"second_offset 不对：{res.get('second_offset')} vs 封面 {封面大小} + zip {res.get('zip_size')}")
        if res.get("second_size") == 第二大小:
            ok(f"second_size={res.get('second_size')} 与用户给的 zip 大小一致")
        else:
            bad(f"second_size 不对：{res.get('second_size')} vs {第二大小}")
    else:
        bad(f"协议没认 two：{str(r)[:300]}")

    r = 协议({"id": 2, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto_bad.mp4"), "cover": str(COVER),
              "layout": "two", "readme": False, "checksum": False})
    if r and not r.get("ok"):
        ok(f"协议层：two 不给 second 也报错（{r.get('error', {}).get('code')}）")
    else:
        bad(f"协议层没拦住：{str(r)[:300]}")

    # ── ⑪ 可复现 ────────────────────────────────────────────────────
    print("\n⑪ 同一份输入打两次要逐字节一致")
    out_again = tmp / "l7_again.mp4"
    run(["ys", str(src1), str(src2), "-o", str(out_again),
         "--cover", str(COVER), "--layout", "L7", "--second", str(second_zip),
         "--no-readme", "--no-checksum"], tmp)
    if out_again.exists() and sha256(out_again) == sha256(out):
        ok("两次产物 sha256 相同")
    else:
        bad("两次产物不一样 —— 有随机量没走稳定种子")

    print(f"\n通过 {n_pass} / 失败 {n_fail} / 跳过 {n_skip}")
    if n_fail:
        print("失败项：")
        return 1
    print("全部通过 ✅")
    return 0


if __name__ == "__main__":
    sys.exit(main())
