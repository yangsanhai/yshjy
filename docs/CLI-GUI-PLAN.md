# 施工清单：终端收口 + 界面入口 + 出包（2026-10-28）

> **本文只写「怎么下手」，不记状态。** 做完没做完看 `docs/TODO.md`（唯一真源），
> 证据写 `CHANGELOG.md`。两边永远不重复记同一件事。

---

## 0. 基线事实（2026-10-28 实测，动手前先认这个，别凭记忆）

| 事实 | 实测值 | 出处 |
|---|---|---|
| 命令行解析 | **手写**，没有 clap | `src/main.rs` 2429 行 |
| 参数表 | `带值的选项` 13 个 / `无值选项` 19 个 | `src/main.rs:129-135`、`:141-149` |
| `--help` 长选项 | **35 个** | `target/release/yshjy --help \| grep -oE '\-\-[a-z-]+'` |
| 手写补全登记 | **18 个**（缺 17 个） | `scripts/completions/{yshjy.bash,_yshjy,yshjy.fish}` |
| man 页 | **手写**，不是生成的 | `docs/yshjy.1` |
| 打包怎么用它们 | 直接搬文件进包 | `scripts/build-deb.sh:134,138-140` |
| 现有护栏（弱） | `--help` 只要能跑；包里**只要有**这几个文件 | `tests/smoke.sh:37`、`scripts/test-packages.sh:58-61` |
| 第三方依赖 | 22 个（刻意最小）；终端底层已有 `crossterm 0.29` + `unicode-width 0.2.2` | `Cargo.toml` |
| 没有的库 | `dialoguer` / `comfy-table` / `clap_complete` / `clap_mangen` | 同上，grep 计数 0 |
| 许可证白名单 | MIT / MIT-0 / Apache-2.0 / BSD-3-Clause / Unicode-3.0 / Zlib / CC0-1.0 | `deny.toml` |

**结论**：补全对不上不是「谁忘了抄」，是**同一份选项清单存在三处手抄副本**（代码常量表 / 三个补全文件 / man 页），
而**没有任何一项测试验内容对不对**。要治的是这个结构。

---

## 1. 第 1 行：补全与 man 页改成自动生成（最高优先）

### 1.1 先定真源，别先写生成器
- **甲（选这个，低风险）**：新增 `src/cli_spec.rs`，把选项清单升成**带字段的表**：
  长名 / 短名 / 是否带值 / 值名（如 `<目录>`）/ 说明的 i18n 键。
  `main.rs` 那两份常量表改成从这张表推导 —— **第一步只搬家，行为零变化**。
- **乙（不做）**：引 clap 全量重写解析层。35 个选项 + 互斥组合（`--list` / `--rescue` / `--sfx` / `--serve` / `--pick`）
  全部要重新验，风险远大于收益。真要做，单独立项。

### 1.2 六小步，每步单独提交
1. **建表搬家**：`src/cli_spec.rs`；`main.rs` 两份常量改成推导。加单测钉住「表里长选项集合 == 35 个」。
2. **出补全**：生成器（`src/bin/gen_cli_assets.rs` 或 `xtask`）从表生成 bash / zsh / fish 三份。
3. **出 man**：生成 groff 格式 man 页；先与现有 `docs/yshjy.1` **逐节 diff**，把措辞/章节顺序对齐再替换。
4. **改打包**：`scripts/build-deb.sh:134,138-140` 改成装生成产物；**手写三件套退休**（删掉，别留孤本同时存在）。
5. **加对账探针**：`tests/completions_match.sh` —— `--help` 的长选项集合 == 补全文件里的集合 == man 页里的集合；
   挂进 `tests/all.sh`。这条是防回退的关键。
6. **跑全绿再提交**：`bash tests/all.sh`（+ 出包时 `scripts/test-packages.sh` 33 项）。

### 1.3 红线
- **不许改 help 文案**（7 个语言包的 i18n 键一个字不动），只换「谁来输出」。
- 生成结果里只出现 ASCII 长名；中文只出现在说明文字里。
- 若要引 `clap_complete` / `clap_mangen`（MIT/Apache-2.0，在 `deny.toml` 白名单内）：必须跑
  `cargo deny check` 四项全绿才算过。
- man 页版本号那行要从 `CARGO_PKG_VERSION` 取，**不许再手写**（历史上落后过三个版本）。

---

## 2. 第 2 行：`--pick` 全屏勾选

- 库：`dialoguer`（MIT）。前置：`cargo deny` 四项；它与已有 `crossterm 0.29` 的版本要对齐，**实测不冲突再往下走**。
- 落点：新增 `src/pick.rs`；`--pick` 走它；`--select` 语义一个字不动。
- 非 TTY 降级：`io::IsTerminal` 为假时报「当前不是交互终端，请改用 --select」并以 USAGE(2) 退出。
- 验收（照 TODO 四条）：选中结果与 `--select "2,5,9"` 逐字节一致 / 管道降级有提示 / 中文文件名不错行 / 退出后 `stty -a` 与进入前一致。
- 红线：raw mode 退出复原**交给库**，绝不手写循环。

---

## 3. 第 3 行：`--list` 改表格

- 库：`comfy-table`（MIT）+ 已装的 `unicode-width`。
- 落点：`--list` 的文本输出层。**`--json` 输出一个字都不许变**（界面靠它吃饭）。
- 验收：40 / 80 / 120 列三档不换行不乱码；中文按显示宽度对齐；**非 TTY 自动退回原来的一行一条**。
- 红线：`--list` 的结果集合、顺序、退出码不变，只改排版。

---

## 4. 第 4 行：进度条换 `indicatif`（排后面）

- 只有 `-j 2` 同时解多个包才真正需要（现手搓版两行会互相覆盖）。
- 现有手搓实现在 `src/main.rs` 的 `画进度行`（基于 `crossterm`）。
- 红线：ETA 仍用引擎 `EtaEstimator` 算好塞进 `{msg}` —— **不许让命令行和界面显示两个不同的剩余时间**。
- 验收：两个任务各自推进不覆盖 / 单任务输出与现在等价 / 非 TTY 一行都不画。

---

## 5. 界面两条入口

### 5.1 只取封面（成本最低）
- 引擎早就有：协议 `extract` 支持 `cover` 开关（`src/api.rs:218` 附近就是 `extract` 参数解析）。
- 只差界面：`gui/lib/pages/home_page.dart` 加一个入口。
- 验收：拖入伪装包 → 点「只取封面」→ 落地一个能正常播放的原版视频，**不解压内层**。

### 5.2 救援模式
- 协议要加：命令表 `src/api.rs:28` 加 `rescue` → `serve`/`api` 接线 → `gui/lib/transport/` 加调用
  → `tests/contract/` 补一份 golden。
- 验收：拿索引被改坏的样本，界面点「救援」也能救出文件。
- 两条都按五态检查（空 / 加载 / 成功 / 失败 / 部分），失败态不许甩裸错误码。

---

## 6. 出包（不上 GitHub 也能出的）

| 目标 | 本机能不能出 | 做法 | 验证 |
|---|---|---|---|
| Linux deb + tar.gz | ✅ 现成 | `scripts/build-deb.sh` | `scripts/test-packages.sh` 33 项 |
| Windows **命令行** exe | ✅ 能 | `cargo build --release --target x86_64-pc-windows-gnu`（目标 + mingw 已装）→ 打 zip | wine 里实测解压 + 打包各一次 |
| 安卓 APK | ✅ 能，但要先装 SDK/NDK（8–10 G，**需用户点头**） | `cargo-ndk` 出 arm64-v8a / armeabi-v7a 的 `libyshjy_ffi.so`（`ffi/Cargo.toml:10` 已 cdylib+staticlib）→ `ffigen` 生成 Dart 绑定 → `flutter build apk` | 真机装了能开、能解压 |
| Windows **图形界面**版 | ❌ 不能 | Flutter 的 Windows 桌面只能在 Windows 上编 | 借机器 |
| macOS / iOS | ❌ 不能 | 必须在 macOS 上编 | 借机器或以后上 CI |

**安卓必须先定的一条政策**（照 localsend 的做法）：App 私有目录直写；外部目录一律走系统授权（SAF），
**不许静默失败**。这条不定，代码写出来就是白写。

---

## 7. 每步的统一收工动作

1. `bash tests/all.sh` 全绿（发版前加 `--slow`）
2. 证据写 `CHANGELOG.md`
3. `docs/TODO.md` 划掉一行
4. 本地 git 提交，**一次一行，不混提**

---

## 8. 明确不做（重复第三遍，免得又捡起来）

AppImage / Windows 图形界面本机自编 / macOS 公证 / iOS 上架 / 边解压边删源（`--reclaim`）/
7z 写包 / 自动更新 / 崩溃上报。
