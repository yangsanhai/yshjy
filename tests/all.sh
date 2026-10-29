#!/usr/bin/env bash
# 一条命令跑完全部验证 —— 「敢改的底子」
#
#   默认（快，约 40–60 秒）：
#     代码格式检查 → 编译 → 单元测试 → clippy → 6 个协议探针 → 界面测试
#   --slow（几十分钟，发版前跑）：
#     额外跑「300 MiB 大 RAR」「RAR 分卷」这类造样本的慢测试
#
# 用法：
#   bash tests/all.sh            # 平时改完代码跑这个
#   bash tests/all.sh --slow     # 发版前
#   bash tests/all.sh --no-gui   # 只验引擎（改界面的人常用）
#
# 设计原则：
#   1. **不删任何文件**（临时目录交给 mktemp，测完不清理也不影响仓库）
#   2. 每项独立计数，失败也继续跑完（一次看全所有问题，不要修一个跑一遍）
#   3. 最后一张表说清：过了几项、挂在哪几项

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

SLOW=0
GUI=1
for a in "$@"; do
  case "$a" in
    --slow) SLOW=1 ;;
    --no-gui) GUI=0 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "不认识的参数：$a（可用：--slow / --no-gui）" >&2; exit 2 ;;
  esac
done

PASS=0
FAIL=0
SKIP=0
FAILED=()
T0=$(date +%s)

ok()   { PASS=$((PASS+1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); FAILED+=("$1"); printf '  \033[31m✗\033[0m %s\n' "$1"; }
skip() { SKIP=$((SKIP+1)); printf '  \033[33m–\033[0m %s（跳过：%s）\n' "$1" "$2"; }
head_() { printf '\n\033[1m═══ %s ═══\033[0m\n' "$1"; }

# 跑一条命令，成功记 ✓、失败记 ✗（输出只在失败时打出来，避免刷屏）
#
# 注意：变量名一律用英文 —— **bash 的 local 不认中文标识符**
# （本项目踩过两次，release.sh 里也记着这条教训）
run() { # run <说明> <命令...>
  local label="$1"; shift
  local log
  log=$(mktemp "${TMPDIR:-/tmp}/yshjy_all.XXXXXX")
  if "$@" >"$log" 2>&1; then
    ok "$label"
  else
    bad "$label"
    echo "    ── 失败详情（最后 15 行，完整日志：$log）──"
    tail -15 "$log" | sed 's/^/    /'
  fi
  return 0
}

# ───────────────────────── 0. 环境 ─────────────────────────
head_ "0. 环境"
if [ -x target/release/yshjy ]; then
  ok "找到 target/release/yshjy"
  BIN="$(pwd)/target/release/yshjy"
else
  bad "没有 target/release/yshjy（先 cargo build --release）"
  BIN=""
fi
if [ "$GUI" = 1 ]; then
  FLUTTER="${FLUTTER_BIN:-}"
  if [ -z "$FLUTTER" ]; then
    if command -v flutter >/dev/null 2>&1; then FLUTTER="flutter"
    elif [ -x "$HOME/flutter/bin/flutter" ]; then FLUTTER="$HOME/flutter/bin/flutter"
    fi
  fi
  if [ -n "$FLUTTER" ]; then ok "找到 flutter（$FLUTTER）"; else skip "界面测试" "没装 flutter"; GUI=0; fi
fi

# ───────────────────────── 1. 编译与静态检查 ─────────────────────────
head_ "1. 编译与静态检查"
run "cargo build --release" cargo build --release --quiet
# debug 也建一份：rar_atomic / pack_progress / resume 三个探针必须用 debug（release 太快，
# 它们的「耗时÷3 定中断点」「进度事件条数」假设会失效 → 假红，2026-10-29 实测）。
# 增量编译约 8 秒，无改动时近 0 秒。
run "cargo build（debug，探针用）" cargo build --quiet
run "cargo clippy（-D warnings）" bash -c 'cargo clippy --quiet -- -D warnings'
# 右键菜单脚本（N7）：语法 + 「能不能自己找到引擎」的干跑
run "右键脚本语法检查" bash -c 'bash -n gui/linux/nautilus-script-yshjy && bash -n scripts/install-nautilus.sh && bash -n scripts/build-deb.sh'
# 格式检查只作提示：本项目是手写紧凑风格 + 大量中文标识符，
# rustfmt 会把整份代码重排（上千行 diff），不适合当「有没有砸坏东西」的判据。
if cargo fmt --check >/dev/null 2>&1; then
  ok "cargo fmt --check"
else
  skip "cargo fmt --check" "手写风格没跑 rustfmt（仅提示，不算失败）"
fi

# ───────────────────────── 2. 单元测试 ─────────────────────────
head_ "2. 单元测试（cargo test）"
run "cargo test --release" cargo test --release --quiet

# ───────────────────────── 3. 协议探针 ─────────────────────────
head_ "3. 协议探针（引擎 ↔ 界面之间的契约）"
if [ -n "$BIN" ]; then
  for t in protocol_select protocol_level protocol_serve_eof protocol_pack_cover protocol_cover_only protocol_rescue protocol_sfx protocol_sfx_linux protocol_cancel_job; do
    if [ -f "tests/$t.py" ]; then
      run "$t" env YSHJY_BIN="$BIN" python3 "tests/$t.py"
    else
      skip "$t" "脚本不存在"
    fi
  done
  run "冒烟测试（smoke.sh）" bash tests/smoke.sh "$BIN"
  run "错误文案探针（errmsgs.sh）" bash tests/errmsgs.sh "$BIN"
  run "重名策略探针（conflict.sh）" bash tests/conflict.sh "$BIN"
  run "RAR 缺卷判定（rar_volume.sh）" bash tests/rar_volume.sh "$BIN"
  run "zip 符号链接（zip_symlink.sh）" bash tests/zip_symlink.sh "$BIN"
  run "默认打开方式（mime_default.sh）" bash tests/mime_default.sh gui/linux/yshjy.desktop
  # 断点续传：默认跑小包（约 10 秒）；--slow 时用大包，中断点更稳
  if [ "$SLOW" = 1 ]; then
    run "断点续传（resume.sh 120MB）" bash tests/resume.sh 120
  else
    run "断点续传（resume.sh 30MB）" bash tests/resume.sh 30
  fi
  # 打包进度（伪装 / 自解压 / 普通都要报）：150 MB 约 10 秒
  run "打包进度（pack_progress.sh）" bash tests/pack_progress.sh 150
  # RAR 原子落盘：中断后只能剩 .yshjy-part。要 rar 造样本（仅测试用，不进发布物）
  if command -v rar >/dev/null 2>&1; then
    run "RAR 原子落盘（rar_atomic.sh 80MB）" bash tests/rar_atomic.sh 80
  else
    skip "RAR 原子落盘" "没装 rar（sudo apt install -y rar 后才会跑）"
  fi
  run "协议契约（contract/run.py）" env YSHJY_BIN="$BIN" python3 tests/contract/run.py
  # CLI 选项表 ↔ 三份补全 ↔ --help 三方对账
  # （2026-10-28 新增：手抄副本实测已漂 —— --import-passwords 漏登记、补全缺 17 个）
  run "CLI 选项对账（cli_spec_match.sh）" bash tests/cli_spec_match.sh
  # --pick 全屏勾选：非 TTY 拒绝 + pty 里勾选结果与 --select 逐字节一致
  run "全屏勾选（pick.sh）" bash tests/pick.sh "$BIN"
  # 进度条：单任务等价 / -j 2 两行独立 / TERM=dumb 不沉默 / 管道零污染
  run "多任务进度（progress_multi.sh）" bash tests/progress_multi.sh "$BIN"
else
  skip "协议探针" "没有可执行文件"
fi

# ───────────────────────── 4. 界面 ─────────────────────────
head_ "4. 界面（flutter test）"
if [ "$GUI" = 1 ]; then
  # app_settings_test 要读写 configDir（`[general] open_after`），必须给它隔离的
  # XDG_CONFIG_HOME —— 否则测试自带的安全闸会拒绝跑（绝不动用户真实 config.toml）
  CFGTMP="${TMPDIR:-/tmp}/yshjy-all-cfg"
  mkdir -p "$CFGTMP"
  run "flutter test" bash -c "cd gui && XDG_CONFIG_HOME='$CFGTMP' '$FLUTTER' test"
else
  skip "界面测试" "用 --no-gui 关掉了"
fi

# ───────────────────────── 5. 慢测试（--slow 才跑）─────────────────────────
head_ "5. 慢测试"
if [ "$SLOW" = 1 ]; then
  run "300 MiB 大 RAR（X2 回归）" cargo test --release --quiet -- --ignored 大rar
  run "RAR 分卷识别（X1 回归）" cargo test --release --quiet 认出_rar_四卷分卷
  # 写盘路径 fuzz：正式跑是 10 分钟（71,661 次），日常回归只跑 60 秒探个底
  run "fuzz 写盘路径（60 秒）" bash -c "cd fuzz && cargo +nightly fuzz run extract_target -- -max_total_time=60 -rss_limit_mb=2048 >/dev/null 2>&1"
else
  skip "300 MiB 大 RAR" "加 --slow 才跑"
  skip "RAR 分卷识别" "加 --slow 才跑"
  skip "fuzz 写盘路径" "加 --slow 才跑（正式 10 分钟）"
fi

# ───────────────────────── 汇总 ─────────────────────────
T1=$(date +%s)
printf '\n\033[1m═══════ 结果 ═══════\033[0m\n'
printf '  通过 %d · 失败 %d · 跳过 %d · 用时 %d 秒\n' "$PASS" "$FAIL" "$SKIP" "$((T1-T0))"
if [ "$FAIL" -gt 0 ]; then
  printf '\n  挂在：\n'
  for c in "${FAILED[@]}"; do printf '    · %s\n' "$c"; done
  printf '\n  提示：失败项的「最后 15 行」已经在上面打出来了。\n'
  exit 1
fi
printf '\n  \033[32m全绿\033[0m —— 这一版没砸坏东西。\n'
exit 0
