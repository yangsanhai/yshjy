# 阶段 0 体检证据（T1–T9）

> 本文件只放**跑出来的证据和结论**；待办仍然只看 `docs/TODO.md` 一处。

## T1 依赖安全体检

体检日期：2026-10-24
工具：`cargo-audit 0.22.2`、`cargo-deny 0.20.2`（均为 GitHub 预编译二进制，装在 `~/.cargo/bin`）
漏洞库：RustSec advisory-db 快照，1332 条（`~/.cargo/advisory-db`，浅克隆）
依赖规模：`Cargo.lock` 共 126 个 crate

## 结论一句话

**扫出 1 个真实漏洞 + 1 个被撤回版本 + 1 处许可证白名单缺口**，全部是传递依赖的补丁级升级，
我们自己的 `Cargo.toml` 一行都不用改。修完 `cargo deny check` 才能全绿。

## 发现 1：ctutils 0.4.3 —— RUSTSEC-2026-0385（有漏洞，必须修）

| 项 | 内容 |
|---|---|
| 漏洞 | `ctutils::Choice` 的掩码生成函数在 v0.4.4 之前可能生成**带分支**的代码；`u128` 比较在 32 位目标上也会分支。分支依赖秘密值 → 可能通过侧信道泄漏密钥 |
| 严重度 | RustSec 归为密码学实现缺陷（非常新，2026-10-23 发布） |
| 修复 | `ctutils >= 0.4.4`（当前最新 0.4.5） |
| 怎么进来的 | `zip 9.0.4` / `rars 0.11.0` / `sevenz-rust2 0.24.0` → `digest 0.11.3` / `sha1` / `sha2` / `hmac` / `pbkdf2` → `ctutils 0.4.3` |
| 用户会怎样撞上 | 本地解压工具，攻击者通常不在对面，**现实风险低**；但走的是「密码校验、哈希比对」这条加密路径，属于该修就修的一类，而且 `deny.toml` 里 vulnerability 默认 deny，门禁会一直红 |

修法：`cargo update -p ctutils --precise 0.4.5`（只动 `Cargo.lock`）

## 发现 2：inout 0.2.3 —— 被撤回（yanked）

| 项 | 内容 |
|---|---|
| 情况 | 该版本已被 crates.io 撤回（yanked），最新 0.2.5 |
| 怎么进来的 | `aes 0.9.3` / `cbc 0.2.1` → `cipher 0.5.2` → `inout` |
| 影响 | yanked ≠ 有漏洞，但意味着上游不建议再用该版本；`cargo-deny` 门禁会报 |

修法：`cargo update -p inout --precise 0.2.5`

## 发现 3：许可证白名单缺口（libbz2-rs-sys）

`libbz2-rs-sys 0.2.5`（`bzip2 0.6.1` → `sevenz-rust2`）的许可证是 `bzip2-1.0.6`，
不在 `deny.toml` 的 allow 列表里 → `licenses FAILED`。

- `bzip2-1.0.6` 是宽松自由许可证，与 MIT 兼容，可放行
- 顺带两条白名单从来没被用到（`BSD-2-Clause`、`ISC`），留着会一直报警告

修法：allow 列表加 `"bzip2-1.0.6"`，删掉没用到的两条（或留注释说明未来可能用到）

## 顺带发现：deny.toml 与新工具不兼容（**已改**）

cargo-deny 0.20 的配置 schema 变了，老配置直接跑不起来：

| 旧写法 | 新版 | 处理 |
|---|---|---|
| `vulnerability = "deny"` | 键已移除（默认就是 deny） | 删掉 + 注释说明 |
| `yanked = "deny"` | 键已移除（默认 deny） | 删掉 + 注释说明 |
| `unmaintained = "warn"` | 只接受 `all` / `workspace` / `transitive` / `none` | 改 `workspace`（只报工作区直接依赖，避免传递依赖噪音） |

## 验收标准（修完要看到）

1. `cargo audit --no-fetch` → `0 vulnerabilities found`，无 yanked
2. `cargo deny check` → advisories ok / bans ok / licenses ok / sources ok
3. `bash tests/all.sh` 仍 15 通过 / 0 失败（证明升级没改坏行为）
4. 补丁升级只动 `Cargo.lock`，`Cargo.toml` 不变

---

## T7 多语言完整度体检

体检日期：2026-10-24
方法：拿 `locales/zh-CN.toml` 当基准，用 Python `tomllib` 递归展平成 `a.b.c` 形式的 key 逐语言比对；
再对 `gui/lib/` 全部 14 个 Dart 文件扫硬编码中文。

### 引擎 / CLI 侧：**完整，无缺口**

7 个语言包全部 **121 个 key，零缺失、零空值、零多余 key**：

    语言            key数  缺key  空值  多余key
    en-US.toml      121     0     0      0
    es-ES.toml      121     0     0      0
    ja-JP.toml      121     0     0      0
    ko-KR.toml      121     0     0      0
    ru-RU.toml      121     0     0      0
    zh-TW.toml      121     0     0      0

「各语言里与中文一字不差」的那几条，逐条看过，**全是纯格式串**（`'   {lock} {size}   {path}'`、
`'   ⚠ {warn}'` 等），没有文字可翻，不算漏译。仅 `ja-JP` 有两条真文字与中文相同：
`tier.common = '常用版'`、`tier.full = '全能版'`（日文汉字写法与中文一致，可接受）。

加载链路也核实过：`build.rs` 扫描 `locales/*.toml` 编译期嵌入 + 支持外部覆盖
（`$YSHTY_LOCALES/` → `~/.config/yshjy/locales/` → 当前目录 `locales/`），`src/i18n.rs` 有兜底断言。

### 界面（Flutter GUI）侧：**不是缺翻译，是根本没接**

| 证据 | 数字 |
|---|---|
| `gui/lib/` 里引用 i18n / AppLocalizations / Intl.message 的文件 | **0 个** |
| 硬编码中文的 Dart 文件 | **11 / 14 个** |
| 硬编码中文字符串出现次数 | **401 处** |

原因不是偷懒，是架构决定：D21/N1 定的是「**引擎只给码、壳自己查文案**」，所以错误码/诊断码
的人话全在 `gui/lib/humanize.dart`（87 行）+ 各页面里；引擎的 7 语言语言包只服务 CLI 与引擎自身消息。

项目自己也如实写着（`settings_page.dart:341`）：
「界面文案多语言切换（引擎已支持 7 种语言，界面文案还没抽成语言包）」。

**要做界面多语言，工作量不是补翻译，而是**：把 `humanize.dart` 三张表 + 401 处中文抽成
Dart 侧语言包（ARB 或自建 `Map<String, String>` 按语言加载），涉及 11 个文件。

### 发现一处会误导人的文案（小，但真人会撞）

`settings_page.dart:327` 把「界面语言」列在「其它（改 config.toml）」区块里，并写出 7 种语言码：

    _kv('界面语言', 'zh-CN / zh-TW / en-US / ja-JP / ko-KR / ru-RU / es-ES')

而 `config.toml` 的 `general.language` 只影响**引擎/CLI 文案**，改它界面一个字都不会变
（界面 0 处读语言包）。用户照这行去改配置，会以为界面能切语言，结果没反应 —— 属于「文案比实现承诺得多」。
建议改成明说「界面语言：中文（界面多语言尚未实现，见下方待办）」，或在设置页做一个真能切的开关。
