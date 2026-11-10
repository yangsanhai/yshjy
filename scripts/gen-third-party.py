#!/usr/bin/env python3
"""生成 `gui/assets/third_party.json` —— 界面「开源许可」页的数据源。

为什么要脚本生成、不手写
------------------------
手抄的清单一定会过期：加一个依赖、升一个版本，谁都不会记得回来改那一大坨。
（本项目在清单这类「第二份真源」上已经栽过：man 页版本号、补全项数、ffi 头文件。
 参考 `refs/tech/hiddify-core-c-abi.md` 第 2 节 —— 人家连头文件都是构建产物。）

所以唯一真源是 `cargo metadata`，本脚本只做搬运 + 过滤 + 排序。

数据从哪来
----------
`cargo metadata --format-version 1 --locked`：拿包名 / 版本 / license 表达式 /
repository。`--locked` 保证和 Cargo.lock 一致（不会因为本地有新解析就漂）。

谁会被列进去
------------
- 全部依赖（含 dev-dependencies）。dev 里的东西不进最终二进制，但「用过」是事实，
  多列不亏；少列才是漏。
- 排除自己（`source` 为 null 的路径包 —— 也就是本仓库）。

许可是空的怎么办
----------------
有的 crate 不写 `license`，只放 `license_file`。这种情况写
`license_file:<文件名>`，让人知道去它的包里看，而不是假装是 MIT。

用法
----
    python3 scripts/gen-third-party.py            # 生成
    python3 scripts/gen-third-party.py --check    # 只比对，不一致退出码 1（给 CI / 发版前用）

产物要跟着提交：界面运行时读它，不联网。
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "gui" / "assets" / "third_party.json"

# ── 参考与致谢（人工维护，改动少） ──────────────────────────────────────────
# 这些项目**没有一行代码进我们的产物**，只读过它们的实现/文档来解题。
# 列出来是「用了别人的思路要说清」这条原则，不是许可证义务 —— 所以这里不写
# 许可证快照（会过期），只给仓库地址，让读者自己去看。
CREDITS = [
    {
        "name": "PeaZip",
        "url": "https://github.com/peazip/PeaZip",
        "what": "界面信息层次的参照：格式能力表、任务队列的排版，以及「别把技术细节塞给普通用户」这条口径",
    },
    {
        "name": "7-Zip",
        "url": "https://www.7-zip.org/",
        "what": "7z 格式细节与命令行行为的事实基准；测试样本用它交叉验证（不链接、不内嵌）",
    },
    {
        "name": "NanaZip",
        "url": "https://github.com/M2Team/NanaZip",
        "what": "自解压（SFX）三档结构的思路；本项目尚未实现，仅记录在案",
    },
    {
        "name": "RustDesk",
        "url": "https://github.com/rustdesk/rustdesk",
        "what": "Flutter 桌面工程的目录拆法、设置页快捷键卡片的摆法、Android 打包参数（minSdk / 签名）",
    },
    {
        "name": "LocalSend",
        "url": "https://github.com/localsend/localsend",
        "what": "Android 分区存储的落地写法：SAF 目录选择、content:// 反解真实路径、自己写 DocumentFile 图快",
    },
    {
        "name": "hiddify-app",
        "url": "https://github.com/hiddify/hiddify-app",
        "what": "C ABI 的工程做法（头文件由构建生成、不入库）—— 用来对照我们手写头文件的隐患",
    },
    {
        "name": "libarchive",
        "url": "https://github.com/libarchive/libarchive",
        "what": "格式识别与错误分类的事实参考；本项目不链接它（引擎是纯 Rust 实现）",
    },
]


def cargo_metadata() -> dict:
    """跑 cargo metadata；失败就骂人，别生成半份清单。"""
    try:
        out = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        sys.exit("找不到 cargo —— 先在能编译本项目的环境里跑这个脚本")
    except subprocess.CalledProcessError as e:
        sys.exit(f"cargo metadata 失败：\n{e.stderr}")
    return json.loads(out.stdout)


def build_payload(meta: dict) -> dict:
    deps = []
    for p in meta["packages"]:
        # `source` 为 null = 本地路径包（就是我们自己），不列
        if p.get("source") is None:
            continue
        lic = p.get("license")
        if not lic:
            lf = p.get("license_file")
            lic = f"license_file:{Path(lf).name}" if lf else "未标注（见其源码仓库）"
        url = p.get("repository") or p.get("homepage") or ""
        deps.append(
            {
                "name": p["name"],
                "version": p["version"],
                "license": lic,
                "url": url,
            }
        )
    deps.sort(key=lambda d: (d["name"].lower(), d["version"]))

    return {
        "_note": "由 scripts/gen-third-party.py 生成，勿手改。重跑：python3 scripts/gen-third-party.py",
        "_source": "cargo metadata --format-version 1 --locked",
        "_count": len(deps),
        "app": {
            "name": "yshjy",
            "license": "MIT",
            "url": "LICENSE",
        },
        "deps": deps,
        "credits": CREDITS,
    }


def main() -> int:
    payload = build_payload(cargo_metadata())
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"

    if "--check" in sys.argv:
        if not OUT.exists():
            print(f"✗ {OUT.relative_to(REPO)} 不存在 —— 跑一次不带 --check 的生成")
            return 1
        old = OUT.read_text(encoding="utf-8")
        if old != text:
            print("✗ 清单和 Cargo.lock 不一致 —— 依赖变过了，重跑一次生成")
            return 1
        print(f"✓ 清单是最新的（{payload['_count']} 个依赖）")
        return 0

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    unlicensed = sum(1 for d in payload["deps"] if d["license"].startswith("未标注"))
    print(f"✓ 写了 {OUT.relative_to(REPO)}：{payload['_count']} 个依赖")
    if unlicensed:
        print(f"  ⚠ 其中 {unlicensed} 个没有 license 字段（清单里已如实标注）")
    print(f"  致谢列表：{len(payload['credits'])} 项（人工维护，在脚本顶部 CREDITS）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
