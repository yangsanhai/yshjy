#!/usr/bin/env python3
"""T18-6「完全离线」的机器断言 —— 不联网不是一句口头承诺。

用户 2026-11-01 的安全口径（原话）：「保证用户安全，尽量本地解决」。
所以「检查更新」直接砍掉，改为把「不联网」钉成**能失败的测试**：

  ① Rust 依赖里不许出现任何 HTTP 客户端 / TLS 栈（Cargo.toml + Cargo.lock）
  ② Flutter 依赖里不许出现任何网络包（gui/pubspec.yaml）
  ③ 源码**非注释行**里不许出现 http:// 或 https://（src/**/*.rs、gui/lib/**/*.dart）

为什么连 Cargo.lock 也查：Cargo.toml 里没写、但某个依赖把 reqwest 拖进来，
运行时一样能发请求 —— 只看直接依赖等于自欺。

为什么排除注释行：项目里确实有指着文档 / 规范的注释（比如脱敏说明里提到
`http://` 字样）。注释不会执行，不算「能联网」。

用法：
    python3 tests/offline_probe.py            # 在仓库根目录跑

退出码：0 全过；1 有违规（会列出文件:行号:内容）。
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# ① Rust 侧：HTTP 客户端 / TLS / 网络栈。命中即失败。
RUST_NET = [
    "reqwest",
    "hyper",
    "ureq",
    "isahc",
    "attohttpc",
    "surf",
    "curl",
    "libcurl",
    "tungstenite",
    "rustls",
    "native-tls",
    "openssl",
    "h2",
]


# ② Flutter 侧：网络包
DART_NET = [
    "http",
    "dio",
    "chopper",
    "graphql",
    "grpc",
    "web_socket_channel",
    "socket_io_client",
    "firebase_core",
    "firebase_analytics",
    "sentry_flutter",
    "url_launcher",
]


def strip_comment(line: str) -> str:
    """去掉行内注释（行首 `//` 或 `///` 整行都算注释）。

    只处理最常见的 `//` 形式。`/* */` 块注释在项目里没有跨行放 URL 的情况，
    所以不为了它引入一个不完整的解析器（宁可漏报一条注释，也不误伤代码）。
    """
    s = line.strip()
    if s.startswith("//"):
        return ""
    idx = line.find("//")
    return line[:idx] if idx >= 0 else line


def scan_cargo() -> list[str]:
    bad: list[str] = []
    for name in ("Cargo.toml", "Cargo.lock"):
        p = ROOT / name
        if not p.exists():
            bad.append(f"{name}: 文件不存在（跑错目录了？）")
            continue
        for no, line in enumerate(p.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            s = strip_comment(line).strip()
            if not s:
                continue
            for dep in RUST_NET:
                # 词边界：前面不能是字母数字，后面不能是字母数字之外的 `-`（`openssl-sys`
                # 也要算命中）；这样 `hyperx` 这类无关名字不会被误判。
                if re.search(rf"(?<![A-Za-z0-9]){re.escape(dep)}(?![A-Za-z0-9])", s):
                    bad.append(f"{name}:{no}: 出现网络依赖 `{dep}` → {s}")
                    break
    return bad


def scan_pubspec() -> list[str]:
    p = ROOT / "gui" / "pubspec.yaml"
    if not p.exists():
        return ["gui/pubspec.yaml: 文件不存在"]
    bad: list[str] = []
    in_deps = False
    for no, line in enumerate(p.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        s = strip_comment(line).strip()
        if not s:
            continue
        if re.match(r"^(dev_)?dependencies:\s*$", s):
            in_deps = True
            continue
        if s.endswith(":") and not s.startswith(" "):
            in_deps = False
            continue
        if not in_deps:
            continue
        name = s.split(":")[0].strip()
        if name in DART_NET:
            bad.append(f"gui/pubspec.yaml:{no}: 出现网络包 `{name}` → {s}")
    return bad


def scan_sources() -> list[str]:
    bad: list[str] = []
    patterns = [("src/**/*.rs", "src"), ("gui/lib/**/*.dart", "gui/lib")]
    for pat, _label in patterns:
        for p in sorted(ROOT.glob(pat)):
            rel = p.relative_to(ROOT)
            for no, line in enumerate(p.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                code = strip_comment(line)
                if "http://" in code or "https://" in code:
                    bad.append(f"{rel}:{no}: 代码里出现 URL → {line.strip()[:120]}")
    return bad


def main() -> int:
    cargo = scan_cargo()
    pubspec = scan_pubspec()
    src = scan_sources()
    allbad = cargo + pubspec + src

    if allbad:
        print("✗ 「完全离线」被破坏，下列位置能联网/含网络依赖：")
        for b in allbad:
            print("   " + b)
        print()
        print("说明：如果确实需要在注释里提 URL，请写在 `//` 之后（注释不执行）。")
        return 1

    print("✓ 离线断言全过：")
    print("   · Cargo.toml / Cargo.lock 无 HTTP/TLS 依赖")
    print("   · gui/pubspec.yaml 无网络包")
    print("   · 源码（非注释）无 http(s):// 网址")
    return 0


if __name__ == "__main__":
    sys.exit(main())
