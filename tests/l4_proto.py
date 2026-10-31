#!/usr/bin/env python3
"""探针：打包布局 L4「插在视频中间」（T18-1 的第一项，2026-10-31 做）

背景：引擎 `PackLayout` 以前只有「尾部追加」和「free 原子」两种。L4 要
**把 ZIP 插进 mdat 的两条 chunk 之间**，而且必须同时做三件事，缺一条就坏
（实测见 `refs/analysis/05-压缩布局验证.md` §4）：

  | 做到哪一步 | ffmpeg 的结果 |
  |---|---|
  | 只插不改 | rc=183，打不开 |
  | 只改 mdat size | rc=0，但 stderr 满屏 `Decoding error`（坏帧） |
  | 只补 stco | rc=183 |
  | 插在 chunk 内部 + 两边都改 | rc=0，但 `marker does not match f_code` |
  | **插在 chunk 边界 + 两边都改** | **rc=0 且 stderr 为空** |

★ 所以本探针的核心断言不是「rc==0」，而是「**rc==0 且 stderr 一个字都没有**」。
   只看返回码会让「只改 mdat size」那种坏包也过验收 —— 这是本项最大的假绿温床。

断言（全部真二进制、真文件）：
   1. `--layout L4` + 带音轨封面 → 产物布局是「插在视频中间」
   2. ffmpeg 解码干净（rc==0 且 stderr 空）
   3. 自家 `--list` 认得出，并且说的是「包藏在文件中间」
   4. 解出来的内容与源文件 sha256 逐一相等
   5. 7-Zip 也能找到这个包（它从文件头扫，不受 64 KiB 尾部窗口限制）；没装 7z 就跳过
   6. 图片外壳 + L4 → **降级**到「尾部追加」并说明原因（绝不能写出一个播不了的假视频）
   7. L4 + 干扰数据 → 拒绝（噪声会把视频数据切断）
   8. L5 / L6 → 拒绝（实测判死的两种：解压残废 / 视频播不了）；L3 / L7 / L8 → 还没实现
   9. `--tail` 与 `--layout L4` 一起给 → USAGE(2)（两个自相矛盾的说法只能留一个）
  10. 协议 `pack.layout=mid` → 回 `layout: "MidChunk"`；乱写 → BAD_PARAM
  11. 协议 `tail:true` + `layout:mid` → BAD_PARAM（同一个坑，两个入口都要堵）

用法：python3 tests/l4_proto.py [二进制路径]
"""
from __future__ import annotations

import hashlib
import json
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
if os.sep in BIN:
    BIN = os.path.abspath(BIN)

# 带音轨的封面：101 条 chunk 边界，插点才有意义
# （单轨封面只有 1 条 chunk，插在边界和插在数据里没区别，验不出真本事）
COVER = ROOT / "tests" / "assets" / "pack" / "cover5.mp4"
COVER_PNG = ROOT / "tests" / ".work" / "l4_proto_cover.png"

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


def 造png(path):
    """造一个 40×40 的纯色 PNG（不依赖 ImageMagick）"""
    import struct
    import zlib

    def chunk(t, d):
        return (struct.pack(">I", len(d)) + t + d
                + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF))

    w = h = 40
    raw = b"".join(b"\x00" + b"\x87\xce\xfa" * w for _ in range(h))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


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


def 解码检查(video, tmp):
    """ffmpeg 完整解码一遍，返回 (rc, stderr)。没装 ffmpeg 返回 None。"""
    if not shutil.which("ffmpeg"):
        return None
    err = tmp / "ff.err"
    with open(err, "w") as f:
        p = subprocess.run(["ffmpeg", "-v", "error", "-i", str(video), "-f", "null", "-"],
                           stdout=subprocess.DEVNULL, stderr=f, timeout=300)
    return p.returncode, err.read_text(errors="replace")


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="yshjy_l4_"))
    src1 = tmp / "a.txt"
    src2 = tmp / "b.bin"
    src1.write_text("hello L4\n", encoding="utf-8")
    src2.write_bytes(os.urandom(20000))

    print(f"引擎：{BIN}\n临时目录：{tmp}\n")

    # ── ① 真打一个 L4 包 ──────────────────────────────────────────────
    print("① 打包 L4（带音轨封面）")
    out = tmp / "l4.mp4"
    rc, so, se = run(["ys", str(src1), str(src2), "-o", str(out),
                      "--cover", str(COVER), "--layout", "L4",
                      "--no-readme", "--no-checksum"], tmp)
    if rc != 0 or not out.exists():
        bad(f"打包失败 rc={rc}：{(se or so).strip()[:300]}")
        return 1
    ok("打包成功")
    if "插在视频中间" in so:
        ok("报告的布局就是「插在视频中间」（不是悄悄降级）")
    else:
        bad(f"报告的布局不对：{so.strip()[-200:]}")

    # ── ② ffmpeg 干净（本项最硬的验收）────────────────────────────────
    print("\n② 视频还能不能播（rc==0 且 stderr 空）")
    got = 解码检查(out, tmp)
    if got is None:
        skip("ffmpeg 解码检查", "本机没有 ffmpeg")
    else:
        rc2, err = got
        if rc2 == 0 and not err.strip():
            ok("ffmpeg rc=0 且 stderr 一个字都没有（不是「rc=0 但满屏坏帧」）")
        elif rc2 == 0:
            bad(f"rc=0 但 stderr 有内容 = 画面在坏：{err.strip()[:300]}")
        else:
            bad(f"ffmpeg 打不开 rc={rc2}：{err.strip()[:300]}")

    # ── ③ 自家认得出 ─────────────────────────────────────────────────
    print("\n③ 自家工具认不认")
    rc3, so3, se3 = run(["--list", str(out)], tmp)
    if rc3 == 0 and "a.txt" in so3 and "b.bin" in so3:
        ok("--list 列出了两个文件")
    else:
        bad(f"--list 不认 rc={rc3}：{(se3 or so3).strip()[:300]}")
    if "中间" in so3:
        ok("并且说明了「包藏在文件中间」")
    else:
        bad(f"没提示包在中间，用户会以为它坏了：{so3.strip()[:200]}")

    # ── ④ 解出来逐字节一致 ───────────────────────────────────────────
    print("\n④ 解出来的内容")
    dest = tmp / "out"
    rc4, so4, se4 = run([str(out), "-o", str(dest), "-q"], tmp)
    if rc4 != 0:
        bad(f"解压失败 rc={rc4}：{(se4 or so4).strip()[:300]}")
    else:
        found = {p.name: sha256(p) for p in dest.rglob("*") if p.is_file()}
        want = {src1.name: sha256(src1), src2.name: sha256(src2)}
        if all(found.get(k) == v for k, v in want.items()):
            ok(f"两个文件 sha256 与源文件一致（{len(found)} 个文件）")
        else:
            bad(f"内容不一致：解出 {found} / 期望 {want}")

    # ── ⑤ 7-Zip 也能找到（尾部窗口限制的对照）──────────────────────────
    print("\n⑤ 7-Zip（从文件头扫，不受 64 KiB 尾部窗口限制）")
    if not shutil.which("7z"):
        skip("7z 能找到包", "本机没有 7z")
    else:
        p = subprocess.run(["7z", "l", str(out)], capture_output=True, text=True)
        if p.returncode == 0 and "a.txt" in p.stdout and "b.bin" in p.stdout:
            ok("7z 正常列出两个文件")
        else:
            bad(f"7z 找不到包 rc={p.returncode}：{p.stdout[-300:]}")

    # ── ⑥ 图片外壳 → 降级 ────────────────────────────────────────────
    print("\n⑥ 图片外壳 + L4 → 必须降级并说明")
    造png(COVER_PNG)
    out_png = tmp / "img.png"
    rc6, so6, se6 = run(["ys", str(src1), "-o", str(out_png),
                         "--cover", str(COVER_PNG), "--layout", "L4",
                         "--no-readme", "--no-checksum"], tmp)
    if rc6 != 0:
        bad(f"图片外壳打包失败 rc={rc6}：{(se6 or so6).strip()[:200]}")
    else:
        if "已自动改用" in so6 and "尾部追加" in so6:
            ok("说明白了「插不进中间，已改成尾部追加」")
        else:
            bad(f"降级了却没说 / 说错了：{so6.strip()[-200:]}")
        rc6b, so6b, _ = run(["--list", str(out_png)], tmp)
        if rc6b == 0 and "a.txt" in so6b:
            ok("降级后的包照样能解")
        else:
            bad("降级后的包解不开")

    # ── ⑦ L4 + 干扰数据 → 拒绝 ───────────────────────────────────────
    print("\n⑦ L4 + 干扰数据 → 拒绝（噪声会切断视频数据）")
    rc7, so7, se7 = run(["ys", str(src1), "-o", str(tmp / "dc.mp4"),
                         "--layout", "L4", "--decoy", "4",
                         "--no-readme", "--no-checksum"], tmp)
    if rc7 != 0 and (tmp / "dc.mp4").exists() is False:
        ok(f"拒绝了，且零产物（rc={rc7}）")
    elif rc7 != 0:
        ok(f"拒绝了（rc={rc7}）")
    else:
        bad("居然打成功了 —— 打出来的是坏视频")

    # ── ⑧ 判死/未实现的布局要明确拒 ──────────────────────────────────
    print("\n⑧ 别的布局编号")
    for 代号, 说明 in [("L5", "分散多段（实测 7-Zip 只解出 1/4）"),
                       ("L6", "前后加垃圾（视频直接播不了）"),
                       ("L3", "ftyp 后 free（本轮还没做）"),
                       ("L7", "两个包（本轮还没做）"),
                       ("L8", "MKV 附件（本轮还没做）")]:
        p = tmp / f"bad_{代号}.mp4"
        rc8, so8, se8 = run(["ys", str(src1), "-o", str(p), "--layout", 代号,
                             "--no-readme", "--no-checksum"], tmp)
        if rc8 == USAGE and not p.exists():
            ok(f"{代号} → USAGE(2) 且零产物（{说明}）")
        else:
            bad(f"{代号} 没被拦住：rc={rc8} 产物={p.exists()}")

    # ── ⑨ --tail 与 --layout 打架 ────────────────────────────────────
    print("\n⑨ --tail 和 --layout L4 一起给")
    rc9, so9, se9 = run(["ys", str(src1), "-o", str(tmp / "both.mp4"),
                         "--tail", "--layout", "L4",
                         "--no-readme", "--no-checksum"], tmp)
    if rc9 == USAGE:
        ok("USAGE(2)：两个自相矛盾的说法只能留一个")
    else:
        bad(f"没拦住 rc={rc9}（会变成「谁最后写谁赢」的两套优先级）")

    # ── ⑩ 协议层 ────────────────────────────────────────────────────
    print("\n⑩ 协议 pack.layout")
    out_p = tmp / "proto.mp4"
    r = 协议({"id": 1, "cmd": "pack", "inputs": [str(src1)], "out": str(out_p),
              "cover": str(COVER), "layout": "mid",
              "readme": False, "checksum": False})
    if r and r.get("ok") and (r.get("result") or {}).get("layout") == "MidChunk":
        ok("layout=mid → 结果回 MidChunk")
    else:
        bad(f"协议没认 mid：{str(r)[:300]}")

    r = 协议({"id": 2, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto_bad.mp4"), "cover": str(COVER),
              "layout": "分散多段", "readme": False, "checksum": False})
    if r and not r.get("ok"):
        ok(f"乱写布局 → 报错（{r.get('error', {}).get('code')}）")
    else:
        bad(f"乱写布局居然过了：{str(r)[:300]}")

    r = 协议({"id": 3, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto_both.mp4"), "cover": str(COVER),
              "layout": "mid", "tail": True, "readme": False, "checksum": False})
    if r and not r.get("ok"):
        ok("tail:true + layout:mid → BAD_PARAM（同一个坑两个入口都堵）")
    else:
        bad(f"协议层没拦住自相矛盾：{str(r)[:300]}")

    print(f"\n通过 {n_pass} / 失败 {n_fail} / 跳过 {n_skip}")
    if n_fail:
        print("失败项：")
        return 1
    print("全部通过 ✅")
    return 0


if __name__ == "__main__":
    sys.exit(main())
