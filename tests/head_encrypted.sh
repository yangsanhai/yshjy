#!/usr/bin/env bash
# 7z 头加密（`-mhe=on`）的四种场景 + 一个真 bug 的回归
#
# 由来（2026-10-29）：头加密的包**连文件清单都读不到**，之前一律报「这个包带锁，需要解压码」，
# 用户会以为「给了密码就能列」——而实际情况是「不给密码连里面有几个文件都不知道」，解法不同
# （前者输密码就行，后者要先去问发布者要密码）。所以拆出独立错误码 `HEAD_ENCRYPTED`。
#
# **顺带抓到的真 bug**：`do_list` 里 `preview(&path, &ctx, None)` 把密码写死成 `None`，
# 于是 `yshjy --list -p 密码 头加密.7z` 永远报「需要解压码」—— 手里的密码根本没用上。
#
# 断言：
#   1. 头加密 + 无密码：退出码 4，且是 `HEAD_ENCRYPTED`（不是笼统的 NEED_PASSWORD）
#   2. 头加密 + **正确密码**：退出码 0 且列出条目（这条就是那个 bug 的回归）
#   3. 头加密 + 错误密码：退出码 4 且是 `WRONG_PASSWORD`（别把「给错了」说成「没给」）
#   4. 内容加密（`-mhe=off`）+ 无密码：**不许误报成头加密**，仍然能列清单
#   5. 头加密 + 正确密码 + 解压：真能解出文件
#
# 用法：bash tests/head_encrypted.sh [二进制路径]
set -u
cd "$(dirname "$0")/.." || exit 1

BIN="${1:-}"
if [ -z "$BIN" ]; then
    for c in target/release/yshjy target/debug/yshjy; do
        [ -x "$c" ] && { BIN="$c"; break; }
    done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "❌ 找不到可用的 yshjy 二进制（先 cargo build --release）"
    exit 1
fi
case "$BIN" in /*) ;; *) BIN="$(pwd)/$BIN" ;; esac

if ! command -v 7z >/dev/null 2>&1; then
    echo "  – 跳过：没装 7z（sudo apt install -y p7zip-full 后才会跑）"
    exit 0
fi

n_pass=0
n_fail=0
ok()  { echo "  ✓ $1"; n_pass=$((n_pass+1)); }
bad() { echo "  ✗ $1"; n_fail=$((n_fail+1)); }

D="${TMPDIR:-/tmp}/yshjy_head_enc_$$"
mkdir -p "$D/src"
cd "$D/src" || exit 1
printf 'hello 杨三海\n' > a.txt

# 头加密（清单也加密）与只加密内容（清单可读）各造一份
7z a -t7z -mhe=on -p123 头加密.7z a.txt >/dev/null 2>&1 || { echo "❌ 造头加密样本失败"; exit 1; }
7z a -t7z -mhe=off -p123 内容加密.7z a.txt >/dev/null 2>&1 || { echo "❌ 造内容加密样本失败"; exit 1; }

echo "── 1. 头加密 + 无密码：必须是 HEAD_ENCRYPTED ──"
out=$("$BIN" --list 头加密.7z 2>&1); rc=$?
if [ "$rc" = 4 ] && printf '%s' "$out" | grep -q 'HEAD_ENCRYPTED'; then
    ok "退出码 4 且码是 HEAD_ENCRYPTED（说清「清单也加密了」）"
else
    bad "rc=$rc，没看到 HEAD_ENCRYPTED"
    printf '%s\n' "$out" | sed 's/^/      /' | head -6
fi

echo "── 2. 头加密 + 正确密码：能列清单（do_list 密码丢失的回归）──"
out=$("$BIN" --list -p 123 头加密.7z 2>&1); rc=$?
if [ "$rc" = 0 ] && printf '%s' "$out" | grep -q 'a.txt'; then
    ok "退出码 0 且列出了 a.txt"
else
    bad "rc=$rc，没列出条目（密码没传进 preview 的老 bug 又回来了？）"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 3. 头加密 + 错误密码：必须是 WRONG_PASSWORD ──"
out=$("$BIN" --list -p 999 头加密.7z 2>&1); rc=$?
if [ "$rc" = 4 ] && printf '%s' "$out" | grep -q 'WRONG_PASSWORD'; then
    ok "退出码 4 且码是 WRONG_PASSWORD（「给错了」不许说成「没给」）"
else
    bad "rc=$rc，没看到 WRONG_PASSWORD"
    printf '%s\n' "$out" | sed 's/^/      /' | head -6
fi

echo "── 4. 内容加密 + 无密码：不许误报成头加密 ──"
out=$("$BIN" --list 内容加密.7z 2>&1); rc=$?
if [ "$rc" = 0 ] && printf '%s' "$out" | grep -q 'a.txt' && ! printf '%s' "$out" | grep -q 'HEAD_ENCRYPTED'; then
    ok "退出码 0、列出条目、没误报头加密"
else
    bad "rc=$rc（内容加密的包清单是可读的，必须能列出来）"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo "── 5. 头加密 + 正确密码 + 解压：真能解出文件 ──"
out=$("$BIN" -p 123 -o "$D/out" 头加密.7z 2>&1); rc=$?
if [ "$rc" = 0 ] && [ -f "$D/out/a.txt" ]; then
    ok "退出码 0 且 a.txt 已落地"
else
    bad "rc=$rc，产物存在=$( [ -f "$D/out/a.txt" ] && echo 是 || echo 否 )"
    printf '%s\n' "$out" | sed 's/^/      /' | head -8
fi

echo
echo "── 结果：通过 $n_pass · 失败 $n_fail ──"
[ "$n_fail" -eq 0 ] || exit 1
