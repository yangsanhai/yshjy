#!/usr/bin/env python3
"""Linux `.sh` 自解压回归测试（第 23 批 P23-5）。

用法：
    python3 tests/protocol_sfx_linux.py [path/to/yshjy]

覆盖：
 1. 协议 `sfx:"linux"` 生成 .sh；开头是 #!/bin/sh，结尾紧跟 ZIP
 2. 产物可执行（权限 755），直接运行能解出内容
 3. 带解压码：运行时装 YSHJY_SFX_PASSWORD 能解开
 4. 后缀不是 .sh → BAD_PARAM；分卷/cover 仍互斥
 5. CLI `--sfx-linux` 同样能出包并解开
"""
import json
import os
import pathlib
import random
import stat
import subprocess
import sys
import tempfile

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/yshjy"
W = pathlib.Path(tempfile.mkdtemp(prefix="yshjy_sfxsh_"))

SRC = W / "内容"
SRC.mkdir()
random.seed(21)
(SRC / "说明.txt").write_text("Linux 自解压测试\n", encoding="utf-8")
(SRC / "数据.bin").write_bytes(bytes(random.getrandbits(8) for _ in range(3000)))
(SRC / "子目录").mkdir()
(SRC / "子目录" / "深层.txt").write_text("深层内容\n", encoding="utf-8")

proc = subprocess.Popen([BIN, "--serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, text=True, bufsize=1)
_id = 0


def 发(**kw):
    global _id
    _id += 1
    proc.stdin.write(json.dumps({"id": _id, **kw}, ensure_ascii=False) + "\n")
    proc.stdin.flush()
    return _id


def 收(mid, 超时=180):
    import time
    截止 = time.time() + 超时
    while time.time() < 截止:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("引擎把管道关了")
        v = json.loads(line)
        if v.get("type") == "event":
            continue
        if v.get("id") == mid:
            return v
    raise TimeoutError(f"等 id={mid} 超时")


def 断言(c, m):
    if not c:
        print(f"❌ {m}")
        sys.exit(1)
    print(f"✅ {m}")


def 跑(p, outdir, env_extra=None):
    目标 = W / outdir
    目标.mkdir(exist_ok=True)
    包 = 目标 / p.name
    包.write_bytes(p.read_bytes())
    包.chmod(0o755)
    env = dict(os.environ, YSHJY_SFX_NO_OPEN="1")
    if env_extra:
        env.update(env_extra)
    return subprocess.run(["sh", str(包)], cwd=str(目标), env=env,
                          capture_output=True, text=True, timeout=180), 目标


# 1. 协议生成 .sh
sh = W / "自解压包.sh"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(sh), mode="disguise",
         sfx="linux", checksum=False, readme=False))
断言(r.get("ok"), f"sfx=linux 打包成功（{r.get('error')}）")
断言(r["result"].get("sfx_kind") == "linux", f"结果 sfx_kind=linux（实际 {r['result'].get('sfx_kind')}）")
数据 = sh.read_bytes()
断言(数据.startswith(b"#!/bin/sh"), "产物以 #!/bin/sh 开头")
断言(b"ZIP_OFFSET=" in 数据 and b"ZIP_SIZE=" in 数据, "脚本里写死了偏移与长度")
断言(b"PK\x03\x04" in 数据, "ZIP 数据紧跟在脚本后面")
断言(stat.S_IMODE(sh.stat().st_mode) & 0o111, "产物有可执行权限")

# 2. 直接运行 → 解出内容
r, 目录 = 跑(sh, "跑1")
断言(r.returncode == 0, f"运行退出码 0（实际 {r.returncode}）\n{r.stderr[-400:]}")
解到 = 目录 / "自解压包"
断言((解到 / "内容" / "说明.txt").read_text(encoding="utf-8") == "Linux 自解压测试\n",
     "解出 内容/说明.txt（顶层目录名保留）")
断言((解到 / "内容" / "子目录" / "深层.txt").exists(), "子目录结构保留")

# 3. 带解压码
sh_pw = W / "加密自解压.sh"
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(sh_pw), mode="disguise",
         sfx="linux", password="pw123", checksum=False, readme=False))
断言(r.get("ok"), f"带解压码 .sh 打包成功（{r.get('error')}）")
r, 目录 = 跑(sh_pw, "跑2", {"YSHJY_SFX_PASSWORD": "pw123"})
断言(r.returncode == 0, f"带密码运行成功（rc={r.returncode}）\n{r.stderr[-300:]}")
断言((目录 / "加密自解压" / "内容" / "说明.txt").exists(), "带解压码的包能解开")

# 4. 参数校验
r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "错后缀.exe"), mode="disguise", sfx="linux"))
断言(not r["ok"] and r["error"]["code"] == "BAD_PARAM", "linux 自解压非 .sh → BAD_PARAM")

r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "分卷.sh"), mode="disguise",
         sfx="linux", split=1024 * 1024))
断言(not r["ok"] and r["error"]["code"] == "BAD_PARAM", "linux 自解压 + 分卷 → BAD_PARAM")

r = 收(发(cmd="pack", inputs=[str(SRC)], out=str(W / "带壳.sh"), mode="disguise",
         sfx="linux", cover=str(SRC / "说明.txt")))
断言(not r["ok"] and r["error"]["code"] == "BAD_PARAM", "linux 自解压 + cover → BAD_PARAM")

proc.stdin.close()
proc.wait(timeout=10)

# 5. CLI
cli_sh = W / "命令行自解压.sh"
p = subprocess.run([BIN, "ys", str(SRC), "-o", str(cli_sh), "--sfx-linux",
                    "--no-checksum", "--no-readme"],
                   capture_output=True, text=True, timeout=180)
断言(p.returncode == 0, f"CLI --sfx-linux 退出码 0（实际 {p.returncode}）\n{p.stderr[-400:]}")
断言(cli_sh.exists() and cli_sh.read_bytes().startswith(b"#!/bin/sh"), "CLI 产物是 .sh 自解压")
r, 目录 = 跑(cli_sh, "跑3")
断言(r.returncode == 0, f"CLI 产物能运行（rc={r.returncode}）\n{r.stderr[-300:]}")
断言((目录 / "命令行自解压" / "内容" / "说明.txt").exists(), "CLI 产物解出了内容")

print("\n全部通过（Linux .sh 自解压 11 组断言）")
