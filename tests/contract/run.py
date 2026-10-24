#!/usr/bin/env python3
"""协议契约测试：把 `--json` 事件流的**结构**固化成 golden。

为什么只比结构、不比具体值：
  路径、耗时、字节数每次都不同；但「事件名 + 字段名 + 字段类型 + 退出码」是
  界面（Flutter 壳）与引擎之间的契约。结构悄悄一改，界面就会静默失效 ——
  这正是这套测试要挡的事（阶段 1 梯队三·第 1 项）。

用法：
    python3 tests/contract/run.py            # 跑对比
    python3 tests/contract/run.py --update   # 重新生成 golden（真改了协议、确认过再用）

退出码：0 全绿 / 1 有结构变化
"""
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
DATA = os.path.join(ROOT, "tests", "data")
GOLDEN = os.path.join(HERE, "golden")
BIN = os.environ.get("YSHJY_BIN", os.path.join(ROOT, "target", "release", "yshjy"))

# name, 参数（{out} = 本次专用临时输出目录，{data} = tests/data）, 说明
CASES = [
    ("extract_zip", ["--json", "-o", "{out}", "{data}/C1_plain.zip"], "解压普通 zip"),
    ("list_zip", ["--json", "--list", "{data}/C1_plain.zip"], "只看内容（--list）"),
    ("extract_7z", ["--json", "-o", "{out}", "{data}/C2_plain.7z"], "解压 7z"),
    ("extract_tar_gz", ["--json", "-o", "{out}", "{data}/C4_plain.tar.gz"], "解压 tar.gz"),
    ("extract_enc_ok", ["--json", "-p", "zipcrypto123", "-o", "{out}", "{data}/C8_zipcrypto.zip"], "带正确密码"),
    ("extract_enc_wrong", ["--json", "-p", "错的密码", "-o", "{out}", "{data}/C8_zipcrypto.zip"], "密码错误"),
    ("extract_truncated", ["--json", "-o", "{out}", "{data}/D1_truncated.zip"], "下载没下完的包"),
    ("rescue", ["--json", "--rescue", "-o", "{out}", "{data}/D1_truncated.zip"], "救援模式"),
    ("cover", ["--json", "--extract-cover", "-o", "{out}", "{data}/A1_steg_modeA_plain.mp4"], "只取原版封面视频"),
    ("select", ["--json", "--select", "*.txt", "-o", "{out}", "{data}/C1_plain.zip"], "只解一部分"),
    ("pack", ["--json", "ys", "-o", "{out}", "{data}/C1_plain.zip"], "打包成伪装文件"),
    ("caps", ["--json", "--caps"], "各格式能力表"),
]


def shape_of(v):
    """把一份 JSON 值压成「形状」：键名 + 类型，值本身丢掉。"""
    if isinstance(v, dict):
        return {k: shape_of(x) for k, x in v.items()}
    if isinstance(v, list):
        # 列表长度随包内容而定，不是契约；只固化第一个元素的形状
        return [shape_of(v[0])] if v else []
    return type(v).__name__


def collect(stdout):
    """按行抓 JSON 事件，聚合成 {事件名: 形状}。"""
    events = {}
    for line in stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(obj, dict):
            continue
        for name, payload in obj.items():
            if name == "JobFinished" and isinstance(payload, dict):
                # outcome 是稳定枚举：Success / Partial / Cancelled 带对象，
                # Failed 直接就是**错误码字符串**（界面的 humanize 靠它查文案）。
                # 所以这里不压成类型，而是把错误码的**实际值**固化 —— 码一改名，界面文案就失配。
                oc = payload.get("outcome")
                if isinstance(oc, dict):
                    fixed = {}
                    for k, v in oc.items():
                        fixed[k] = v if isinstance(v, str) else shape_of(v)
                    events.setdefault(name, {"job": shape_of(payload.get("job")), "outcome": fixed})
                    continue
            events.setdefault(name, shape_of(payload))
    return events


def diff(want, got, path=""):
    """逐层比形状，返回人话差异列表。"""
    out = []
    if isinstance(want, dict) and isinstance(got, dict):
        for k in sorted(want):
            if k not in got:
                out.append(f"{path}.{k} 不见了")
            else:
                out.extend(diff(want[k], got[k], f"{path}.{k}"))
        for k in sorted(got):
            if k not in want:
                out.append(f"{path}.{k} 是新加的字段（契约变更）")
    elif isinstance(want, list) and isinstance(got, list):
        if want and got:
            out.extend(diff(want[0], got[0], path + "[]"))
        elif bool(want) != bool(got):
            out.append(f"{path}: 列表有没有元素变了")
    elif want != got:
        out.append(f"{path}: 类型 {want} → {got}")
    return out


def run_case(name, args, tmp):
    out_dir = os.path.join(tmp, name)
    os.makedirs(out_dir, exist_ok=True)
    argv = [BIN] + [a.replace("{out}", out_dir).replace("{data}", DATA) for a in args]
    try:
        p = subprocess.run(argv, capture_output=True, text=True, timeout=300, cwd=ROOT)
    except subprocess.TimeoutExpired:
        return None, f"超时（300 秒）：{' '.join(argv)}"
    return {"rc": p.returncode, "events": collect(p.stdout)}, None


def main():
    update = "--update" in sys.argv
    if not os.path.exists(BIN):
        print(f"找不到引擎：{BIN}")
        return 2
    os.makedirs(GOLDEN, exist_ok=True)

    tmp = tempfile.mkdtemp(prefix="yshjy_contract_")
    bad = 0
    for name, args, desc in CASES:
        got, err = run_case(name, args, tmp)
        if got is None:
            print(f"  ✗ {name}（{desc}）：{err}")
            bad += 1
            continue

        gpath = os.path.join(GOLDEN, f"{name}.json")
        if update or not os.path.exists(gpath):
            with open(gpath, "w", encoding="utf-8") as f:
                json.dump(got, f, ensure_ascii=False, indent=2, sort_keys=True)
                f.write("\n")
            print(f"  ✓ {name}（{desc}）：已写入 golden")
            continue

        with open(gpath, encoding="utf-8") as f:
            want = json.load(f)
        problems = diff(want, got)
        if problems:
            print(f"  ✗ {name}（{desc}）：")
            for p in problems:
                print(f"      · {p}")
            bad += 1
        else:
            print(f"  ✓ {name}（{desc}）")

    print()
    if bad:
        print(f"  协议契约：{bad} 项与 golden 不一致（真改了协议就 --update 并说明原因）")
        return 1
    print(f"  协议契约：{len(CASES)} 项全部符合 golden")
    return 0


if __name__ == "__main__":
    sys.exit(main())
