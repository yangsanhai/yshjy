#!/usr/bin/env python3
"""自解压存根回归测试（Linux 上即可跑）。

用法：
    python3 tests/sfx_stub_test.py [存根二进制]

原理：ELF / PE 尾部追加数据不影响执行，所以可以直接拿「Linux 版存根」
拼一个包，验证「读尾标 → 定位 ZIP → 解压 → 目录结构」这段核心逻辑；
Windows 版用同源代码，交叉编译后同样能过（wine 里设 YSHJY_SFX_SILENT=1）。

覆盖：
  1. 正常包：中文文件名 + 子目录 → 解到「exe 同名文件夹」
  2. 尾标错误 → 明确拒绝，不产生半成品目录
  3. 长度对不上（截断）→ 拒绝
  4. 重名目录 → 自动加 (1)，不覆盖已有文件
"""
import os
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile

STUB = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "sfx/target/release/yshjy-sfx")
MAGIC = b"YSHSFX01"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_sfx_"))


def 断言(cond, msg):
    if not cond:
        print(f"❌ {msg}")
        sys.exit(1)
    print(f"✅ {msg}")


def 造zip(路径: pathlib.Path):
    with zipfile.ZipFile(路径, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("笔记/说明.txt", "被藏起来的内容\n")
        z.writestr("根目录.txt", "hello sfx\n")
        z.writestr("空目录/", "")


def 拼包(目标: pathlib.Path, zip文件: pathlib.Path, 坏尾标=False):
    stub = STUB.read_bytes()
    zdata = zip文件.read_bytes()
    偏移 = len(stub)
    目标.write_bytes(stub + zdata + (b"BROKEN!!" if 坏尾标 else MAGIC)
                     + struct.pack("<QQ", 偏移, len(zdata)))
    目标.chmod(0o755)


def 跑(包: pathlib.Path):
    env = dict(os.environ, YSHJY_SFX_SILENT="1", YSHJY_SFX_NO_OPEN="1")
    # 正式存根是 Windows PE（MZ）→ wine；开发机的 Linux 版直接跑
    if STUB.read_bytes()[:2] == b"MZ":
        env["WINEDEBUG"] = "-all"
        cmd = ["wine", str(包)]
    else:
        cmd = [str(包)]
    return subprocess.run(cmd, cwd=str(包.parent), env=env,
                          capture_output=True, text=True, timeout=180)


断言(STUB.exists(), f"存根存在：{STUB}")

# 1. 正常包
z = W / "内容.zip"
造zip(z)
包 = W / "游戏包.exe"
拼包(包, z)
r = 跑(包)
输出目录 = W / "游戏包"
断言(输出目录.is_dir(), "解到了同名文件夹")
断言((输出目录 / "笔记" / "说明.txt").read_text(encoding="utf-8") == "被藏起来的内容\n",
     "中文路径 + 内容正确")
断言((输出目录 / "根目录.txt").read_text(encoding="utf-8") == "hello sfx\n",
     "根目录文件正确")
断言((输出目录 / "空目录").is_dir(), "空目录也建出来了")

# 2. 尾标错误
坏 = W / "坏包.exe"
拼包(坏, z, 坏尾标=True)
r = 跑(坏)
断言(not (W / "坏包").exists(), "尾标错误时不解压")
断言("不是杨三海自解压包" in (r.stderr or ""), "尾标错误有明确提示")

# 3. 截断（尾标还在，但 ZIP 少了一截 → 长度对不上）
截 = W / "截断包.exe"
stub数据 = STUB.read_bytes()
zdata = z.read_bytes()
偏移 = len(stub数据)
截.write_bytes(stub数据 + zdata[:-100] + MAGIC + struct.pack("<QQ", 偏移, len(zdata)))
截.chmod(0o755)
r = 跑(截)
断言(not (W / "截断包").exists(), "截断包不解压")
断言("改动过" in (r.stderr or ""), "截断有明确提示")

# 4. 重名目录
(W / "游戏包 (1)").mkdir()
(W / "游戏包 (1)" / "旧文件.txt").write_text("别覆盖我", encoding="utf-8")
r = 跑(包)
断言((W / "游戏包 (1)" / "旧文件.txt").read_text(encoding="utf-8") == "别覆盖我",
     "已有目录不被覆盖")
断言((W / "游戏包 (2)" / "根目录.txt").exists(), "重名时自动加 (2)")

print(f"\n全部通过。测试目录：{W}")
