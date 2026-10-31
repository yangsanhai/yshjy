#!/usr/bin/env python3
"""探针：打包布局 L8「MKV 附件轨」（T18-1 第四项，2026-11-01 做）

背景：L8 = 借系统 ffmpeg 把封面视频**重封装**成 MKV（不转码），ZIP 原文塞进
MKV 的 `Attachments/AttachedFile/FileData`（mimetype=application/zip）。
配方与实测见 `refs/analysis/05-压缩布局验证.md` §3 L8。

★ 这套布局的判据**不能只看返回码**：
  · `ffmpeg` 封装完返回 0，不代表产物能播 → 必须再解码一遍且 **stderr 为空**
    （L3 那次「rc=0 但音频全坏」的教训）；
  · 产物是 MKV 但名字叫 .mp4 也是骗人 → 后缀必须 .mkv，写错要报错；
  · 产物必须**逐字节可复现** → ffmpeg 得带 `-fflags +bitexact`（否则 SegmentUID 随机）。

断言（全部真二进制、真文件）：
   1. 打包成功，报告布局是「MKV 附件」
   2. 产物开头是 EBML 魔数 `1a 45 df a3`
   3. `ffmpeg -f null` 解码 **rc=0 且 stderr 为空**
   4. `ffprobe` 三条流：video + audio + attachment
   5. 7-Zip 能定位（Offset > 0）并列出成员
   6. `unzip` **找不到**（这是 L8 的代价，反向断言 —— 它突然能找到说明结构变了）
   7. 我们自己的 `--list` 能列
   8. 我们自己的解压：sha256 与源一致
   9. `ffmpeg -dump_attachment:t:0` 写出的就是原 ZIP（成员一致）
  10. 同一份输入打两次逐字节一致
  11. `-o x.mp4` → USAGE(2) 且零产物；`-o 裸名` → 自动补 `.mkv`
  12. L8 + `--decoy` → 拒绝且零产物
  13. 协议 `layout:"mkv"` → 回 `MkvAttachment`
  14. **没装 ffmpeg → 如实降级**（把 PATH 指到空目录模拟）：产物不是 MKV，
      且结果里明确说「没装 ffmpeg」

用法：python3 tests/l8_proto.py [二进制路径]
"""
from __future__ import annotations

import hashlib
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
EBML = b"\x1aE\xdf\xa3"
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


def run(args, cwd, timeout=300, env=None):
    p = subprocess.run([BIN, *args], cwd=cwd, capture_output=True, text=True,
                       timeout=timeout, env=env)
    return p.returncode, p.stdout, p.stderr


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()


def 协议(命令):
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
    for 工具 in ("ffmpeg", "ffprobe"):
        if not shutil.which(工具):
            print(f"❌ 本机没有 {工具} —— L8 的探针没法跑（引擎也会自动降级）")
            return 1
    if not COVER.exists():
        print(f"❌ 缺测试封面 {COVER}")
        return 1

    tmp = Path(tempfile.mkdtemp(prefix="yshjy_l8_"))
    src1 = tmp / "a.txt"
    src2 = tmp / "b.bin"
    src1.write_text("hello L8\n", encoding="utf-8")
    src2.write_bytes(os.urandom(6000))

    print(f"引擎：{BIN}\n临时目录：{tmp}\n")

    # ── ① 真打一个 L8 包 ──────────────────────────────────────────────
    print("① 打包 L8（封装成 MKV，ZIP 进附件轨）")
    out = tmp / "out.mkv"
    rc, so, se = run(["ys", str(src1), str(src2), "-o", str(out),
                      "--cover", str(COVER), "--layout", "L8",
                      "--no-readme", "--no-checksum"], tmp)
    if rc != 0 or not out.exists():
        bad(f"打包失败 rc={rc}：{(se or so).strip()[:400]}")
        return 1
    ok("打包成功")
    if "MKV 附件" in so:
        ok("报告的布局就是「MKV 附件」")
    else:
        bad(f"报告的布局不对：{so.strip()[-200:]}")

    # ── ② 产物是 MKV ──────────────────────────────────────────────────
    print("\n② 产物结构（EBML 魔数）")
    头 = out.read_bytes()[:4]
    if 头 == EBML:
        ok(f"文件头是 EBML（{头.hex()}）—— 这确实是个 MKV")
    else:
        bad(f"文件头不对：{头.hex()}")

    # ── ③ ★ 核心判据：解码 rc=0 且 stderr 空 ──────────────────────────
    print("\n③ ffmpeg 完整解码（★ 判据是 rc=0 **且** stderr 为空）")
    p = subprocess.run(["ffmpeg", "-v", "error", "-i", str(out), "-f", "null", "-"],
                       capture_output=True, text=True)
    if p.returncode != 0:
        bad(f"解码失败 rc={p.returncode}：{p.stderr.strip()[:400]}")
    elif p.stderr.strip():
        bad(f"rc=0 但 stderr 有内容（假绿的典型）：{p.stderr.strip()[:400]}")
    else:
        ok("解码 rc=0，stderr 一个字都没有 —— 视频/音频是干净的")

    # ── ④ ffprobe 三条流 ─────────────────────────────────────────────
    print("\n④ ffprobe：视频 + 音频 + 附件")
    p = subprocess.run(["ffprobe", "-v", "error", "-show_entries",
                        "stream=codec_type", "-of", "csv=p=0", str(out)],
                       capture_output=True, text=True)
    类型 = [x.strip() for x in p.stdout.split() if x.strip()]
    if "video" in 类型 and "audio" in 类型 and "attachment" in 类型:
        ok(f"三条流都在：{类型}")
    else:
        bad(f"流不对：{类型}（期望 video/audio/attachment）")

    # ── ⑤ 7-Zip 能定位 ───────────────────────────────────────────────
    print("\n⑤ 7-Zip 能看见包")
    if not shutil.which("7z"):
        skip("7z 能定位并列出成员", "本机没有 7z")
    else:
        p = subprocess.run(["7z", "l", str(out)], capture_output=True, text=True)
        有成员 = "a.txt" in p.stdout and "b.bin" in p.stdout
        有偏移 = "Offset = 540" in p.stdout or "Offset =" in p.stdout
        if 有成员 and 有偏移:
            ok("7-Zip 报出 Offset 且列出了成员")
        else:
            bad(f"7-Zip 看不到包：{p.stdout[-400:]}")

    # ── ⑥ ★ 代价反向断言：unzip 找不到 ───────────────────────────────
    print("\n⑥ unzip 找不到（这是 L8 的代价，不是 bug）")
    if not shutil.which("unzip"):
        skip("unzip 找不到包", "本机没有 unzip")
    else:
        p = subprocess.run(["unzip", "-l", str(out)], capture_output=True, text=True)
        if p.returncode != 0 and "cannot find zipfile directory" in (p.stdout + p.stderr):
            ok("unzip 明确说找不到 —— 符合预期（EOCD 离文件尾 89 KB，超出它的 64 KiB 窗口）")
        else:
            bad(f"unzip 的行为变了（rc={p.returncode}）—— 结构可能被改坏了：{p.stdout[-300:]}")

    # ── ⑦ 我们自己列得出 ─────────────────────────────────────────────
    print("\n⑦ 我们自己的 --list")
    rc7, so7, se7 = run(["--list", str(out)], tmp)
    if rc7 == 0 and "a.txt" in so7 and "b.bin" in so7:
        ok("本工具能从 MKV 里把包列出来")
    else:
        bad(f"--list 失败 rc={rc7}：{(se7 or so7).strip()[:400]}")

    # ── ⑧ 我们自己解得回来 ───────────────────────────────────────────
    print("\n⑧ 我们自己的解压（sha256）")
    d = tmp / "got"
    rc8, so8, se8 = run([str(out), "-o", str(d), "-q"], tmp)
    if rc8 != 0:
        bad(f"解压失败 rc={rc8}：{(se8 or so8).strip()[:400]}")
    else:
        got = {p.name: sha256(p) for p in d.rglob("*") if p.is_file()}
        want = {src1.name: sha256(src1), src2.name: sha256(src2)}
        if all(got.get(k) == v for k, v in want.items()):
            ok("两个文件 sha256 全对")
        else:
            bad(f"内容不对：{got}")

    # ── ⑨ dump_attachment 拿到的就是原 ZIP ───────────────────────────
    print("\n⑨ ffmpeg -dump_attachment 取出附件")
    dump = tmp / "dump.zip"
    # `-dump_attachment` 是**输入侧**选项，必须写在 -i 之前（写后面会被当成输出选项）
    p = subprocess.run(["ffmpeg", "-v", "error", "-y",
                        "-dump_attachment:t:0", str(dump),
                        "-i", str(out), "-f", "null", "-"],
                       capture_output=True, text=True)
    if not dump.exists():
        bad(f"没取出附件 rc={p.returncode}：{p.stderr.strip()[:300]}")
    else:
        try:
            with zipfile.ZipFile(dump) as z:
                名 = sorted(z.namelist())
            if "a.txt" in 名 and "b.bin" in 名:
                ok(f"附件就是原 ZIP（成员 {名}）")
            else:
                bad(f"附件成员不对：{名}")
        except Exception as e:
            bad(f"附件不是合法 ZIP：{e}")

    # ── ⑩ 可复现 ────────────────────────────────────────────────────
    print("\n⑩ 同一份输入打两次要逐字节一致（靠 -fflags +bitexact）")
    out2 = tmp / "out2.mkv"
    run(["ys", str(src1), str(src2), "-o", str(out2),
         "--cover", str(COVER), "--layout", "L8",
         "--no-readme", "--no-checksum"], tmp)
    if out2.exists() and sha256(out2) == sha256(out):
        ok("两次产物 sha256 相同（没写随机 SegmentUID）")
    else:
        bad("两次产物不一样 —— +bitexact 没生效")

    # ── ⑪ 后缀规则 ──────────────────────────────────────────────────
    print("\n⑪ 后缀：写错要拦，不写要补")
    rcA, soA, seA = run(["ys", str(src1), "-o", str(tmp / "bad.mp4"),
                         "--cover", str(COVER), "--layout", "L8",
                         "--no-readme", "--no-checksum"], tmp)
    if rcA == USAGE and not (tmp / "bad.mp4").exists():
        ok("-o bad.mp4 → USAGE(2) 且零产物（内容是 MKV 就不能叫 .mp4）")
        if ".mkv" in (seA + soA):
            ok("还说清了「要以 .mkv 结尾」")
        else:
            bad(f"拒了但没说清后缀要求：{(seA + soA).strip()[:200]}")
    else:
        bad(f"没拦住 rc={rcA}（产物存在={(tmp / 'bad.mp4').exists()}）—— 会骗人")

    rcB, soB, seB = run(["ys", str(src1), "-o", str(tmp / "裸名"),
                         "--cover", str(COVER), "--layout", "L8",
                         "--no-readme", "--no-checksum"], tmp)
    if rcB == 0 and (tmp / "裸名.mkv").exists():
        ok("-o 不写后缀 → 自动补成 .mkv")
    else:
        bad(f"没自动补后缀 rc={rcB}")

    # ── ⑫ L8 + 干扰数据 → 拒绝 ──────────────────────────────────────
    print("\n⑫ L8 + 干扰数据 → 拒绝")
    rcC, soC, seC = run(["ys", str(src1), "-o", str(tmp / "dc.mkv"),
                         "--cover", str(COVER), "--layout", "L8", "--decoy", "4",
                         "--no-readme", "--no-checksum"], tmp)
    if rcC != 0 and not (tmp / "dc.mkv").exists():
        ok(f"拒绝了且零产物（rc={rcC}）—— 噪声会写坏 MKV 结构")
    else:
        bad(f"居然打成功了 rc={rcC}")

    # ── ⑬ 协议层 ────────────────────────────────────────────────────
    print("\n⑬ 协议 pack.layout=mkv")
    r = 协议({"id": 1, "cmd": "pack", "inputs": [str(src1)],
              "out": str(tmp / "proto.mkv"), "cover": str(COVER),
              "layout": "mkv", "readme": False, "checksum": False})
    res = (r or {}).get("result") or {}
    if r and r.get("ok") and res.get("layout") == "MkvAttachment":
        ok("layout=mkv → 结果回 MkvAttachment")
    else:
        bad(f"协议没认 mkv：{str(r)[:300]}")

    # ── ⑭ 没装 ffmpeg → 如实降级 ────────────────────────────────────
    print("\n⑭ 没装 ffmpeg 时：降级 + 明说（把 PATH 指到空目录）")
    空 = tmp / "empty-path"
    空.mkdir(exist_ok=True)
    环境 = os.environ.copy()
    环境["PATH"] = str(空)
    降 = tmp / "nof.mkv"
    rcd, sod, sed = run(["ys", str(src1), "-o", str(降),
                         "--cover", str(COVER), "--layout", "L8",
                         "--no-readme", "--no-checksum"], tmp, env=环境)
    if rcd != 0 or not 降.exists():
        bad(f"降级没成功 rc={rcd}：{(sed or sod).strip()[:300]}")
    else:
        是mkv = 降.read_bytes()[:4] == EBML
        if 是mkv:
            bad("产物还是 MKV —— 没有 ffmpeg 怎么可能？")
        else:
            ok("产物不是 MKV（真降级了，不是写了个假 MKV）")
        if "ffmpeg" in (sod + sed):
            ok("结果里明确提到「没装 ffmpeg」")
        else:
            bad(f"降级了但没说原因：{(sod + sed).strip()[:300]}")

    print(f"\n通过 {n_pass} / 失败 {n_fail} / 跳过 {n_skip}")
    if n_fail:
        print("有失败项，逐条看上面 ✗")
        return 1
    print("全部通过 ✅")
    return 0


if __name__ == "__main__":
    sys.exit(main())
