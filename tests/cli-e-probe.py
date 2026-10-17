#!/usr/bin/env python3
"""第21批组E：CLI 扫尾参数实测（--config / --layout / --cover / --keep /
--password-file / --no-auto-password / -v / --log / --no-color）"""
import os
import shutil
import subprocess
import time

BIN = os.path.expanduser("~/hermes/yshjy/target/release/yshjy")
W = os.path.expanduser(f"~/.hermes/cache/scratch/cli_e_{int(time.time())}")
os.makedirs(os.path.join(W, "src"))
open(os.path.join(W, "src", "a.txt"), "w").write("hello cli\n")
open(os.path.join(W, "src", "中文.txt"), "w").write("中文\n")

fails = []


def run(*args, **kw):
    return subprocess.run([BIN, *args], cwd=W, capture_output=True, text=True, **kw)


def check(name, ok, info=""):
    print(("PASS " if ok else "FAIL ") + name + (("  " + str(info)) if info else ""))
    if not ok:
        fails.append(name)


# ① --config：指定一个 language=en 的配置，输出应当变英文
open(os.path.join(W, "en.toml"), "w").write('[general]\nlanguage = "en"\n')
r = run("--config", "en.toml", "--version")
check("--config 生效（版本行变英文）", "common" in r.stdout.lower() or "supports" in r.stdout.lower(),
      r.stdout.splitlines()[:1])
r = run("--config", "不存在的.toml", "--version")
check("--config 文件不存在时警告并回退", r.returncode == 0 and "不存在" in r.stderr, r.stderr.strip()[:60])

# ② --layout
r = run("ys", "-o", "l1.mp4", "--layout", "L1", "src")
head = open(os.path.join(W, "l1.mp4"), "rb").read(12)
check("--layout L1 打包成功且是 MP4 头", r.returncode == 0 and head[4:8] == b"ftyp", head)
r = run("ys", "-o", "l5.mp4", "--layout", "L5", "src")
check("--layout L5 明确拒绝（退出 2）", r.returncode == 2 and "不做" in r.stderr, r.stderr.strip()[:70])
r = run("ys", "-o", "l9.mp4", "--layout", "L9", "src")
check("--layout 非法值报用法错误", r.returncode == 2, r.returncode)

# ③ --cover：自己给的封面文件要原样进产物开头
png = next((p for p in [
    "/usr/share/pixmaps/debian-logo.png",
    "/usr/share/pixmaps/firefox.png",
] if os.path.exists(p)), None)
if png is None:
    for root, _, files in os.walk("/usr/share/pixmaps"):
        png = next((os.path.join(root, f) for f in files if f.endswith(".png")), None)
        if png:
            break
r = run("ys", "-o", "my.png", "--cover", png, "src")
prod = open(os.path.join(W, "my.png"), "rb").read(8)
check("--cover 用上了自定义封面（PNG 魔数）", r.returncode == 0 and prod == b"\x89PNG\r\n\x1a\n", prod)
r = run("ys", "-o", "x.mp4", "--cover", "没有这个文件", "src")
check("--cover 读不到时报错（不静默换素材）", r.returncode == 1 and "读不了" in r.stderr, r.stderr.strip()[:60])

# ④ --keep：中间 ZIP 保留，并打印路径
r = run("ys", "-o", "keep.mp4", "--keep", "src")
line = [l for l in r.stdout.splitlines() if "中间文件" in l]
kept = line and os.path.exists(line[0].split("：", 1)[1].strip())
check("--keep 保留中间 ZIP 并打印路径", bool(kept), line[:1])

# ⑤ --password-file
run("ys", "-o", "pw.mp4", "-p", "密码123", "--no-readme", "src")
open(os.path.join(W, "pw.txt"), "w").write("# 注释行\n错的密码\n密码123\n")
r = run("-o", "o1", "--password-file", "pw.txt", "pw.mp4")
check("--password-file 里第 2 条能试出来", r.returncode == 0, r.returncode)
open(os.path.join(W, "bad.txt"), "w").write("错的密码\n")
r = run("-o", "o2", "--password-file", "bad.txt", "pw.mp4")
check("--password-file 全错时报密码问题（4）", r.returncode == 4, r.returncode)
r = run("-o", "o3", "--password-file", "没这个.txt", "pw.mp4")
check("--password-file 文件不存在报用法错误", r.returncode == 2 and "不存在" in r.stderr, r.stderr.strip()[:50])

# ⑥ --no-auto-password（旁边有说明书写着密码）
run("ys", "-o", "note.mp4", "-p", "密码123", "src")
r = run("-o", "n1", "note.mp4")
check("默认会自动从说明抓密码", r.returncode == 0, r.returncode)
r = run("--no-auto-password", "-o", "n2", "note.mp4")
check("--no-auto-password 关掉自动抓密码（4）", r.returncode == 4, r.returncode)

# ⑦ -v / --log
r = run("--log", "run.log", "-v", "-o", "v1", "note.mp4")
log = open(os.path.join(W, "run.log")).read() if os.path.exists(os.path.join(W, "run.log")) else ""
check("--log 写出日志文件", bool(log.strip()), len(log))
check("-v 日志里有事件/诊断细节", ("诊断" in log) or ("事件" in log), log.splitlines()[:1])

# ⑧ --no-color
r = run("--no-color", "-o", "c1", "note.mp4")
check("--no-color 输出没有颜色转义", "\x1b[3" not in r.stdout and "\x1b[3" not in r.stderr, repr(r.stdout[:40]))

print("\n工作目录:", W)
print("结果：" + ("全部通过" if not fails else f"{len(fails)} 项失败: {fails}"))
raise SystemExit(1 if fails else 0)
