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
# Windows 产物带 .exe。⚠️ MSYS 的 [ -x/-e/-f ] 对无后缀名会自动兜底同名 .exe
# （W7 实测：yshjy 不存在、yshjy.exe 存在时 [ -x yshjy ] 也是真）——
# 所以必须 **先查 .exe**：Linux 上它不存在，自然落到无后缀分支
if [ -x target/release/yshjy.exe ]; then
  ok "找到 target/release/yshjy.exe（Windows）"
  BIN="$(pwd)/target/release/yshjy.exe"
elif [ -x target/release/yshjy ]; then
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
# `pub` 会压住 dead_code 检查，所以 clippy 全绿也挡不住「没人用的公开 API」累积。
# 判据：整个仓库（引擎+界面+测试+示例）里只出现定义这一处 = 疑似死代码。
# 明示保留的（`#[allow(dead_code)]` / `#[deprecated]` 兼容名）脚本会跳过。
run "开源许可清单与 Cargo.lock 一致（gen-third-party.py --check）" python3 scripts/gen-third-party.py --check
run "死代码探测（dead_pub.sh）" bash -c 'n=$(bash scripts/dead_pub.sh --count); if [ "$n" -ne 0 ]; then echo "发现 $n 个没人用的 pub："; bash scripts/dead_pub.sh; exit 1; fi'
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
  for t in protocol_select protocol_level protocol_serve_eof protocol_pack_cover protocol_cover_only protocol_rescue protocol_sfx protocol_sfx_linux protocol_cancel_job seek_probe; do
    if [ -f "tests/$t.py" ]; then
      run "$t" env YSHJY_BIN="$BIN" python3 "tests/$t.py"
    else
      skip "$t" "脚本不存在"
    fi
  done
  run "冒烟测试（smoke.sh）" bash tests/smoke.sh "$BIN"
  run "错误文案探针（errmsgs.sh）" bash tests/errmsgs.sh "$BIN"
  run "救援模式承诺（rescue_verify.sh）" bash tests/rescue_verify.sh "$BIN"
  # 分卷 ZIP（.zip + .z01）：以前 --list 只重建出 1 项、--test 说「不是压缩包」（审计 A-5）
  run "分卷 ZIP（zip_split.sh）" bash tests/zip_split.sh "$BIN"
  run "重名策略探针（conflict.sh）" bash tests/conflict.sh "$BIN"
  # 原包处置：--trash-source 进回收站 / --delete-source 二次确认（唯一不可逆的一类）
  run "原包处置（trash_source.sh）" bash tests/trash_source.sh "$BIN"
  # 自动找密码的两档各自只关自己（T8：界面 S4 的两个勾选靠它）
  run "自动找密码两档（auto_pw_switch.sh）" bash tests/auto_pw_switch.sh "$BIN"
  # 条目声明大小封顶：多写不落盘、少写报截断（libarchive entry_bytes_declared 语义）
  run "packSize 校验（pack_size_cap.sh）" bash tests/pack_size_cap.sh "$BIN"
  # 文件管理器右键脚本：可写→原地解；只读→改投「下载」且通知说明（不许静默失败）
  # 右键脚本是 Linux 专属（Nautilus），且「只读目录回退」在 NTFS 上复现不了（不认 chmod）
  if uname -s | grep -q '^MINGW'; then
    skip "右键脚本（nautilus_script.sh）" "Linux 专属（Nautilus；NTFS 不认 chmod 只读）"
  else
    run "右键脚本（nautilus_script.sh）" bash tests/nautilus_script.sh "$BIN"
  fi
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
  # 补全脚本**运行时**行为：带空格文件名不许被 IFS 劈成两个候选（2026-10-29 真 bug）
  run "补全运行时（completions.sh）" bash tests/completions.sh
  # C ABI 头文件 ↔ 实现 对账（手写第二份真源，同 man 页 / 补全同一种病）
  run "FFI 头文件对账（ffi-header-sync.sh）" bash tests/ffi-header-sync.sh
  # --test 只测不写：完整解码 + 校验，但一个字节都不落盘（7-Zip 的 t / Ark 的 SupportsTesting）
  run "只测不写（verify_only.sh）" bash tests/verify_only.sh "$BIN"
  run "干扰数据/假签名（decoy.sh）" bash tests/decoy.sh "$BIN"
  # caps 按格式声明：协议 hello.formats 与 CLI --caps 必须同源（手抄第二份真源的老病）
  run "能力表同源（caps_protocol.py）" env YSHJY_BIN="$BIN" python3 tests/caps_protocol.py "$BIN"
  run "空间不足 detail（t2_space_detail.py）" python3 tests/t2_space_detail.py "$BIN" "${YSHJY_SPACE_PROBE_PKG:-/dev/null}"
  # S10 多候选：引擎把「一个文件里几处藏着归档」都报出来（t5），
  # 命令行能指定解第几处（candidate_cli，T16 —— 界面走协议，命令行走 --candidate）
  run "多候选扫描（t5_candidates.py）" env YSHJY_BIN="$BIN" python3 tests/t5_candidates.py
  run "命令行指定候选（candidate_cli.py）" python3 tests/candidate_cli.py "$BIN"
  run "协议 smart_dir 开关（smart_dir_proto.py）" python3 tests/smart_dir_proto.py "$BIN"
  # E2 尾部模式 / E6 诊断包落点：这两项以前只有 widget 测试（验界面传了什么），
  # 引擎那侧真怎么落盘没人守 —— 改坏了 flutter test 照样绿。
  run "协议 pack.tail（tail_proto.py）" python3 tests/tail_proto.py "$BIN"
  run "协议 pack.format（format_proto.py）" python3 tests/format_proto.py "$BIN"
  run "协议 pack.tweak_time（tweak_time_proto.py）" python3 tests/tweak_time_proto.py "$BIN"
  # T18-2 自定义显示名：--name 只改名字不改位置；顺带盯「-o 目录」对打包是否管用
  run "自定义显示名（name_cli.py）" python3 tests/name_cli.py "$BIN"
  # T18-1 布局 L4（插在视频中间）：核心断言是「ffmpeg rc=0 且 stderr 空」——
  # 只看 rc 会让「改了 mdat size 没改 stco」那种满是坏帧的包也过验收
  run "布局 L4 插在视频中间（l4_proto.py）" python3 tests/l4_proto.py "$BIN"
  # T18-1 布局 L3（放在文件头部）：插点在 ftyp 末尾、mdat.size 不动、全表偏移 +增量。
  # ★ 头长少算 8 字节同样表现为「rc=0 但 stderr 满屏 AAC 错」—— 也是靠 stderr 空守住
  run "布局 L3 放在文件头部（l3_proto.py）" python3 tests/l3_proto.py "$BIN"
  # T18-1 布局 L7（一个文件两个包）：★ 核心是两条「反向断言」——
  # 7z 只能看到第一个、unzip 只能看到第二个（这是设计代价，不是 bug）；能解两处的只有我们自己
  run "布局 L7 两个包（l7_proto.py）" python3 tests/l7_proto.py "$BIN"
  # T18-1 布局 L8（MKV 附件轨）：要系统 ffmpeg；没装就自动降级（探针第 ⑭ 条专门验这个）。
  # ★ 判据不能只看返回码：封装完还要解码一遍**且 stderr 为空**（L3 那次假绿的教训）
  if command -v ffmpeg >/dev/null 2>&1 && command -v ffprobe >/dev/null 2>&1; then
    run "布局 L8 MKV 附件（l8_proto.py）" python3 tests/l8_proto.py "$BIN"
  else
    skip "布局 L8 MKV 附件（l8_proto.py）" "本机没有 ffmpeg/ffprobe"
  fi
  run "协议 report.out_dir（report_outdir_proto.py）" python3 tests/report_outdir_proto.py "$BIN"
  # T18-4 队列「全部暂停 / 继续」：★ 只认 PAUSED 事件 + 文件数不再增长，
  # 不认 pause 的回执（回执只代表请求送到）；产物逐文件 sha256 与不暂停那次对齐
  run "队列暂停继续（t18_4_pause.py）" python3 tests/t18_4_pause.py "$BIN"
  # 「完全离线」不是口头承诺（T18-6）：依赖表 + 源码非注释行里的网址都查
  run "完全离线断言（offline_probe.py）" python3 tests/offline_probe.py
  # 7z 头加密（-mhe=on）：连清单都读不到时必须单独说，另含 --list 密码丢失的回归
  if command -v 7z >/dev/null 2>&1; then
    run "7z 头加密（head_encrypted.sh）" bash tests/head_encrypted.sh "$BIN"
  else
    skip "7z 头加密" "没装 7z（sudo apt install -y p7zip-full 后才会跑）"
  fi
  # --pick 全屏勾选：非 TTY 拒绝 + pty 里勾选结果与 --select 逐字节一致
  # pty 是 Unix 概念（Windows 的 python 没有 termios/pty），没有就明说跳过，不许假红
  if python3 -c "import pty" 2>/dev/null; then
    run "全屏勾选（pick.sh）" bash tests/pick.sh "$BIN"
  else
    skip "全屏勾选（pick.sh）" "本机 python 没有 pty（Windows），pty 交互只在 Unix 验"
  fi
  # 预览树（第三期 A）：界面勾目录 = 传一串序号，这条验「送出去的序号」与
  # 等价通配表达式产物逐字节一致（树的构建/三态由 gui/test/entry_tree_test.dart 覆盖）
  run "预览树选择一致性（tree_select.sh）" bash tests/tree_select.sh "$BIN"
  # 进度条：单任务等价 / -j 2 两行独立 / TERM=dumb 不沉默 / 管道零污染（同样吃 pty）
  if python3 -c "import pty" 2>/dev/null; then
    run "多任务进度（progress_multi.sh）" bash tests/progress_multi.sh "$BIN"
  else
    skip "多任务进度（progress_multi.sh）" "本机 python 没有 pty（Windows），pty 交互只在 Unix 验"
  fi
  # 移动设备落盘检验（2026-10-30 用户拍板的方案）：
  #   本机盘不管开关都不等 / 移动设备+关→只提醒不等 / 移动设备+开→等完才报成功
  #   没有可写移动设备时探针自己明说跳过（退出码 0），不算通过也不算失败
  run "移动设备落盘检验（flush_movable.sh）" bash tests/flush_movable.sh "$BIN"
else
  skip "协议探针" "没有可执行文件"
fi

# ───────────────────────── 4. 界面 ─────────────────────────
head_ "4. 界面（flutter test）"
if [ "$GUI" = 1 ]; then
  # app_settings_test 要读写 configDir（`[general] open_after`），必须给它隔离的
  # XDG_CONFIG_HOME —— 否则测试自带的安全闸会拒绝跑（绝不动用户真实 config.toml）
  # 历史页测试（T18-5 排序）要**造**几条记录，落点是 stateDir/gui-history.json，
  # 同理必须隔离 XDG_STATE_HOME（否则直接改用户真实历史）
  CFGTMP="${TMPDIR:-/tmp}/yshjy-all-cfg"
  STMTMP="${TMPDIR:-/tmp}/yshjy-all-state"
  mkdir -p "$CFGTMP" "$STMTMP"
  run "flutter test" bash -c "cd gui && XDG_CONFIG_HOME='$CFGTMP' XDG_STATE_HOME='$STMTMP' '$FLUTTER' test"
  # 窗口尺寸记忆（P10）：这条必须真的开界面窗口 —— Wayland 不许外部查询
  # 别人窗口的几何、GNOME 48 的截图接口对非 shell 进程也不给用（都实测过），
  # 只靠界面里 YSHJY_WINDOW_TRACE 的打点断言。默认跳过：用户明确讨厌回归
  # 时往桌面弹窗（被文件管理器弹窗抓到过两次）。
  if [ "${YSHJY_GUI_VISIBLE_TEST:-}" = 1 ]; then
    run "窗口尺寸记忆（gui_window_state.sh）" bash tests/gui_window_state.sh
  else
    skip "窗口尺寸记忆" "会开界面窗口，设 YSHJY_GUI_VISIBLE_TEST=1 才跑"
  fi
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
