#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""make_samples.py —— 生成 yshjy（杨三海解压）项目的全套测试样本。

用法（可重跑，默认覆盖重建）：
    python3 tests/make_samples.py                 # 生成 + 验证
    python3 tests/make_samples.py --verify-only   # 只验证已有样本
    python3 tests/make_samples.py --only A,B      # 只生成/验证指定分组

产出：
    tests/data/       所有样本（本脚本的最终产物）
    tests/assets/     素材（外壳视频、载荷目录），供复现
    tests/.work/      临时目录（不入库）
    tests/results.json 每样本的验证结果（机器可读）

设计要点（与 refs/analysis/steganographier-hide.md 对齐）：
  * A 组用隐写者**真源码**跑（exec 源码 + 只打桩缺失依赖），不重写任何逻辑。
  * 隐写者 mkv 模式需要 mkvmerge；本机无系统 mkvmerge，用 apt-get download 解包出的
    mkvmerge（见 MKVMERGE 探测），并把 sg.mkvmerge_exe 指向它。
  * B/C/D 组是手工/系统工具构造，模拟 copy /b、apate、PolyZip、损坏包等场景。
"""

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import types
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
DATA = os.path.join(HERE, "data")
ASSETS = os.path.join(HERE, "assets")
WORK = os.path.join(HERE, ".work")

SG_DIR = os.path.join(REPO, "refs", "SteganographierGUI")
SG_SRC = os.path.join(SG_DIR, "Steganographier.py")

FFMPEG_CANDIDATES = [
    "/usr/bin/ffmpeg",
    os.path.expanduser("~/.hermes/tools/ffmpeg-9.0.1-linux-x64/bin/ffmpeg"),
]
SEVENZIP_CANDIDATES = ["/usr/bin/7z", "/usr/bin/7za", "7z"]

# mkvmerge 探测顺序：环境变量 → 系统 → 用户目录里的「apt-get download + dpkg-deb -x」解包副本
# （本机无 root、也没装 mkvtoolnix；解包副本见 README §2）
MKVTOOLS_DIRS = [
    os.path.expanduser("~/.local/share/yshjy-tools/mkvtoolnix/usr/bin"),
    os.path.expanduser("~/.hermes/cache/scratch/mkvtools/usr/bin"),
]
MKVMERGE_CANDIDATES = ([os.environ.get("MKVMERGE", ""), "/usr/bin/mkvmerge"]
                       + [os.path.join(d, "mkvmerge") for d in MKVTOOLS_DIRS])
MKVEXTRACT_CANDIDATES = ([os.environ.get("MKVEXTRACT", ""), "/usr/bin/mkvextract"]
                         + [os.path.join(d, "mkvextract") for d in MKVTOOLS_DIRS])
MKVINFO_CANDIDATES = ([os.environ.get("MKVINFO", ""), "/usr/bin/mkvinfo"]
                      + [os.path.join(d, "mkvinfo") for d in MKVTOOLS_DIRS])

# 密码（写进 README，测试时用）
PWD_ASCII = "P@ssw0rd!2026"
PWD_CN_STEG = "demo-password"          # 隐写者 A3：中文密码（AES-256 / AE-2）
PWD_CN_7Z = "demo-password2026"        # C10：中文密码 7z
PWD_C8 = "zipcrypto123"
PWD_C9 = "AES256asciiPwd"


# --------------------------------------------------------------------------
# 小工具
# --------------------------------------------------------------------------
def first_existing(paths):
    for p in paths:
        if p and (os.path.isabs(p) and os.path.exists(p) or shutil.which(p)):
            return p
    return None


def sh(cmd, cwd=None, env=None, timeout=300, input_text=None):
    """跑一条命令，返回 (rc, stdout, stderr)。cmd 为 list 或 str。"""
    e = dict(os.environ)
    if env:
        e.update(env)
    r = subprocess.run(cmd, shell=isinstance(cmd, str), cwd=cwd, env=e,
                       capture_output=True, text=True, input=input_text, timeout=timeout)
    return r.returncode, r.stdout or "", r.stderr or ""


def norm_env_with_ld(tool_path=None):
    """mkvtoolnix 从 .deb 解包出来时，需要指向它自带的 lib 目录才能跑。"""
    lib = None
    if tool_path:
        # <prefix>/usr/bin/mkvmerge -> <prefix>/usr/lib/x86_64-linux-gnu
        prefix = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(tool_path))))
        cand = os.path.join(prefix, "usr", "lib", "x86_64-linux-gnu")
        if os.path.isdir(cand):
            lib = cand
    if lib:
        old = os.environ.get("LD_LIBRARY_PATH", "")
        return {"LD_LIBRARY_PATH": lib + (":" + old if old else "")}
    return {}


def size_of(path):
    return os.path.getsize(path) if os.path.exists(path) else 0


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def ffmpeg_decode_ok(ffmpeg, path):
    """返回 (rc, stderr)。rc==0 表示解码器能整条读完。"""
    rc, out, err = sh([ffmpeg, "-v", "error", "-i", path, "-f", "null", "-"], timeout=120)
    return rc, (err or "").strip()[:300]


def sevenzip_info(sevenzip, path):
    """用 7z l -slt 取 Type / Physical Size / Offset 等。"""
    rc, out, err = sh([sevenzip, "l", "-slt", path], timeout=120)
    info = {"rc": rc, "raw_err": err.strip()[:200]}
    for line in out.splitlines():
        if " = " in line:
            k, v = line.split(" = ", 1)
            k = k.strip()
            if k in ("Type", "Physical Size", "Offset", "Path", "Method", "Encrypted", "Files", "Size"):
                info[k] = v.strip()
    return info


def scan_local_headers(path, limit=8):
    """顺序扫 local file header，取 flags / method / 0x9901(AES) extra —— 判 ZipCrypto / AES 用。"""
    data = open(path, "rb").read()
    out, pos = [], 0
    while len(out) < limit:
        off = data.find(b"PK\x03\x04", pos)
        if off < 0 or off + 30 > len(data):
            break
        flags = int.from_bytes(data[off + 6:off + 8], "little")
        method = int.from_bytes(data[off + 8:off + 10], "little")
        nl = int.from_bytes(data[off + 26:off + 28], "little")
        el = int.from_bytes(data[off + 28:off + 30], "little")
        name = data[off + 30:off + 30 + nl].decode("utf-8", "replace")
        extra = data[off + 30 + nl:off + 30 + nl + el]
        aes = None
        p = 0
        while p + 4 <= len(extra):
            tag = int.from_bytes(extra[p:p + 2], "little")
            ln = int.from_bytes(extra[p + 2:p + 4], "little")
            if tag == 0x9901 and p + 4 + ln <= len(extra):
                b = extra[p + 4:p + 4 + ln]
                aes = {"version": int.from_bytes(b[0:2], "little"),
                       "vendor": b[2:4].decode("latin1"),
                       "strength": b[4],
                       "real_method": int.from_bytes(b[5:7], "little")}
            p += 4 + ln
        out.append({"off": off, "flags": hex(flags), "encrypted": bool(flags & 1), "method": method,
                    "name": name, "aes": aes, "extra_hex": extra.hex()[:80]})
        pos = off + 4
    return out


def zip_tail_math(path):
    """算 concat：EOCD 实际位置 − size_cd − offset_cd（= 前置垃圾长度）。"""
    data = open(path, "rb").read()
    e = data.rfind(b"PK\x05\x06")
    if e < 0:
        return {"eocd": None}
    size_cd = int.from_bytes(data[e + 12:e + 16], "little")
    off_cd = int.from_bytes(data[e + 16:e + 20], "little")
    concat = e - size_cd - off_cd
    first_local = data.find(b"PK\x03\x04")
    return {"eocd": e, "eocd_from_end": len(data) - 4 - e, "size_cd": size_cd,
            "offset_cd": off_cd, "concat": concat, "first_local_header": first_local,
            "cd_at_concat": data[off_cd + concat:off_cd + concat + 4].hex() if concat >= 0 else None,
            "comment_len": int.from_bytes(data[e + 20:e + 22], "little")}


def rescue_zip_local_headers(path, outdir):
    """救援演示：完全不用中心目录，只按 local header 顺序扫描并解出成员。

    覆盖 method 0(stored) / 8(deflate)，校验 CRC32；带 data descriptor(bit3) 或 AES 的跳过。
    返回 (恢复成功的成员名列表, 失败信息列表, 总字节数)。
    """
    import zlib
    data = open(path, "rb").read()
    os.makedirs(outdir, exist_ok=True)
    ok, bad, total = [], [], 0
    pos = 0
    while True:
        off = data.find(b"PK\x03\x04", pos)
        if off < 0 or off + 30 > len(data):
            break
        (ver, flags, method, mt, md, crc, csz, usz, nl, el) = struct.unpack("<HHHHHIIIHH",
                                                                          data[off + 4:off + 30])
        name = data[off + 30:off + 30 + nl].decode("utf-8", "replace")
        dstart = off + 30 + nl + el
        if flags & 0x08:
            bad.append(f"{name}: 有 data descriptor(bit3)，需流式判定长度")
            pos = off + 4
            continue
        if method not in (0, 8):
            bad.append(f"{name}: method={method} 不在演示范围")
            pos = dstart + csz
            continue
        raw = data[dstart:dstart + csz]
        if len(raw) < csz:
            bad.append(f"{name}: 数据被截断（需要 {csz}B，只剩 {len(raw)}B）")
            break
        try:
            out = raw if method == 0 else zlib.decompressobj(-15).decompress(raw)
        except Exception as e:
            bad.append(f"{name}: 解压失败 {type(e).__name__}")
            pos = off + 4
            continue
        if zlib.crc32(out) & 0xFFFFFFFF != crc:
            bad.append(f"{name}: CRC32 不匹配")
        else:
            dst = os.path.join(outdir, name.replace("/", "__"))
            with open(dst, "wb") as f:
                f.write(out)
            ok.append(name)
            total += len(out)
        pos = dstart + csz
    return ok, bad, total


def reveal_probe(names, pwd_map):
    """用隐写者真源码的读端跑一遍，记录「尝试了哪些方法 / 最终命中哪个」。"""
    S = load_steg_class()
    sg = S(gui_enabled=True, enable_log_file=False)
    sg.passwords = []
    logs = []
    if hasattr(sg, "set_log_callback"):
        sg.set_log_callback(lambda m: logs.append(m))
    out = {}
    for name in names:
        src = os.path.join(DATA, name)
        if not os.path.exists(src):
            continue
        d = os.path.join(WORK, "reveal", name)
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d, exist_ok=True)
        dst = os.path.join(d, name)
        shutil.copy(src, dst)
        logs.clear()
        err = None
        try:
            sg.reveal_file(dst, password=pwd_map.get(name), type_option_var="mp4",
                           delete_original=False)
        except Exception as e:
            err = f"{type(e).__name__}: {e}"
        wrote = sorted(f for f in os.listdir(d) if f != name)
        tried, hit = [], []
        for l in logs:
            if "尝试方法: " in l:
                tried.append(l.split("尝试方法: ", 1)[1].strip())
            if "成功方法: " in l:
                hit.append(l.split("成功方法: ", 1)[1].strip())
        out[name] = {"tried": tried, "hit": hit[:2], "err": err, "wrote": wrote[:5]}
    return out



def sevenzip_test(sevenzip, path, pwd=None, cwd=None):
    cmd = [sevenzip, "t", path]
    if pwd is not None:
        cmd.append("-p" + pwd)
    rc, out, err = sh(cmd, cwd=cwd, timeout=300)
    tail = (out.strip().splitlines() or [""])[-1]
    return rc, tail[:200], err.strip()[:200]


# --------------------------------------------------------------------------
# 1. 素材：外壳视频 + 载荷目录
# --------------------------------------------------------------------------
PAYLOAD_FILES = {
    "说明.txt": "这是一个测试载荷。\n用于 yshjy 测试样本集。\n" * 20,
    "中文目录/文件1.txt": "中文文件名测试 abc 123\n" * 40,
    "中文目录/sub/空格 文件名.bin": None,   # 随机数据，见下
    "random.bin": None,
    "empty.txt": "",
}


def build_assets(ffmpeg):
    os.makedirs(ASSETS, exist_ok=True)
    cover_dir = os.path.join(ASSETS, "cover")
    os.makedirs(cover_dir, exist_ok=True)

    covers = []
    specs = [
        ("cover.mp4", "testsrc=size=128x72:rate=10:duration=3", []),
        ("cover_fast.mp4", "testsrc2=size=128x72:rate=10:duration=3", ["-movflags", "+faststart"]),
        ("cover2.mp4", "smptebars=size=160x90:rate=8:duration=2", []),
        # cover_big.mp4：mp4 数据段 >64KB，用来构造「ZIP 前后都有大段数据」的样本
        ("cover_big.mp4", "testsrc=size=320x240:rate=15:duration=5", []),
    ]
    for name, src, extra in specs:
        p = os.path.join(cover_dir, name)
        cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", src,
               "-c:v", "mpeg4", "-q:v", "5"] + extra + ["-y", p]
        rc, out, err = sh(cmd, timeout=180)
        if rc != 0:
            raise SystemExit(f"ffmpeg 生成 {name} 失败: {err[-400:]}")
        covers.append(p)

    # cover3.mp4：在合法外壳后面再补 4 个空 free 原子 —— 顶层原子变多，
    # 隐写者 zarchiver 模式的「每个原子后插一份噪声」也随之变多，尾部噪声总量
    # 超过 64KB，使得「从尾部找 EOCD」这条路走不通（只能靠 free 原子扫描）。
    cover3 = os.path.join(cover_dir, "cover3.mp4")
    with open(cover3, "wb") as o:
        o.write(open(os.path.join(cover_dir, "cover.mp4"), "rb").read())
        for _ in range(4):
            o.write(struct.pack(">I4s", 8, b"free"))
    covers.append(cover3)

    # B3 用的 PNG / JPG 外壳
    png = os.path.join(cover_dir, "cover.png")
    rc, _, err = sh([ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "lavfi",
                     "-i", "testsrc=size=96x54:rate=1:duration=1", "-frames:v", "1", "-y", png])
    if rc != 0:
        raise SystemExit("ffmpeg 生成 png 失败: " + err[-300:])
    jpg = os.path.join(cover_dir, "cover.jpg")
    rc, _, err = sh([ffmpeg, "-hide_banner", "-loglevel", "error", "-f", "lavfi",
                     "-i", "testsrc=size=96x54:rate=1:duration=1", "-frames:v", "1", "-y", jpg])
    if rc != 0:
        raise SystemExit("ffmpeg 生成 jpg 失败: " + err[-300:])

    # 载荷目录（含中文名、子目录、随机二进制，保证可压缩性混合）
    payload = os.path.join(ASSETS, "payload")
    if os.path.isdir(payload):
        shutil.rmtree(payload)
    os.makedirs(payload, exist_ok=True)
    import random as _rnd
    gen = _rnd.Random(20261016)          # 固定种子 → 载荷字节可复现
    for rel, content in PAYLOAD_FILES.items():
        p = os.path.join(payload, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        if content is None:
            with open(p, "wb") as f:
                f.write(bytes(gen.getrandbits(8) for _ in range(200 * 1024)))
        else:
            with open(p, "w", encoding="utf-8") as f:
                f.write(content)
    # 把 mtime 全部钉死：tar/gzip/7z/zip 都会把 mtime 写进产物，
    # 不固定的话每次重跑产物字节都不同，回归测试没法比对。
    FIXED_MTIME = 1750000000
    for root, dirs, files in os.walk(payload):
        for x in files + dirs:
            os.utime(os.path.join(root, x), (FIXED_MTIME, FIXED_MTIME))
    os.utime(payload, (FIXED_MTIME, FIXED_MTIME))
    return {"covers": covers, "png": png, "jpg": jpg, "payload": payload}


FIXED_MTIME = 1750000000


def pin_payload_times(payload):
    """把 payload 的 mtime/atime 一起钉死。

    Info-ZIP 的 UT extra 字段会同时写 mtime 和 **atime**，而任何一次读取都会更新
    atime，于是同一份载荷两次跑 zip 出来的字节就不同（.z01 里能差几个字节）。
    7z / tar 只写 mtime，所以不受影响。凡是要跑 zip 之前都先调一次本函数。
    """
    for root, dirs, files in os.walk(payload):
        for x in files + dirs:
            os.utime(os.path.join(root, x), (FIXED_MTIME, FIXED_MTIME))
    os.utime(payload, (FIXED_MTIME, FIXED_MTIME))


def build_payload_zip(dst, payload_dir, comment=None):
    """把 payload 目录压成一个普通 ZIP（C1 的复用件，也用作 B 组的内芯）。"""
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
        for root, dirs, files in os.walk(payload_dir):
            for fn in sorted(files):
                full = os.path.join(root, fn)
                arc = os.path.join(os.path.basename(payload_dir),
                                   os.path.relpath(full, payload_dir))
                z.write(full, arc)
                # 中文名冲突？系统里唯一
    if comment:
        with open(dst, "r+b") as f:
            data = f.read()
            # 简单办法：给 EOCD 加 comment（重写整个 EOCD）
            idx = data.rfind(b"PK\x05\x06")
            eocd = bytearray(data[idx:idx + 22])
            cb = comment.encode("utf-8")
            eocd[20:22] = struct.pack("<H", len(cb))
            new = data[:idx] + bytes(eocd) + cb
            f.seek(0)
            f.write(new)
            f.truncate()
    return dst


# --------------------------------------------------------------------------
# 2. A 组：隐写者真源码
# --------------------------------------------------------------------------
def load_steg_class():
    """exec 真源码（CRLF→LF，去掉 __main__），只打桩本机缺失的依赖。"""
    def stub_hachoir():
        m = types.ModuleType("hachoir")
        sys.modules["hachoir"] = m
        p = types.ModuleType("hachoir.parser")
        p.createParser = lambda pth: None
        md = types.ModuleType("hachoir.metadata")
        md.extractMetadata = lambda pth: None
        sys.modules["hachoir.parser"] = p
        sys.modules["hachoir.metadata"] = md

    try:
        import hachoir.parser  # noqa: F401
    except ImportError:
        stub_hachoir()
    try:
        import natsort  # noqa: F401
    except ImportError:
        m = types.ModuleType("natsort")
        m.ns = type("NS", (), {"PATH": "PATH"})()
        m.natsorted = lambda it, **kw: sorted(it)
        sys.modules["natsort"] = m

    src = open(SG_SRC, encoding="utf-8", errors="replace", newline="").read().replace("\r\n", "\n")
    body = src.split('\nif __name__ == "__main__":')[0]
    ns = {"__name__": "sgr_gen", "__file__": SG_SRC, "application_path": SG_DIR}
    exec(compile(body, SG_SRC, "exec"), ns)
    return ns["Steganographier"]


def make_A_samples(dst_dir, assets, mkvmerge):
    S = load_steg_class()
    sg = S(gui_enabled=True, enable_log_file=False)
    if mkvmerge:
        sg.mkvmerge_exe = mkvmerge
    sg.passwords = []
    logs = []
    if hasattr(sg, "set_log_callback"):
        sg.set_log_callback(lambda m: logs.append(m))

    payload = assets["payload"]
    cover = os.path.join(ASSETS, "cover", "cover.mp4")
    cover_fast = os.path.join(ASSETS, "cover", "cover_fast.mp4")
    cover2 = os.path.join(ASSETS, "cover", "cover2.mp4")
    cover3 = os.path.join(ASSETS, "cover", "cover3.mp4")

    made = {}
    jobs = [
        ("A1_steg_modeA_plain.mp4", "mp4", cover, None),
        ("A2_steg_modeA_ascii_pwd.mp4", "mp4", cover2, PWD_ASCII),
        ("A3_steg_modeA_cn_pwd.mp4", "mp4", cover_fast, PWD_CN_STEG),
        ("A4_steg_modeB_zarchiver.mp4", "mp4(zarchiver)", cover, None),
        ("A4b_steg_modeB_tailnoise_gt64k.mp4", "mp4(zarchiver)", cover3, None),
        ("A5_steg_mkv_mode.mkv", "mkv", cover, None),
    ]
    for name, mode, cv, pwd in jobs:
        out = os.path.join(dst_dir, name)
        if os.path.exists(out):
            os.remove(out)
        logs.clear()
        cwd = os.getcwd()
        try:
            os.chdir(WORK)   # mkv 模式会在 CWD 里写 temp_xxx 随机文件
            sg.hide_file(payload, cover_video_CLI=cv, password=pwd, processed_files=0,
                         output_file_path=out, type_option_var=mode)
        finally:
            os.chdir(cwd)
        made[name] = {
            "mode": mode, "cover": os.path.basename(cv), "password": pwd,
            "ok": os.path.exists(out) and size_of(out) > 0,
            "size": size_of(out),
            "log_tail": [l for l in logs if any(k in l for k in
                         ("最终文件大小", "结束位置", "输出文件", "Output file"))][-3:],
        }
    return made


def make_A6_oldstyle(dst, cover, payload_zip):
    """老版模式 A（报告 §3.1 描述）：[外壳][裸 ZIP][假签名+随机]×2[假 mdat][空 mdat]。

    隐写者 1.3.x 已改掉这个写法，但线上仍有大量老版产物，故手工补一个。
    """
    import random as _rnd
    _rnd.seed(1311)
    sigs = [b"\x52\x61\x72\x21\x1A\x07\x00", b"\x37\x7A\xBC\xAF\x27\x1C",
            b"\x50\x4B\x03\x04", b"\x1F\x8B", b"\x42\x5A\x68", b"\xFD\x37\x7A\x58\x5A\x00"]
    with open(dst, "wb") as out:
        with open(cover, "rb") as f:
            out.write(f.read())
        with open(payload_zip, "rb") as f:
            out.write(f.read())
        for _ in range(2):
            out.write(_rnd.choice(sigs))
            out.write(os.urandom(1024 * _rnd.randint(5, 10)))
        out.write((8).to_bytes(4, "big") + b"mdat")   # 假 mdat（声明 8 字节，即自身）
        out.write((8).to_bytes(4, "big") + b"mdat")   # 空 mdat 结尾标记
    return dst


# --------------------------------------------------------------------------
# 3. B 组：手工构造
# --------------------------------------------------------------------------
def mp4_top_atoms(data):
    """顺序扫顶层原子，返回 [(type, offset, size)]，遇非法即停。"""
    out, pos, n = [], 0, len(data)
    while pos + 8 <= n:
        size = int.from_bytes(data[pos:pos + 4], "big")
        atype = data[pos + 4:pos + 8].decode("latin1")
        hdr = 8
        if size == 1:
            if pos + 16 > n:
                break
            size = int.from_bytes(data[pos + 8:pos + 16], "big")
            hdr = 16
        elif size == 0:
            size = n - pos
        if size < 8 or pos + size > n:
            break
        out.append((atype, pos, size))
        pos += size
    return out


def update_stco(data, delta):
    """把 moov 子树里所有 stco(32b)/co64(64b) 的偏移加 delta。正确版实现（无噪声干扰）。"""
    buf = bytearray(data)

    def walk(start, end):
        pos = start
        while pos + 8 <= end:
            size = int.from_bytes(buf[pos:pos + 4], "big")
            atype = bytes(buf[pos + 4:pos + 8])
            hdr = 8
            if size == 1:
                size = int.from_bytes(buf[pos + 8:pos + 16], "big")
                hdr = 16
            elif size == 0:
                size = end - pos
            if size < 8 or pos + size > end:
                return
            if atype == b"stco":
                cnt = int.from_bytes(buf[pos + 12:pos + 16], "big")
                for i in range(cnt):
                    o = pos + 16 + i * 4
                    if o + 4 > pos + size:
                        break
                    v = int.from_bytes(buf[o:o + 4], "big") + delta
                    buf[o:o + 4] = v.to_bytes(4, "big")
            elif atype == b"co64":
                cnt = int.from_bytes(buf[pos + 12:pos + 16], "big")
                for i in range(cnt):
                    o = pos + 16 + i * 8
                    if o + 8 > pos + size:
                        break
                    v = int.from_bytes(buf[o:o + 8], "big") + delta
                    buf[o:o + 8] = v.to_bytes(8, "big")
            elif atype in (b"moov", b"trak", b"mdia", b"minf", b"stbl", b"udta"):
                walk(pos + hdr, pos + size)
            pos += size

    for atype, off, size in mp4_top_atoms(bytes(buf)):
        if atype == "moov":
            walk(off + 8, off + size)
    return bytes(buf)


def make_B_samples(dst_dir, assets, payload_zip):
    cover = os.path.join(ASSETS, "cover", "cover.mp4")
    made = {}

    # --- B1: copy /b 式尾部拼接（MP4 + ZIP 直接 cat）
    b1 = os.path.join(dst_dir, "B1_tail_append_cat.mp4")
    with open(b1, "wb") as o:
        o.write(open(cover, "rb").read())
        o.write(open(payload_zip, "rb").read())
    made["B1_tail_append_cat.mp4"] = {"how": "cat cover.mp4 + payload.zip"}

    # --- B2: apate 格式（换头 + 尾部逆序原始头 + 4B 小端 N）
    b2 = os.path.join(dst_dir, "B2_apate.mp4")
    with open(payload_zip, "rb") as f:
        original = f.read()
    mask_head = open(cover, "rb").read(28)          # ftyp 原子 28 字节，正好是个像样的 MP4 头
    N = len(mask_head)
    with open(b2, "wb") as o:
        o.write(mask_head)                           # 1) 头部换成面具
        o.write(original[N:])                        # 2) 原文件剩余部分
        o.write(original[:N][::-1])                  # 3) 逆序的原始头
        o.write(struct.pack("<i", N))                # 4) 小端 N
    made["B2_apate.mp4"] = {"how": f"apate: mask_head={N}B, original={len(original)}B",
                            "apate_N": N, "original_size": len(original)}

    # --- B3: PNG / JPG 尾部追加（PolyZip / 图种风格）
    for name, base in (("B3_png_tail.png", "cover.png"), ("B3_jpg_tail.jpg", "cover.jpg")):
        dst = os.path.join(dst_dir, name)
        with open(dst, "wb") as o:
            o.write(open(os.path.join(ASSETS, "cover", base), "rb").read())
            o.write(open(payload_zip, "rb").read())
        made[name] = {"how": f"cat {base} + payload.zip"}

    # --- B4: 手工 free 原子内嵌（ftyp 之后，正确的 zarchiver 形态：无噪声、stco 精确补偿）
    #     用大外壳，使 ZIP 之后仍有 >64KB 数据 → 尾部 EOCD 窗口（64KB）够不着，
    #     只能靠「遍历顶层原子找 free」解出来。
    b4 = os.path.join(dst_dir, "B4_free_atom_interior.mp4")
    cover_big = os.path.join(ASSETS, "cover", "cover_big.mp4")
    data = open(cover_big, "rb").read()
    atoms = mp4_top_atoms(data)
    ftyp = next(a for a in atoms if a[0] == "ftyp")
    zip_bytes = open(payload_zip, "rb").read()
    free_head = struct.pack(">I4s", 8 + len(zip_bytes), b"free")
    delta = len(free_head) + len(zip_bytes)
    rest = bytearray(data[ftyp[1] + ftyp[2]:])
    rest = bytearray(update_stco(bytes(rest), delta))
    with open(b4, "wb") as o:
        o.write(data[ftyp[1]:ftyp[1] + ftyp[2]])     # ftyp
        o.write(free_head)                           # free 头
        o.write(zip_bytes)                           # ZIP 原文（无 pad，原子内就以 PK 开头）
        o.write(rest)                                # 其余原子（stco 已精确 +delta）
    made["B4_free_atom_interior.mp4"] = {
        "how": f"ftyp + free(hdr+{len(zip_bytes)}B zip) + rest(stco+{delta})", "delta": delta}
    return made


def apate_restore(src, dst):
    """按逆向规则还原 apate 包：末 4B 小端 N + [len-4-N,len-4) 逆序 = 原始头。"""
    data = open(src, "rb").read()
    N = struct.unpack("<i", data[-4:])[0]
    if not (4 <= N <= len(data) - 4):
        raise ValueError(f"非法 N={N}")
    head = data[len(data) - 4 - N:len(data) - 4][::-1]
    body = data[N:len(data) - 4 - N]
    with open(dst, "wb") as f:
        f.write(head + body)
    return N, head


# --------------------------------------------------------------------------
# 4. C 组：普通压缩包
# --------------------------------------------------------------------------
def make_C_samples(dst_dir, assets, sevenzip, mk_c11_split=True):
    payload = assets["payload"]
    made = {}
    P = os.path.join(WORK, "cpkg")
    os.makedirs(P, exist_ok=True)

    def dst(name):
        return os.path.join(dst_dir, name)

    # C1 zip（普通）
    sh([sevenzip, "a", "-tzip", dst("C1_plain.zip"), payload, "-mx=5"], cwd=P)
    # C1 也保留一份纯 python zipfile 造的（用于 D 组切截断，保证结构可控）
    # C2 7z
    sh([sevenzip, "a", "-t7z", dst("C2_plain.7z"), payload, "-mx=5"], cwd=P)
    # C3 tar / C4 tar.gz / C5 tar.xz / C6 tar.zst / C7 tar.bz2
    tar_files = {
        "C3_plain.tar": None,
        "C4_plain.tar.gz": "-z",
        "C5_plain.tar.xz": "-J",
        "C7_plain.tar.bz2": "-j",
    }
    for name, flag in tar_files.items():
        cmd = ["tar", "-cf", dst(name)] + ([flag] if flag else []) + ["-C", os.path.dirname(payload),
                                                                     os.path.basename(payload)]
        sh(cmd, timeout=180)
    # C6 tar.zst（tar 不带 zstd 时走管道）
    rc, out, err = sh(["tar", "--zstd", "-cf", dst("C6_plain.tar.zst"),
                       "-C", os.path.dirname(payload), os.path.basename(payload)], timeout=180)
    if rc != 0:
        rc2, _, err2 = sh(f"tar -cf - -C {os.path.dirname(payload)} {os.path.basename(payload)} "
                          f"| zstd -q -o {dst('C6_plain.tar.zst')} -f", timeout=180)
        if rc2 != 0:
            raise SystemExit("zstd tar 失败: " + (err or err2)[-300:])

    # C5b/C6b/C7b：单流 .xz/.zst/.bz2（单文件，测单流解压路径）
    single = os.path.join(payload, "说明.txt")
    sh(f"xz -k -c -6 {single} > {dst('C5b_single.xz')}", timeout=120)
    sh(f"zstd -q -19 -o {dst('C6b_single.zst')} -f {single}", timeout=120)
    sh(f"bzip2 -c -9 {single} > {dst('C7b_single.bz2')}", timeout=120)

    # C8 ZipCrypto（zip -e / -P 走传统 PKWARE 加密）
    pin_payload_times(payload)
    sh(f"zip -q -X -r -P {PWD_C8} {dst('C8_zipcrypto.zip')} {os.path.basename(payload)}",
       cwd=os.path.dirname(payload), timeout=180)
    # C9 AES-256 zip（7z -mem=AES256）
    sh([sevenzip, "a", "-tzip", "-mem=AES256", "-p" + PWD_C9, dst("C9_aes256.zip"), payload],
       cwd=P, timeout=180)
    # C10 加密 7z（中文密码）
    sh([sevenzip, "a", "-t7z", "-p" + PWD_CN_7Z, dst("C10_pwd_cn.7z"), payload], cwd=P, timeout=180)
    # C10b 加密 7z + 头加密（-mhe=on，连文件名都加密）
    sh([sevenzip, "a", "-t7z", "-p" + PWD_CN_7Z, "-mhe=on", dst("C10b_pwd_cn_mhe.7z"), payload],
       cwd=P, timeout=180)

    # C11 7z 分卷（.7z.001/.002）
    if mk_c11_split:
        for f in os.listdir(dst_dir):
            if f.startswith("C11_") :
                os.remove(os.path.join(dst_dir, f))
        sh([sevenzip, "a", "-v300k", dst("C11_split.7z"), payload], cwd=P, timeout=180)
    # C12 zip 分卷（.zip + .z01）
    for f in os.listdir(dst_dir):
        if f.startswith("C12_"):
            os.remove(os.path.join(dst_dir, f))
    pin_payload_times(payload)
    sh(f"zip -q -X -s 300k -r {dst('C12_split.zip')} {os.path.basename(payload)}",
       cwd=os.path.dirname(payload), timeout=180)
    return made


# --------------------------------------------------------------------------
# 5. D 组：异常 / 损坏样本
# --------------------------------------------------------------------------
def make_D_samples(dst_dir, assets, sevenzip):
    payload = assets["payload"]
    P = os.path.join(WORK, "dpkg")
    os.makedirs(P, exist_ok=True)
    # 用 python zipfile 造一个结构完全可控的 zip 作为 D 组的底本
    base = os.path.join(WORK, "base_for_D.zip")
    build_payload_zip(base, payload)

    made = {}

    # D1 截断的 ZIP：砍掉尾部（EOCD + 中心目录的一部分）
    d1 = os.path.join(dst_dir, "D1_truncated.zip")
    data = open(base, "rb").read()
    cut = 300
    with open(d1, "wb") as f:
        f.write(data[:-cut])
    made["D1_truncated.zip"] = {"how": f"去掉尾部 {cut}B（EOCD 丢失）"}

    # D2a EOCD 里 cd_offset 被改（宣称偏移比真实大 2355200）
    #      —— 直接照抄我们实测到的症状：CD 宣称位置与实际位置差 2355200
    d2 = os.path.join(dst_dir, "D2a_eocd_cd_offset_tampered.zip")
    data = bytearray(open(base, "rb").read())
    idx = data.rfind(b"PK\x05\x06")
    old = int.from_bytes(data[idx + 16:idx + 20], "little")
    new = old + 2355200
    data[idx + 16:idx + 20] = new.to_bytes(4, "little")
    with open(d2, "wb") as f:
        f.write(bytes(data))
    made["D2a_eocd_cd_offset_tampered.zip"] = {
        "how": f"EOCD.cd_offset {old} -> {new}（真实 CD 仍在 {old}）",
        "cd_offset_declared": new, "cd_offset_real": old}

    # D2b 真实「前面垫了 2355200 字节垃圾」的 ZIP（= 我们 8.7GB 案例的形态）
    d2b = os.path.join(dst_dir, "D2b_prefixed_2355200.zip")
    with open(d2b, "wb") as f:
        f.write(b"JUNKJUNKJUNK" * (2355200 // 12))
        f.write(open(base, "rb").read())
    made["D2b_prefixed_2355200.zip"] = {"how": "2355200B 垃圾 + 完整 ZIP（统一偏移差）"}

    # D3 缺一个分卷的 7z（切 2 卷，只留 .001，.002 消失）
    for f in os.listdir(dst_dir):
        if f.startswith("D3_"):
            os.remove(os.path.join(dst_dir, f))
    tmpname = os.path.join(WORK, "d3src.7z")
    sh([sevenzip, "a", "-v300k", tmpname, payload], cwd=P, timeout=180)
    vols = sorted(f for f in os.listdir(WORK) if f.startswith("d3src.7z."))
    keep = {vols[0]} if vols else set()
    for v in vols:
        if v in keep:
            shutil.copy(os.path.join(WORK, v),
                        os.path.join(dst_dir, "D3_missing_volume.7z." + v.rsplit(".", 1)[1]))
        os.remove(os.path.join(WORK, v))
    made["D3_missing_volume.7z.001"] = {
        "how": f"7z 分卷共 {len(vols)} 卷（{vols}），只保留第一卷（.002 缺失）"}

    # D4 内部文件被损坏的 zip（中心目录完好，某个成员的数据被改 → CRC 错）
    d4 = os.path.join(dst_dir, "D4_corrupt_member.zip")
    data = bytearray(open(base, "rb").read())
    with zipfile.ZipFile(base) as z:
        # 挑最大的成员（random.bin）破坏它的压缩数据中段
        target = max(z.infolist(), key=lambda zi: zi.file_size)
        off = target.header_offset
        name_len = int.from_bytes(data[off + 26:off + 28], "little")
        extra_len = int.from_bytes(data[off + 28:off + 30], "little")
        data_off = off + 30 + name_len + extra_len
        for i in range(data_off + 64, data_off + 80):
            data[i] ^= 0xFF
    with open(d4, "wb") as f:
        f.write(bytes(data))
    made["D4_corrupt_member.zip"] = {
        "how": "被破坏的成员 = " + target.filename + "（数据段 16 字节 XOR，CD 完好）"}
    return made


# --------------------------------------------------------------------------
# 6. 验证
# --------------------------------------------------------------------------
def verify_all(dst_dir, assets, ffmpeg, sevenzip, mkvmerge, mkvextract, mkvinfo):
    import pyzipper  # 只用真包做 AES 校验（Hermes python 自带）

    R = {}

    def rec(name, **kw):
        R.setdefault(name, {}).update(kw)

    def P(name):
        p = os.path.join(dst_dir, name)
        rec(name, exists=os.path.exists(p), size=size_of(p))
        return p

    # ---- A 组
    for name in ["A1_steg_modeA_plain.mp4", "A2_steg_modeA_ascii_pwd.mp4",
                 "A3_steg_modeA_cn_pwd.mp4", "A4_steg_modeB_zarchiver.mp4",
                 "A4b_steg_modeB_tailnoise_gt64k.mp4"]:
        p = P(name)
        if not os.path.exists(p):
            continue
        rec(name, sha256=sha256_of(p))
        info = sevenzip_info(sevenzip, p)
        rec(name, sevenzip_type=info.get("Type"), sevenzip_offset=info.get("Offset"),
            sevenzip_physical=info.get("Physical Size"))
        rc, err = ffmpeg_decode_ok(ffmpeg, p)
        rec(name, ffmpeg_rc=rc, ffmpeg_err=err)
        # 尾部扫描：EOCD 是否就在文件末尾附近
        data = open(p, "rb").read()
        rec(name, eocd_pos=data.rfind(b"PK\x05\x06"), eocd_from_end=len(data) - 4 - data.rfind(b"PK\x05\x06"),
            pk_local_offsets=[i for i in range(len(data)) if data.startswith(b"PK\x03\x04", i)][:5],
            tail16=data[-16:].hex())
        rec(name, tail_math=zip_tail_math(p))
        # 能否用密码解开
        pwd = {"A2_steg_modeA_ascii_pwd.mp4": PWD_ASCII,
               "A3_steg_modeA_cn_pwd.mp4": PWD_CN_STEG}.get(name)
        if name.startswith("A4"):
            # zip 在文件中部：7z 从尾部找 EOCD（中部也能找到）→ 取决于实现
            rc, tail, err = sevenzip_test(sevenzip, p, pwd=None)
            rec(name, sevenzip_test_rc=rc, sevenzip_test_tail=tail)
        else:
            rc, tail, err = sevenzip_test(sevenzip, p, pwd=pwd)
            rec(name, sevenzip_test_rc=rc, sevenzip_test_tail=tail)
        # python zipfile / pyzipper 直开
        try:
            if pwd:
                with pyzipper.AESZipFile(p) as z:
                    z.setpassword(pwd.encode("utf-8"))
                    names = z.namelist()
                    first = names[0]
                    z.read(first)
                rec(name, pyzipper="OK", pyzipper_members=len(names), pyzipper_first=first)
            else:
                with zipfile.ZipFile(p) as z:
                    names = z.namelist()
                    bad = z.testzip()
                    rec(name, zipfile="OK", zipfile_members=len(names), zipfile_bad=bad)
        except Exception as e:
            rec(name, zipfile_or_pyzipper=f"{type(e).__name__}: {e}")

    # A6（老版裸追加）
    p = P("A6_oldstyle_bare_append.mp4")
    if os.path.exists(p):
        info = sevenzip_info(sevenzip, p)
        rc, err = ffmpeg_decode_ok(ffmpeg, p)
        data = open(p, "rb").read()
        rc2, tail, _ = sevenzip_test(sevenzip, p)
        rec("A6_oldstyle_bare_append.mp4", sevenzip_type=info.get("Type"),
            sevenzip_offset=info.get("Offset"), ffmpeg_rc=rc, ffmpeg_err=err,
            sevenzip_test_rc=rc2, sevenzip_test_tail=tail,
            eocd_pos=data.rfind(b"PK\x05\x06"), size=len(data),
            tail16=data[-16:].hex(), tail_math=zip_tail_math(p))

    # ---- A5 mkv
    p = P("A5_steg_mkv_mode.mkv")
    if os.path.exists(p) and mkvinfo:
        env = norm_env_with_ld(mkvinfo or mkvmerge)
        rc, out, err = sh([mkvinfo, p], env=env, timeout=120)
        att = [l.strip() for l in out.splitlines() if "ttach" in l or "File name" in l or "MIME" in l]
        rec("A5_steg_mkv_mode.mkv", mkvinfo_rc=rc, mkvinfo_attach=att[:8], mkvinfo_err=err[:200])
        if mkvextract:
            ex = os.path.join(WORK, "mkvx")
            shutil.rmtree(ex, ignore_errors=True)
            os.makedirs(ex, exist_ok=True)
            rc, out, err = sh([mkvextract, "attachments", p, "1:out1", "2:out2"], cwd=ex, env=env, timeout=120)
            files = {f: size_of(os.path.join(ex, f)) for f in os.listdir(ex)}
            rec("A5_steg_mkv_mode.mkv", mkvextract_rc=rc, mkvextract_files=files, mkvextract_err=err[:200])
            o1 = os.path.join(ex, "out1")
            if os.path.exists(o1):
                info = sevenzip_info(sevenzip, o1)
                rec("A5_steg_mkv_mode.mkv", attachment1_type=info.get("Type"))

    # ---- B 组
    p = P("B1_tail_append_cat.mp4")
    if os.path.exists(p):
        info = sevenzip_info(sevenzip, p)
        rc, err = ffmpeg_decode_ok(ffmpeg, p)
        rc2, tail, _ = sevenzip_test(sevenzip, p)
        try:
            with zipfile.ZipFile(p) as z:
                zt = f"OK members={len(z.namelist())} bad={z.testzip()}"
        except Exception as e:
            zt = f"{type(e).__name__}: {e}"
        rec("B1_tail_append_cat.mp4", sevenzip_type=info.get("Type"), sevenzip_offset=info.get("Offset"),
            ffmpeg_rc=rc, ffmpeg_err=err, sevenzip_test_rc=rc2, sevenzip_test_tail=tail, zipfile=zt,
            tail_math=zip_tail_math(p))

    p = P("B2_apate.mp4")
    if os.path.exists(p):
        data = open(p, "rb").read()
        N = struct.unpack("<i", data[-4:])[0]
        restored = os.path.join(WORK, "apate_restored.zip")
        try:
            n2, head = apate_restore(p, restored)
            with zipfile.ZipFile(restored) as z:
                ok = f"OK members={len(z.namelist())} bad={z.testzip()}"
            rec("B2_apate.mp4", N=N, head_hex=head.hex(), restore_method="逆序还原",
                restored_type=sevenzip_info(sevenzip, restored).get("Type"),
                restored_zipfile=ok, restored_sha256=sha256_of(restored))
        except Exception as e:
            rec("B2_apate.mp4", N=N, restore_err=f"{type(e).__name__}: {e}")

    for name in ["B3_png_tail.png", "B3_jpg_tail.jpg"]:
        p = P(name)
        if not os.path.exists(p):
            continue
        info = sevenzip_info(sevenzip, p)
        rc, tail, _ = sevenzip_test(sevenzip, p)
        data = open(p, "rb").read()
        if name.endswith(".png"):
            marker = data.find(b"IEND")
            extra = "IEND 之后有数据" if marker > 0 and marker + 8 < len(data) else "IEND 之后无数据"
        else:
            marker = data.rfind(b"\xff\xd9")
            extra = "EOI 之后有数据" if marker > 0 and marker + 2 < len(data) else "EOI 之后无数据"
        rec(name, sevenzip_type=info.get("Type"), sevenzip_offset=info.get("Offset"),
            sevenzip_test_rc=rc, sevenzip_test_tail=tail, tail_marker=extra)

    p = P("B4_free_atom_interior.mp4")
    if os.path.exists(p):
        data = open(p, "rb").read()
        atoms = mp4_top_atoms(data)
        rc, err = ffmpeg_decode_ok(ffmpeg, p)
        info = sevenzip_info(sevenzip, p)
        rec("B4_free_atom_interior.mp4", atoms=[(a, o, s) for a, o, s in atoms[:5]],
            ffmpeg_rc=rc, ffmpeg_err=err, sevenzip_type=info.get("Type"),
            sevenzip_offset=info.get("Offset"))

    # ---- C 组
    simple = {
        "C1_plain.zip": ("7z", [sevenzip, "t"]),
        "C2_plain.7z": ("7z", [sevenzip, "t"]),
        "C3_plain.tar": ("tar", ["tar", "-tf"]),
        "C4_plain.tar.gz": ("tar", ["tar", "-tzf"]),
        "C5_plain.tar.xz": ("tar", ["tar", "-tJf"]),
        "C6_plain.tar.zst": ("tar", ["tar", "--zstd", "-tf"]),
        "C7_plain.tar.bz2": ("tar", ["tar", "-tjf"]),
    }
    for name, (kind, cmd) in simple.items():
        p = P(name)
        if not os.path.exists(p):
            continue
        if kind == "7z":
            rc, tail, err = sevenzip_test(sevenzip, p)
            rec(name, verify_tool="7z t", rc=rc, tail=tail)
        else:
            rc, out, err = sh(cmd + [p], timeout=120)
            n = len([l for l in out.splitlines() if l.strip()])
            rec(name, verify_tool=" ".join(cmd), rc=rc, members=n, err=err[:200])

    for name, tool in [("C5b_single.xz", "xz"), ("C6b_single.zst", "zstd"), ("C7b_single.bz2", "bzip2")]:
        p = P(name)
        if not os.path.exists(p):
            continue
        rc, out, err = sh([tool, "-dc", p], timeout=120)
        rec(name, verify_tool=f"{tool} -dc", rc=rc, out_bytes=len(out), err=err[:200])

    # 加密组
    for name, pwd in [("C8_zipcrypto.zip", PWD_C8), ("C9_aes256.zip", PWD_C9),
                      ("C10_pwd_cn.7z", PWD_CN_7Z), ("C10b_pwd_cn_mhe.7z", PWD_CN_7Z)]:
        p = P(name)
        if not os.path.exists(p):
            continue
        rc_bad, tail_bad, _ = sevenzip_test(sevenzip, p, pwd="wrongpwd")
        rc_ok, tail_ok, _ = sevenzip_test(sevenzip, p, pwd=pwd)
        extra = {}
        if name.endswith(".zip"):
            info = sevenzip_info(sevenzip, p)
            extra["sevenzip_type"] = info.get("Type")
            extra["local_headers"] = scan_local_headers(p)
            if name == "C9_aes256.zip":
                try:
                    with pyzipper.AESZipFile(p) as z:
                        z.setpassword(pwd.encode())
                        n = z.namelist()
                        zi = z.infolist()[0]
                        extra["pyzipper"] = f"OK members={len(n)}"
                        extra["wz_aes_extra"] = zi.extra.hex()
                except Exception as e:
                    extra["pyzipper"] = f"{type(e).__name__}: {e}"
            else:
                rc, out, err = sh(f"unzip -P {pwd} -t {p}", timeout=120)
                extra["unzip_test_rc"] = rc
                extra["unzip_tail"] = (out.strip().splitlines() or [""])[-1][:120]
        rec(name, wrong_pwd_rc=rc_bad, correct_pwd_rc=rc_ok, correct_pwd_tail=tail_ok, **extra)

    # 分卷
    vols = sorted(f for f in os.listdir(dst_dir) if f.startswith("C11_"))
    for f in vols:
        rec(f, size=size_of(os.path.join(dst_dir, f)))
    if vols:
        rc, out, err = sh([sevenzip, "t", os.path.join(dst_dir, "C11_split.7z.001")], timeout=180)
        rec("C11_split.7z.001", verify_tool="7z t .001", rc=rc,
            tail=(out.strip().splitlines() or [""])[-1][:200])
    zvols = sorted(f for f in os.listdir(dst_dir) if f.startswith("C12_"))
    for f in zvols:
        rec(f, size=size_of(os.path.join(dst_dir, f)))
    if zvols:
        rc, out, err = sh([sevenzip, "t", os.path.join(dst_dir, "C12_split.zip")], timeout=180)
        rec("C12_split.zip", verify_tool="7z t .zip(set)", rc=rc,
            tail=(out.strip().splitlines() or [""])[-1][:200])

    # ---- D 组
    p = P("D1_truncated.zip")
    if os.path.exists(p):
        rc, tail, err = sevenzip_test(sevenzip, p)
        eocd = open(p, "rb").read().rfind(b"PK\x05\x06")
        rec("D1_truncated.zip", sevenzip_test_rc=rc, sevenzip_test_tail=tail,
            eocd_present=eocd >= 0, local_headers=open(p, "rb").read().count(b"PK\x03\x04"))

    p = P("D2a_eocd_cd_offset_tampered.zip")
    if os.path.exists(p):
        rc, tail, err = sevenzip_test(sevenzip, p)
        try:
            with zipfile.ZipFile(p) as z:
                zt = f"OK members={len(z.namelist())}"
        except Exception as e:
            zt = f"{type(e).__name__}: {e}"
        rec("D2a_eocd_cd_offset_tampered.zip", sevenzip_test_rc=rc, sevenzip_test_tail=tail,
            zipfile=zt, note="真实 CD 可被 local header 扫描救回")

    p = P("D2b_prefixed_2355200.zip")
    if os.path.exists(p):
        rc, tail, err = sevenzip_test(sevenzip, p)
        try:
            with zipfile.ZipFile(p) as z:
                zt = f"OK members={len(z.namelist())} bad={z.testzip()}"
        except Exception as e:
            zt = f"{type(e).__name__}: {e}"
        rec("D2b_prefixed_2355200.zip", sevenzip_test_rc=rc, sevenzip_test_tail=tail,
            zipfile=zt, uniform_delta=2355200)
        # 演示「统一偏移修正」：concat = EOCD实际位置 − size_cd − offset_cd
        data = open(p, "rb").read()
        e = data.rfind(b"PK\x05\x06")
        size_cd = int.from_bytes(data[e + 12:e + 16], "little")
        off_cd = int.from_bytes(data[e + 16:e + 20], "little")
        concat = e - size_cd - off_cd
        rec("D2b_prefixed_2355200.zip", computed_concat=concat,
            cd_at_expected=data[off_cd + concat:off_cd + concat + 4].hex())

    p = P("D3_missing_volume.7z.001")
    if os.path.exists(p):
        rc, tail, err = sevenzip_test(sevenzip, p)
        rec("D3_missing_volume.7z.001", sevenzip_test_rc=rc, sevenzip_test_tail=tail,
            err_tail=err.strip()[-200:])

    p = P("D4_corrupt_member.zip")
    if os.path.exists(p):
        rc, tail, err = sevenzip_test(sevenzip, p)
        rec("D4_corrupt_member.zip", sevenzip_test_rc=rc, sevenzip_test_tail=tail,
            err_tail=err.strip()[-300:])
        try:
            with zipfile.ZipFile(p) as z:
                bad = z.testzip()
            rec("D4_corrupt_member.zip", zipfile_testzip_bad=bad)
        except Exception as e:
            rec("D4_corrupt_member.zip", zipfile_testzip_err=f"{type(e).__name__}: {e}")

    # ---- 救援演示：只按 local header 扫描（绕过中心目录）
    for name in ["D1_truncated.zip", "D2a_eocd_cd_offset_tampered.zip", "D2b_prefixed_2355200.zip"]:
        p = os.path.join(dst_dir, name)
        if not os.path.exists(p):
            continue
        outdir = os.path.join(WORK, "rescue", name)
        shutil.rmtree(outdir, ignore_errors=True)
        try:
            ok, bad, total = rescue_zip_local_headers(p, outdir)
            rec(name, rescue_local_headers={"recovered": ok, "failed": bad, "bytes": total})
        except Exception as e:
            rec(name, rescue_local_headers=f"{type(e).__name__}: {e}")

    # ---- 实解一遍（7z x → 数出多少文件），证明「能解开」
    extract_list = ["A1_steg_modeA_plain.mp4", "A2_steg_modeA_ascii_pwd.mp4",
                    "A3_steg_modeA_cn_pwd.mp4", "A4_steg_modeB_zarchiver.mp4",
                    "A4b_steg_modeB_tailnoise_gt64k.mp4", "A6_oldstyle_bare_append.mp4",
                    "B1_tail_append_cat.mp4", "B3_png_tail.png", "B4_free_atom_interior.mp4",
                    "C1_plain.zip", "C2_plain.7z", "C8_zipcrypto.zip", "C9_aes256.zip",
                    "C10_pwd_cn.7z", "C10b_pwd_cn_mhe.7z", "D2b_prefixed_2355200.zip",
                    "D1_truncated.zip", "D2a_eocd_cd_offset_tampered.zip",
                    "D4_corrupt_member.zip", "C11_split.7z.001", "C12_split.zip"]
    pwd_map = {"A2_steg_modeA_ascii_pwd.mp4": PWD_ASCII, "A3_steg_modeA_cn_pwd.mp4": PWD_CN_STEG,
               "C8_zipcrypto.zip": PWD_C8, "C9_aes256.zip": PWD_C9,
               "C10_pwd_cn.7z": PWD_CN_7Z, "C10b_pwd_cn_mhe.7z": PWD_CN_7Z}
    for name in extract_list:
        p = os.path.join(dst_dir, name)
        if not os.path.exists(p):
            continue
        outdir = os.path.join(WORK, "x", name)
        shutil.rmtree(outdir, ignore_errors=True)
        os.makedirs(outdir, exist_ok=True)
        cmd = [sevenzip, "x", "-y", "-o" + outdir, p] + (["-p" + pwd_map[name]] if name in pwd_map else [])
        rc, out, err = sh(cmd, timeout=300)
        n = total = 0
        for root, _dirs, files in os.walk(outdir):
            for f in files:
                n += 1
                total += size_of(os.path.join(root, f))
        rec(name, extract7z={"rc": rc, "files": n, "bytes": total})

    # ---- 隐写者真源码读端回环（记录「尝试了哪些方法 / 命中哪个」）
    reveal_names = ["A1_steg_modeA_plain.mp4", "A2_steg_modeA_ascii_pwd.mp4",
                    "A3_steg_modeA_cn_pwd.mp4", "A4_steg_modeB_zarchiver.mp4",
                    "A4b_steg_modeB_tailnoise_gt64k.mp4", "A6_oldstyle_bare_append.mp4",
                    "B1_tail_append_cat.mp4", "B2_apate.mp4", "B3_png_tail.png",
                    "B4_free_atom_interior.mp4"]
    try:
        probe = reveal_probe(reveal_names, {"A2_steg_modeA_ascii_pwd.mp4": PWD_ASCII,
                                           "A3_steg_modeA_cn_pwd.mp4": PWD_CN_STEG})
        for k, v in probe.items():
            rec(k, reveal_probe=v)
    except Exception as e:
        R["_reveal_probe_error"] = f"{type(e).__name__}: {e}"
    return R


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--verify-only", action="store_true")
    ap.add_argument("--only", default=None, help="逗号分隔的分组：A,B,C,D")
    a = ap.parse_args()

    only = set(x.strip().upper() for x in a.only.split(",")) if a.only else {"A", "B", "C", "D"}
    ffmpeg = first_existing(FFMPEG_CANDIDATES)
    sevenzip = first_existing(SEVENZIP_CANDIDATES)
    mkvmerge = first_existing(MKVMERGE_CANDIDATES)
    mkvextract = first_existing(MKVEXTRACT_CANDIDATES)
    mkvinfo = first_existing(MKVINFO_CANDIDATES)
    print(f"[env] ffmpeg={ffmpeg}\n[env] 7z={sevenzip}\n[env] mkvmerge={mkvmerge}\n"
          f"[env] mkvextract={mkvextract}\n[env] mkvinfo={mkvinfo}")
    if not ffmpeg or not sevenzip:
        raise SystemExit("缺 ffmpeg 或 7z")

    os.makedirs(DATA, exist_ok=True)
    os.makedirs(WORK, exist_ok=True)
    os.makedirs(ASSETS, exist_ok=True)

    print("[1] 素材 …")
    assets = build_assets(ffmpeg)
    payload_zip = os.path.join(ASSETS, "payload.zip")
    build_payload_zip(payload_zip, assets["payload"])
    print("    payload.zip", size_of(payload_zip), "B")

    result = {}
    if not a.verify_only:
        if "A" in only:
            print("[2] A 组（隐写者真源码）…")
            result["A"] = make_A_samples(DATA, assets, mkvmerge)
            p = os.path.join(DATA, "A6_oldstyle_bare_append.mp4")
            make_A6_oldstyle(p, os.path.join(ASSETS, "cover", "cover.mp4"), payload_zip)
            print("    A6 老版裸追加:", size_of(p))
        if "B" in only:
            print("[3] B 组（手工构造）…")
            result["B"] = make_B_samples(DATA, assets, payload_zip)
        if "C" in only:
            print("[4] C 组（系统工具）…")
            result["C"] = make_C_samples(DATA, assets, sevenzip)
        if "D" in only:
            print("[5] D 组（异常样本）…")
            result["D"] = make_D_samples(DATA, assets, sevenzip)

    print("[6] 验证 …")
    ver = verify_all(DATA, assets, ffmpeg, sevenzip, mkvmerge, mkvextract, mkvinfo)
    result["verify"] = ver

    with open(os.path.join(HERE, "results.json"), "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, indent=2)

    print("\n===== 样本清单 =====")
    for fn in sorted(os.listdir(DATA)):
        print(f"  {fn:42s} {size_of(os.path.join(DATA, fn)):>10,} B")
    print(f"\n结果写入 {os.path.join(HERE, 'results.json')}")


if __name__ == "__main__":
    main()
