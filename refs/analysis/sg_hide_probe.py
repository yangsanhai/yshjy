#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""sg_hide_probe.py — 复现 SteganographierGUI 的「藏(hide)」字节级布局。

用法:
    python3 sg_hide_probe.py [--src <Steganographier.py>] [--work <工作目录>] [--password <pwd>]

做什么:
  1. 用 ffmpeg 生成一个可解码的小外壳 MP4（ftyp/free/mdat/moov），另生成 faststart 变体（ftyp/moov/free/mdat）
  2. 打桩缺失依赖后 exec 源码，直接调用真实的 Steganographier.hide_file()
     - 返回码/字节全部来自真实代码，不做任何逻辑重写
  3. 对 'mp4' / 'mp4(zarchiver)' 两种模式，打印原子链、free 原子位置、PK 签名位置、stco 前后值
  4. 用 zipfile / 7z / ffmpeg 验证「能否解出」与「能否播放」

依赖: ffmpeg(带 lavfi+mpeg4), 7z(可选), pyzipper(仅密码模式需要)
"""
import argparse, os, shutil, struct, subprocess, sys, tempfile, types

DEFAULT_SRC = "/home/user/hermes/yshjy/refs/SteganographierGUI/Steganographier.py"
FFMPEG_CANDIDATES = [
    "ffmpeg",
    "/home/user/.hermes/tools/ffmpeg-9.0.1-linux-x64/bin/ffmpeg",
]


def find_ffmpeg():
    for c in FFMPEG_CANDIDATES:
        if shutil.which(c) or os.path.exists(c):
            return c
    return None


def make_cover(ffmpeg, path, faststart=False):
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
           "testsrc=size=128x72:rate=10:duration=3", "-c:v", "mpeg4", "-q:v", "5"]
    if faststart:
        cmd += ["-movflags", "+faststart"]
    cmd += ["-y", path]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"ffmpeg 生成外壳失败: {r.stderr[-400:]}")
    return path


def load_module(src_path, app_path):
    """尽量用真依赖；缺失的才打桩（pyzipper 缺失会让 zarchiver 读端路径静默降级，见报告）。"""
    def try_import(name):
        try:
            return __import__(name)
        except ImportError:
            return None

    if not try_import("pyzipper"):
        m = types.ModuleType("pyzipper")
        for k, v in dict(AESZipFile=object, ZIP_DEFLATED=8, ZIP_STORED=0,
                         WZ_AES=99, BadZipFile=Exception).items():
            setattr(m, k, v)
        sys.modules["pyzipper"] = m
        print("[warn] pyzipper 未安装 → 打桩；zarchiver 读端(extract_with_offset_correction)会静默降级到别的解压方法")
    if not try_import("hachoir.parser"):
        m = types.ModuleType("hachoir")
        sys.modules["hachoir"] = m
        p = types.ModuleType("hachoir.parser"); p.createParser = lambda pth: None
        md = types.ModuleType("hachoir.metadata"); md.extractMetadata = lambda pth: None
        sys.modules["hachoir.parser"], sys.modules["hachoir.metadata"] = p, md
    if not try_import("natsort"):
        m = types.ModuleType("natsort")
        m.ns = type("NS", (), {"PATH": "PATH"})()
        m.natsorted = lambda it, **kw: sorted(it)
        sys.modules["natsort"] = m
    if not try_import("tkinterdnd2"):
        m = types.ModuleType("tkinterdnd2")
        m.DND_FILES, m.TkinterDnD = "DND_Files", object
        sys.modules["tkinterdnd2"] = m

    src = open(src_path, encoding="utf-8", errors="replace", newline="").read().replace("\r\n", "\n")
    body = src.split('\nif __name__ == "__main__":')[0]
    ns = {"__name__": "sgr_probe", "__file__": src_path}
    exec(compile(body, src_path, "exec"), ns)
    ns["application_path"] = app_path          # 真源码该全局量只在 __main__ 里定义
    return ns["Steganographier"]


CONTAINERS = ("moov", "trak", "mdia", "minf", "stbl", "udta")


def atoms(path, maxdepth=3):
    """返回 (type, size, offset, header_size, depth)，容器原子递归展开（纯内存切片，不动共享指针）。"""
    data = open(path, "rb").read()
    out = []

    def walk(start, end, depth):
        pos = start
        while pos + 8 <= end:
            size = int.from_bytes(data[pos:pos + 4], "big")
            atype = data[pos + 4:pos + 8].decode("latin1")
            hdr = 8
            if size == 1:
                if pos + 16 > end:
                    return
                size = int.from_bytes(data[pos + 8:pos + 16], "big")
                hdr = 16
            elif size == 0:
                size = end - pos
            if size < 8 or pos + size > end:
                out.append((atype, size, pos, hdr, depth))   # 非法/越界：如实报告后停止
                return
            out.append((atype, size, pos, hdr, depth))
            if depth < maxdepth and atype in CONTAINERS:
                walk(pos + hdr, pos + size, depth + 1)
            pos += size

    walk(0, len(data), 0)
    return out


def stco_entries(path):
    """读取 stco/co64 偏移表。布局: [size4][type4][version/flags4][count4][entries...]"""
    data = open(path, "rb").read()
    res = {}
    for tag, fmt, w in ((b"stco", ">I", 4), (b"co64", ">Q", 8)):
        i = data.find(tag)          # i 指向 type 字段
        if i < 0:
            continue
        n = struct.unpack(">I", data[i + 8:i + 12])[0]
        base = i + 12
        if n > 4096 or base + w * n > len(data):
            res[tag.decode()] = f"<非法 entry_count={n}>"
            continue
        res[tag.decode()] = [struct.unpack(fmt, data[base + w * k: base + w * (k + 1)])[0] for k in range(n)]
    return res


def sig_hits(data):
    sigs = {"RAR4": b"\x52\x61\x72\x21\x1a\x07\x00", "RAR5": b"\x52\x61\x72\x21\x1a\x07\x01\x00",
            "7Z": b"\x37\x7a\xbc\xaf\x27\x1c", "ZIP": b"PK\x03\x04", "GZIP": b"\x1f\x8b",
            "BZIP2": b"BZh", "XZ": b"\xfd7zXZ\x00"}
    out = {}
    for name, s in sigs.items():
        ps, i = [], 0
        while True:
            j = data.find(s, i)
            if j < 0:
                break
            ps.append(j); i = j + 1
        if ps:
            out[name] = ps
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=DEFAULT_SRC)
    ap.add_argument("--work", default=None)
    ap.add_argument("--password", default=None)
    a = ap.parse_args()

    work = a.work or tempfile.mkdtemp(prefix="sg_probe_")
    os.makedirs(os.path.join(work, "cover_video"), exist_ok=True)
    os.makedirs(os.path.join(work, "payload", "sub"), exist_ok=True)
    with open(os.path.join(work, "payload", "hello.txt"), "w") as f:
        f.write("hello unbox! " * 200)
    with open(os.path.join(work, "payload", "sub", "nested.bin"), "wb") as f:
        f.write(bytes(range(256)) * 40)

    ffmpeg = find_ffmpeg()
    if not ffmpeg:
        sys.exit("找不到 ffmpeg")
    cover = make_cover(ffmpeg, os.path.join(work, "cover_video", "cover.mp4"))
    cover_fast = make_cover(ffmpeg, os.path.join(work, "cover_fast.mp4"), faststart=True)
    print(f"[work] {work}")
    for c in (cover, cover_fast):
        print(f"[cover] {os.path.basename(c)} size={os.path.getsize(c)} atoms={[(t,s,o) for t,s,o,h,_ in atoms(c)]}")

    S = load_module(a.src, work)
    sg = S(gui_enabled=True, enable_log_file=False)
    logs = []
    sg.set_log_callback(lambda m: logs.append(m))
    pw = a.password

    for mode, name, cv in [("mp4", "out_plain.mp4", cover),
                           ("mp4(zarchiver)", "out_z.mp4", cover),
                           ("mp4(zarchiver)", "out_zfast.mp4", cover_fast)]:
        out = os.path.join(work, name)
        logs.clear()
        sg.hide_file(os.path.join(work, "payload"), cover_video_CLI=cv, password=pw,
                     processed_files=0, output_file_path=out, type_option_var=mode)
        data = open(out, "rb").read()
        print(f"\n===== mode={mode} cover={os.path.basename(cv)} -> {name} size={len(data)} =====")
        print("  atoms:", [(t, s, o, d) for t, s, o, h, d in atoms(out)][:6])
        print("  sigs :", sig_hits(data))
        print("  stco :", {k: v[:4] for k, v in stco_entries(out).items()},
              " 封面 stco:", {k: v[:4] for k, v in stco_entries(cv).items()})
        print("  tail :", data[-16:].hex())
        for l in logs:
            if any(k in l for k in ("结束位置", "free 原子", "最终文件大小")):
                print("  LOG  :", l)
        r = subprocess.run([ffmpeg, "-v", "error", "-i", out, "-f", "null", "-"],
                           capture_output=True, text=True)
        print(f"  ffmpeg decode rc={r.returncode} err={r.stderr.strip()[:160]!r}")
        r7 = shutil.which("7z")
        if r7:
            r = subprocess.run([r7, "l", out], capture_output=True, text=True)
            head = [l for l in r.stdout.splitlines() if "Type =" in l or "Offset =" in l or "Physical" in l]
            print("  7z    :", r.returncode, head)

    # 本程序自己的读端回环
    print("\n===== 读端回环（reveal_file）=====")
    sg.passwords = []
    for name, mode in [("out_plain.mp4", "mp4"), ("out_z.mp4", "mp4(zarchiver)")]:
        d = os.path.join(work, "rt_" + name)
        os.makedirs(d, exist_ok=True)
        shutil.copy(os.path.join(work, name), os.path.join(d, "a.mp4"))
        logs.clear()
        sg.reveal_file(os.path.join(d, "a.mp4"), password=pw, type_option_var=mode, delete_original=False)
        got = sorted(os.listdir(d))
        tried = [l.split("尝试方法: ", 1)[1] for l in logs if "尝试方法: " in l]
        ok = [l for l in logs if "成功方法" in l]
        print(f"  {name} mode={mode} -> {got}\n     尝试顺序={tried}\n     命中={ok}")


if __name__ == "__main__":
    main()
