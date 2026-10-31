#!/usr/bin/env python3
"""探针：打包布局 L3「放在文件头部」（T18-1 第二项，2026-11-01 做）

背景：L3 = 把 ZIP 包成 `free` 原子，插在 MP4 的 `ftyp` 之后。与 L4 的三处差别
（`refs/analysis/05-压缩布局验证.md` §3「L3」）：

  | 项 | L4 插在视频中间 | L3 放在文件头部 |
  |---|---|---|
  | 插点 | 挑 chunk 边界 | **固定 `ftyp` 末尾** |
  | `mdat.size` | `+= 插入长度` | **一个字节都不改** |
  | 偏移补偿 | 只补 `>= 插点` 的 | **全表都 `+插入长度`** |

代价：ZIP 的 EOCD 离文件尾极远 → `unzip` / Windows 资源管理器 / 手机解压器
全都找不到；只有 7-Zip（从文件头扫）和本工具能解。**所以本探针不但要验 7z 能解，
还要验 unzip 确实解不开** —— 后者是预期行为，不是 bug；哪天它突然能解了，
说明「包其实还在尾部」，L3 根本没生效。

断言（全部真二进制、真文件）：
   1. `--layout L3` → 报告的布局是「放在文件头部」（不是悄悄降级）
   2. 产物结构：`ftyp`(28) 后紧跟 `free` 头，size = 8 + ZIP 大小，ZIP 起点 = 36
   3. `mdat.size` 与原封面**逐字节相同**（L3 不往 mdat 里插）
   4. 顶层盒子首尾相接、最后一个正好到文件尾
   5. ffmpeg 解码干净（rc==0 且 **stderr 空**）
   6. 自家 `--list` 认得出两个文件
   7. 解出来的内容与源文件 sha256 逐一相等
   8. 7-Zip 能列出（从文件头扫，不受尾部窗口限制）
   9. `unzip` **必须失败**（预期：EOCD 在 118 KB 以外，超出它的尾部窗口）
  10. 图片外壳 + L3 → 降级到「尾部追加」并说明原因
  11. L3 + 干扰数据 → **允许**，噪声写在文件末尾（视频仍可播、包仍可解）
  12. 同一份输入打两次逐字节一致（噪声种子只看输入路径）
  13. 协议 `pack.layout=head` → 回 `layout: "HeadFreeAtom"`；乱写 → BAD_PARAM
  14. `--tail` 与 `--layout L3` 一起给 → USAGE(2)
  15. L7 / L8 → 还没实现，USAGE(2) 且零产物

用法：python3 tests/l3_proto.py [二进制路径]
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

COVER = ROOT / "tests" / "assets" / "pack" / "cover5.mp4"
COVER_PNG = ROOT / "tests" / ".work" / "l3_proto_cover.png"

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


def 顶层盒子(data: bytes):
    """切出顶层 MP4 盒子：返回 [(偏移, 大小, 类型)]；结构坏了就停（不抛）"""
    出 = []
    p = 0
    while p + 8 <= len(data):
        size = int.from_bytes(data[p:p + 4], "big")
        类型 = data[p + 4:p + 8]
        if size == 1 and p + 16 <= len(data):
            size = int.from_bytes(data[p + 8:p + 16], "big")
        if size < 8 or p + size > len(data):
            break
        出.append((p, size, 类型))
        p += size
    return 出, p


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
    tmp = Path(tempfile.mkdtemp(prefix="yshjy_l3_"))
    src1 = tmp / "a.txt"
    src2 = tmp / "b.bin"
    src1.write_text("hello L3\n", encoding="utf-8")
    src2.write_bytes(os.urandom(20000))

    print(f"引擎：{BIN}\n临时目录：{tmp}\n")

    # ── ① 真打一个 L3 包 ──────────────────────────────────────────────
    print("① 打包 L3（把包放文件头部）")
    out = tmp / "l3.mp4"
    rc, so, se = run(["ys", str(src1), str(src2), "-o", str(out),
                      "--cover", str(COVER), "--layout", "L3",
                      "--no-readme", "--no-checksum"], tmp)
    if rc != 0 or not out.exists():
        bad(f"打包失败 rc={rc}：{(se or so).strip()[:300]}")
        return 1
    ok("打包成功")
    if "放在文件头部" in so:
        ok("报告的布局就是「放在文件头部」（不是悄悄降级）")
    else:
        bad(f"报告的布局不对：{so.strip()[-200:]}")

    # ── ② 产物结构（字节级）──────────────────────────────────────────
    print("\n② 产物结构")
    原 = COVER.read_bytes()
    产 = out.read_bytes()
    原盒, _ = 顶层盒子(原)
    新盒, 新尾 = 顶层盒子(产)
    原ftyp = next(b for b in 原盒 if b[2] == b"ftyp")
    原mdat = next(b for b in 原盒 if b[2] == b"mdat")
    ftyp末尾 = 原ftyp[0] + 原ftyp[1]
    产_ftyp = next(b for b in 新盒 if b[2] == b"ftyp")
    产_free = next((b for b in 新盒 if b[2] == b"free"), None)
    产_mdat = next(b for b in 新盒 if b[2] == b"mdat")

    if 产_ftyp[0] == 0 and 产_ftyp[1] == 原ftyp[1]:
        ok(f"ftyp 原样保留（{原ftyp[1]} 字节）")
    else:
        bad(f"ftyp 变了：{产_ftyp} vs 原 {原ftyp}")

    if 产_free is not None and 产_free[0] == ftyp末尾:
        ok(f"ftyp 后面紧跟 free 原子（offset={ftyp末尾}）")
    else:
        bad(f"ftyp 后面不是 free 原子：{产_free}")

    # ZIP 起点 = ftyp 末尾 + free 头 8
    zip_起点 = ftyp末尾 + 8
    if 产[zip_起点:zip_起点 + 4] == b"PK\x03\x04":
        ok(f"ZIP 正好从 {zip_起点} 开始（PK\\x03\\x04）")
    else:
        bad(f"{zip_起点} 处不是 PK 头：{产[zip_起点:zip_起点 + 8]!r}")

    # free 原子的 size 字段 = 8 + ZIP 大小（★ 这里差 8 字节就是「能打开但满屏坏帧」那种坏包）
    if 产_free is not None:
        free_size = int.from_bytes(产[产_free[0]:产_free[0] + 4], "big")
        eocd = 产.find(b"PK\x05\x06", zip_起点)
        if eocd < 0:
            bad("产物里找不到 ZIP 的 EOCD")
        else:
            注释长 = int.from_bytes(产[eocd + 20:eocd + 22], "little")
            zip大小 = eocd + 22 + 注释长 - zip_起点
            if free_size == 8 + zip大小:
                ok(f"free 原子 size={free_size} = 8 + ZIP {zip大小}（头长算对了）")
            else:
                bad(f"free 原子 size={free_size} ≠ 8 + ZIP {zip大小} —— 头长算错，偏移会偏")
            # free 原子结束处必须正好是下一个顶层盒子（原封面的 free / mdat）
            下一个 = next((b for b in 新盒 if b[0] == 产_free[0] + free_size), None)
            if 下一个 is not None:
                ok(f"free 原子结束处正好接上 {下一个[2].decode(errors='replace')} 盒子")
            else:
                bad(f"free 原子结束于 {产_free[0] + free_size}，那里不是盒子起点")

    # ★ mdat.size 一个字节都不该动
    if 产_mdat[1] == 原mdat[1]:
        ok(f"mdat.size 没变（{原mdat[1]}）—— L3 不往 mdat 里插东西")
    else:
        bad(f"mdat.size 被改了：{原mdat[1]} → {产_mdat[1]}（L4 才该改它）")

    if 新尾 == len(产):
        ok(f"顶层盒子首尾相接、最后一个盒子正好到文件尾（{len(产)} B）")
    else:
        bad(f"顶层结构不自洽：盒子拼到 {新尾}，文件 {len(产)} B")

    # ── ③ ffmpeg 干净 ────────────────────────────────────────────────
    print("\n③ 视频还能不能播（rc==0 且 stderr 空）")
    got = 解码检查(out, tmp)
    if got is None:
        skip("ffmpeg 解码检查", "本机没有 ffmpeg")
    else:
        rc2, err = got
        if rc2 == 0 and not err.strip():
            ok("ffmpeg rc=0 且 stderr 一个字都没有")
        elif rc2 == 0:
            bad(f"rc=0 但 stderr 有内容 = 画面在坏：{err.strip()[:300]}")
        else:
            bad(f"ffmpeg 打不开 rc={rc2}：{err.strip()[:300]}")

    # ── ④ 自家认得出 ─────────────────────────────────────────────────
    print("\n④ 自家工具认不认")
    rc3, so3, se3 = run(["--list", str(out)], tmp)
    if rc3 == 0 and "a.txt" in so3 and "b.bin" in so3:
        ok("--list 列出了两个文件")
    else:
        bad(f"--list 不认 rc={rc3}：{(se3 or so3).strip()[:300]}")

    # ── ⑤ 解出来逐字节一致 ───────────────────────────────────────────
    print("\n⑤ 解出来的内容")
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

    # ── ⑥ 7-Zip 能解（从文件头扫）────────────────────────────────────
    print("\n⑥ 7-Zip（会从文件头扫，不受 64 KiB 尾部窗口限制）")
    if not shutil.which("7z"):
        skip("7z 能找到包", "本机没有 7z")
    else:
        p = subprocess.run(["7z", "l", str(out)], capture_output=True, text=True)
        if p.returncode == 0 and "a.txt" in p.stdout and "b.bin" in p.stdout:
            ok("7z 正常列出两个文件")
        else:
            bad(f"7z 找不到包 rc={p.returncode}：{p.stdout[-300:]}")

    # ── ⑦ unzip 必须失败（预期行为）──────────────────────────────────
    print("\n⑦ unzip 必须解不开（这正是 L3 的代价）")
    if not shutil.which("unzip"):
        skip("unzip 失败断言", "本机没有 unzip")
    else:
        p = subprocess.run(["unzip", "-l", str(out)], capture_output=True, text=True)
        if p.returncode != 0:
            ok(f"unzip 果然找不到包（rc={p.returncode}）—— EOCD 在尾部窗口之外")
        else:
            bad("unzip 居然解开了 = 包其实还在尾部，L3 没生效")

    # ── ⑧ 图片外壳 → 降级 ────────────────────────────────────────────
    print("\n⑧ 图片外壳 + L3 → 必须降级并说明")
    造png(COVER_PNG)
    out_png = tmp / "img.png"
    rc6, so6, se6 = run(["ys", str(src1), "-o", str(out_png),
                         "--cover", str(COVER_PNG), "--layout", "L3",
                         "--no-readme", "--no-checksum"], tmp)
    if rc6 != 0:
        bad(f"图片外壳打包失败 rc={rc6}：{(se6 or so6).strip()[:200]}")
    else:
        if "已自动改用" in so6 and "尾部追加" in so6:
            ok("说明白了「放不进文件头部，已改成尾部追加」")
        else:
            bad(f"降级了却没说 / 说错了：{so6.strip()[-200:]}")
        rc6b, so6b, _ = run(["--list", str(out_png)], tmp)
        if rc6b == 0 and "a.txt" in so6b:
            ok("降级后的包照样能解")
        else:
            bad("降级后的包解不开")

    # ── ⑨ L3 + 干扰数据 → 允许，噪声在末尾 ──────────────────────────
    print("\n⑨ L3 + 干扰数据（3a：允许，噪声写在文件末尾）")
    out_dc = tmp / "dc.mp4"
    rc7, so7, se7 = run(["ys", str(src1), str(src2), "-o", str(out_dc),
                         "--cover", str(COVER), "--layout", "L3",
                         "--decoy", "4", "--decoy-size", "16384",
                         "--no-readme", "--no-checksum"], tmp)
    if rc7 != 0 or not out_dc.exists():
        bad(f"L3 + 噪声被拒了（应当允许）rc={rc7}：{(se7 or so7).strip()[:200]}")
    else:
        ok("打成功了（噪声没有切断视频）")
        # 噪声总量 = 4 × 16 KiB = 65536，必须落在文件末尾
        噪声 = 4 * 16384
        if out_dc.stat().st_size == out.stat().st_size + 噪声:
            ok(f"产物正好多了 {噪声} B（噪声全在尾部）")
        else:
            bad(f"产物大小 {out_dc.stat().st_size} ≠ 基础 {out.stat().st_size} + {噪声}")
        got2 = 解码检查(out_dc, tmp)
        if got2 is None:
            skip("带噪声的产物 ffmpeg 检查", "本机没有 ffmpeg")
        else:
            rc2b, err2 = got2
            if rc2b == 0 and not err2.strip():
                ok("带噪声的产物 ffmpeg 仍然 rc=0 且 stderr 空")
            else:
                bad(f"带噪声后视频坏了 rc={rc2b}：{err2.strip()[:300]}")
        rc7b, so7b, _ = run([str(out_dc), "-o", str(tmp / "dc_out"), "-q"], tmp)
        if rc7b == 0:
            got_files = {p.name: sha256(p) for p in (tmp / "dc_out").rglob("*") if p.is_file()}
            if got_files.get("a.txt") == sha256(src1):
                ok("带噪声的包照样解得对")
            else:
                bad(f"带噪声后解出来的不对：{got_files}")
        else:
            bad(f"带噪声后解不开 rc={rc7b}")

    # ── ⑩ 可复现（同一份输入两次逐字节一致）──────────────────────────
    print("\n⑩ 同一份输入打两次要逐字节一致")
    out_again = tmp / "l3_again.mp4"
    run(["ys", str(src1), str(src2), "-o", str(out_again),
         "--cover", str(COVER), "--layout", "L3",
         "--no-readme", "--no-checksum"], tmp)
    if out_again.exists() and sha256(out_again) == sha256(out):
        ok("两次产物 sha256 相同（种子只看输入路径，不看时间）")
    else:
        bad("两次产物不一样 —— 有随机量没走稳定种子")

    # ── ⑪ 协议层 ────────────────────────────────────────────────────
    print("\n⑪ 协议 pack.layout=head")
    r = 协议({"id": 1, "cmd": "pack", "inputs": [str(src1)], "out": str(tmp / "proto.mp4"),
              "cover": str(COVER), "layout": "head",
              "readme": False, "checksum": False})
    if r and r.get("ok") and (r.get("result") or {}).get("layout") == "HeadFreeAtom":
        ok("layout=head → 结果回 HeadFreeAtom")
    else:
        bad(f"协议没认 head：{str(r)[:300]}")

    r = 协议({"id": 2, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto_bad.mp4"), "cover": str(COVER),
              "layout": "文件头部", "readme": False, "checksum": False})
    if r and not r.get("ok"):
        ok(f"乱写布局 → 报错（{r.get('error', {}).get('code')}）")
    else:
        bad(f"乱写布局居然过了：{str(r)[:300]}")

    # ── ⑫ --tail 与 --layout 打架 ────────────────────────────────────
    print("\n⑫ --tail 和 --layout L3 一起给")
    rc9, so9, se9 = run(["ys", str(src1), "-o", str(tmp / "both.mp4"),
                         "--tail", "--layout", "L3",
                         "--no-readme", "--no-checksum"], tmp)
    if rc9 == USAGE:
        ok("USAGE(2)：两个自相矛盾的说法只能留一个")
    else:
        bad(f"没拦住 rc={rc9}（会变成「谁最后写谁赢」的两套优先级）")

    # ── ⑬ 还没实现的布局 ────────────────────────────────────────────
    print("\n⑬ L7 / L8 还没实现")
    for 代号 in ("L7", "L8"):
        p = tmp / f"bad_{代号}.mp4"
        rc8, so8, se8 = run(["ys", str(src1), "-o", str(p), "--layout", 代号,
                             "--no-readme", "--no-checksum"], tmp)
        if rc8 == USAGE and not p.exists():
            ok(f"{代号} → USAGE(2) 且零产物")
        else:
            bad(f"{代号} 没被拦住：rc={rc8} 产物={p.exists()}")

    print(f"\n通过 {n_pass} / 失败 {n_fail} / 跳过 {n_skip}")
    if n_fail:
        print("失败项：")
        return 1
    print("全部通过 ✅")
    return 0


if __name__ == "__main__":
    sys.exit(main())
