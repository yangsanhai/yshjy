# clap_complete / clap_mangen 拆解（补拆 §10.2 第 1 项）

> 来源：`refs/upstream/clap/`（clap 4.6.7 / clap_complete 4.6.11 / clap_mangen 0.3.3，MIT OR Apache-2.0）
> 拆解日期：2026-10-28。读完范围：`clap_complete/src/lib.rs` 全部、`aot/shells/{bash,zsh,fish}.rs` 全部关键段、
> `engine/mod.rs` 全部、`clap_mangen/src/{lib.rs,render.rs}` 关键段。
> 结论先行：**不换 clap**（我们 CLI 是手搓解析，迁 clap 要动 37 个选项的行为，收益不抵风险），
> 但**抄 4 条具体做法**，其中第 1 条是我们**真 bug**（下方 §6.1 有实测铁证）。

---

## 1. clap_complete 的三条路线（先搞清这个，别抄错层）

`clap_complete/src/lib.rs:52-59` 把功能切成三个并列模块：

| 路线 | 模块 | 机制 | 适用 |
|---|---|---|---|
| **AOT 静态生成** | `aot/`（含 `aot/shells/{bash,zsh,fish,elvish,powershell}.rs`） | 把 clap 的 `Command` 树翻译成一份**完整的 shell 脚本**，装到用户机上 | 几乎所有 CLI 项目；ouch 也走这条 |
| 动态补全引擎 | `engine/`（`#[cfg(feature = "unstable-dynamic")]`） | 程序**自己**实现补全逻辑，shell 只调 `yshjy --complete ...` 拿候选 | 候选要实时算（比如从数据库查） |
| 环境变量协议 | `env/`（`CompleteEnv`） | `COMPLETE=bash yshjy ...` 一个变量触发，**连补全脚本都不用装** | 同上，且要求 shell 配置里加一行 `complete -C` |

**我们走 AOT 这一层**（`scripts/gen-cli-assets.py` 就是我们的 AOT 生成器，只是数据源不是 clap 而是
`src/cli_spec.rs`）。所以下面对照只跟 `aot/` 比；`engine/`、`env/` 记在这里当知识，**不抄**（见 §7）。

> 记一笔：`lib.rs:68-97` 里 `generator` / `shells` 两个老模块名已标 Deprecated，只做 re-export。
> 网上很多教程还在用 `clap_complete::generator::Bash`，那是旧 API，别照抄。

---

## 2. bash 生成器：`aot/shells/bash.rs`（309 行）

### 2.1 注册补全函数（`:74-78`）

```bash
if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _yshjy -o nosort -o bashdefault -o default yshjy
else
    complete -F _yshjy -o bashdefault -o default yshjy
fi
```

三个 `-o` 各有来历，**我们现在一个都没有**：

| 选项 | 作用 | 为什么 clap 要加 |
|---|---|---|
| `nosort` | 不按字母序重排候选 | bash ≥4.4 默认会排序，把「短选项在前、长选项在后」和 `-h/--help` 压尾的**意图顺序**打乱 |
| `bashdefault` | COMPREPLY 为空时退回 bash 内置补全（变量/用户名/wildcard） | 只在我们自己没给候选时生效，不干扰 |
| `default` | 同上但退回 readline 默认（文件名） | 兜底：任何漏网的场景至少还能补文件名 |

### 2.2 `cur` / `prev` 用 `$2` `$3` 而不是 `COMP_WORDS`（`:36-41`）

```bash
if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
    cur="$2"
else
    cur="${COMP_WORDS[COMP_CWORD]}"
fi
prev="$3"
```

**为什么**：`complete -F` 的补全函数被调用时会传三个位置参数（`$1`=命令名、`$2`=当前词、
`$3`=前一个词），这份是 bash 已经**解析过引号**的结果；而 `COMP_WORDS` 是**未解析**的词数组，
遇到 `--opt="a b"` 这种会不一致。我们用 `COMP_WORDS` 属于老写法，日常够用但不够严谨。

### 2.3 值补全按 `ValueHint` 分派（`:178-217`）

```rust
let compopt = match o.get_value_hint() {
    ValueHint::FilePath => Some("compopt -o filenames"),
    ValueHint::DirPath  => Some("compopt -o plusdirs"),
    ValueHint::Other    => Some("compopt -o nospace"),
    _ => None,
};
```

`compopt` 是**运行时**改本次补全行为（`filenames` 让补出来的名字自动转义空格，
`plusdirs` 让目录也进候选，`nospace` 表示补完不加空格、继续等值）。
比「在 `complete` 那一行写死 `-o filenames`」精确得多 —— 我们写死的是全局 `-o filenames`。

### 2.4 ⭐ 带空格文件名用 `IFS=$'\n'` 保护（`:190-201`、`:223-234`）

```bash
local oldifs
if [ -n "${IFS+x}" ]; then
    oldifs="$IFS"
fi
IFS=$'\n'
COMPREPLY=($(compgen -f "${cur}"))
if [ -n "${oldifs+x}" ]; then
    IFS="$oldifs"
fi
```

**这一条是本次拆解最重要的产出。** `$(...)` 命令替换出来的一串结果会按 `$IFS`（默认含空格）
再分词一次 —— 文件名里带空格就会被**劈成两个候选**。clap 的做法是临时把 IFS 换成只剩换行，
拿完结果立刻还原（`${IFS+x}` 是「变量是否已设置」判断，比 `-n` 严谨：IFS 被设成空串也要还原）。

> 我们的生成器在 `gen-cli-assets.py:295`（file 分支）和 `:312`（位置参数 fallback）用的是
> `COMPREPLY=($(compgen -f -- "$cur"))`，**没有 IFS 保护** → 真 bug。实测证据在 §6.1。

### 2.5 位置参数**不**输出占位符（`:296-300`，附 issue 号）

```rust
// A positional without possible values has no literal completion word to
// offer here; emitting its `<NAME>`/`[NAME]` usage placeholder would make
// bash glob-expand the brackets (#6407), so leave it to the `-o default`
// (filename) fallback instead.
```

这条是在提醒：**别把 `<文件>` 这种占位符塞进 `compgen -W` 的候选表**，bash 会把方括号当
glob 展开。我们生成器已经在 `opts` 里塞了 `jy ys` 两个子命令词，没有占位符 → 这点我们是对的。

### 2.6 子命令用「函数名拼接」做状态机（`:91-170`）

`add_command` 递归拼出 `_yshjy__subcmd__pack` 这种函数名，用 `cmd` 变量在 `for` 循环里
扫 `COMP_WORDS` 累积当前处于哪个子命令，再 `case "$cmd"` 分派到各自的选项表。
我们**没有子命令**（只有 `jy` / `ys` 两个位置词），所以这套状态机**不需要抄**。

---

## 3. zsh 生成器：`aot/shells/zsh.rs`（704 行，最长的一个）

### 3.1 用 `_arguments` 而不是手写 `compadd`（`:327-345`）

```zsh
_arguments "${_arguments_options[@]}" : \
    '(-h --help)'{-h,--help}'[Print help]' \
    '--dest[输出目录]:目录:_files -/' \
    ...
```

三段式规范 `'<排他组><选项>[<描述>]:<值名>:<补全函数>'`，业界标准写法，我们已经在用（`_arguments -s -S`）。

### 3.2 `ValueHint` → zsh 内置补全函数的映射表（`:400-420`）

```rust
ValueHint::Unknown => "_default",
ValueHint::AnyPath | ValueHint::FilePath => "_files",
ValueHint::DirPath => "_files -/",
ValueHint::ExecutablePath => "_absolute_command_paths",
ValueHint::Username => "_users",
ValueHint::Hostname => "_hosts",
ValueHint::Url => "_urls",
ValueHint::EmailAddress => "_email_addresses",
```

**`DirPath` 是 `_files -/`（带 `-/` 只列目录）**，不是 `_directories`。
我们的生成器用的是 `_directories`（`gen-cli-assets.py:332`）—— 两者都只列目录，
但 `_files -/` 会保留 zsh 的**同目录文件过滤**行为，是官方推荐写法。属小改进，不紧急。

### 3.3 `-S` 修饰符的隐含语义（`:605-640` 的注释，值得记）

```rust
// We use the '-S' _arguments option to disable completion after '--'. Thus, the
// completion for the second argument in scenario (B) does not need to be emitted
// because it is implicitly handled by the '-S' option.
```

`_arguments -S` 除了「选项后不自动补空格」，还**关掉 `--` 之后的补全**。我们用
`_arguments -s -S`，等于也吃到了这个行为 —— 意味着 `yshjy -- --weird-file<TAB>` 不会再去补选项，
符合直觉。这条不用改，**但要知道原因**，免得以后有人想删 `-S`。

---

## 4. fish 生成器：`aot/shells/fish.rs`（324 行）

### 4.1 值补全的映射（`:308-320`）

```rust
ValueHint::Unknown => " -r",
ValueHint::AnyPath | ValueHint::FilePath | ValueHint::ExecutablePath => " -r -F",
ValueHint::DirPath => " -r -f -a \"(__fish_complete_directories)\"",
ValueHint::Username => " -r -f -a \"(__fish_complete_users)\"",
ValueHint::Hostname => " -r -f -a \"(__fish_print_hostnames)\"",
```

要点：`-r` = 该选项需要参数；`-F` = 该参数按**文件**补；`-f` = **关掉**默认的文件补全；
`-a "<候选>"` = 给候选词。**`DirPath` 是 `-r -f -a "(__fish_complete_directories)"`：
关掉默认文件补全，只喂目录**。
我们用的是 `-r -F`（`:391`，dir 和 file 都当文件补）→ 目录补全会**混入普通文件**。属小缺陷。

### 4.2 候选值里带描述要用 `\t'描述'`（`:290-303`）

```rust
Some(format!("{}\t'{}'", escape_string(value.get_name(), true), escape_help(...)))
```

fish 的 `-a` 支持 `值\t描述` 两列，描述**必须用单引号包住**，否则描述里的 `$` 会被 fish 当变量展开。
我们的 `--encoding` 候选（utf-8 gbk big5 sjis）没有描述，暂不受影响。

---

## 5. clap_mangen：`src/lib.rs`(353) + `src/render.rs`(526)

### 5.1 `.TH` 是**五段**元数据（`lib.rs:188-201`）

```rust
fn _render_title(&self, roff: &mut Roff) {
    roff.control("TH", self.title_args());
}
fn title_args(&self) -> Vec<&str> {
    vec![&self.title, &self.section, &self.date, &self.source, &self.manual]
}
// → .TH TITLE SECTION DATE SOURCE MANUAL
```

我们 man 页的 `.TH` 也是五段（文档名 / 1 / 日期 / 版本 / 手册分类），
**但版本号在 `$4`（SOURCE 位）** —— 这正是 0.9.28 那个「man 版本号永远停在旧版」事故的现场：
生成器的比对逻辑把整行 `.TH` 抹掉，版本号跟着被抹。教训已经修掉了，这里记的是
**「五段的位置语义」**：以后谁再动生成器，要知道 `$4` 是我们的版本号。

### 5.2 选项头部的括号语义（`render.rs:389-416`）

```rust
fn markers(required: bool) -> (&'static str, &'static str) {
    if required { ("<", ">") } else { ("[", "]") }
}
fn option_value_markers(arg: &Arg) -> (&'static str, &'static str) {
    match (range.min_values() > 0, arg.is_require_equals_set()) {
        (true, false)  => (" ", ""),      // --opt <VALUE>
        (false, false) => (" [", "]"),    // --opt [<VALUE>]
        (false, true)  => ("[=", "]"),    // --opt[=<VALUE>]
        (true, true)   => ("=", ""),      // --opt=<VALUE>
    }
}
```

四象限表。我们的 man 页对所有带值选项一律写 `--opt <值名>`，**没有「可选值」与「等号形式」两档**。
我们 CLI 里 `--level`、`--jobs` 都允许省略（有默认值），严格说该写 `[<值>]`。
**这是 man 页语义精度问题，不影响使用，记为低优先。**

### 5.3 选项排序键（`render.rs:513`）

```rust
fn option_sort_key(arg: &Arg) -> (usize, String) { ... }   // (display_order, long)
```

clap 允许显式 `display_order`，默认按字母序。我们按 `cli_spec.rs` 的**表顺序**输出 —— 更好，
因为表顺序是人工有意排的（常用在前），别改成字母序。

---

## 6. 对照我们 `scripts/gen-cli-assets.py`：5 处差距（含实测）

> 判定原则：**只有「用户能感觉到」或「会出错」的才算缺陷**，风格差异不算。

### 6.1 🔴 真 bug：没有 bash-completion 包时，带空格的文件名被劈成两个候选

**根因**（`gen-cli-assets.py:295`、`:312`）：

```python
'            if declare -F _filedir >/dev/null; then _filedir else COMPREPLY=($(compgen -f -- "$cur")); fi'
```

`_filedir`（bash-completion 包）内部做了 IFS 保护，所以**装了 bash-completion 的机器没事**；
没装时走 fallback，`$(compgen -f ...)` 按默认 IFS 分词 → 名字里带空格就裂。

**实测（2026-10-28，`~/.hermes/cache/scratch/comptest/`，目录里只有一个 `有 空格.zip`）**：

| 场景 | COMPREPLY 元素数 | 内容 |
|---|---|---|
| A. 无 bash-completion（我们的 fallback） | **2** ❌ | `有` / `空格.zip` |
| B. 有 bash-completion（走 `_filedir`） | 1 ✅ | `有 空格.zip` |
| C. clap 的 `IFS=$'\n'` 写法 | 1 ✅ | `有 空格.zip` |

**修法**（抄 §2.4）：把两处 fallback 包成

```bash
local oldifs
[ -n "${IFS+x}" ] && oldifs="$IFS"
IFS=$'\n'
COMPREPLY=($(compgen -f -- "$cur"))
[ -n "${oldifs+x}" ] && IFS="$oldifs"
```

注意 `compgen -W` 那几处（`--encoding` 之类固定词表）**不需要**改 —— 候选词是我们自己控制的，没有空格。

### 6.2 🟡 `complete` 那行缺三个 `-o`

现在（`gen-cli-assets.py:316`）：`complete -o filenames -F _yshjy_complete yshjy yshys`
clap 的做法：`-o nosort -o bashdefault -o default`（bash ≥4.4 才有 `nosort`）。
建议改成（含版本判断）：

```bash
if [[ "${BASH_VERSINFO[0]}" -gt 4 || ( "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 ) ]]; then
    complete -o filenames -o nosort -o bashdefault -o default -F _yshjy_complete yshjy yshys
else
    complete -o filenames -o bashdefault -o default -F _yshjy_complete yshjy yshys
fi
```

用户可感知的差别：选项候选**不再被字母序重排**（我们表里 `-h --help` 排最后，会被 bash 挪到中间）。
降级为 🟡 是因为它不难看，只是「顺序不是我们排的那个」。

### 6.3 🟢 `cur/prev` 用 `COMP_WORDS`（应改 `$2`/`$3`）

见 §2.2。带引号的参数场景会不准。改动只有 4 行，顺手就改。

### 6.4 🟢 zsh 的目录补全用 `_directories`，业界用 `_files -/`

见 §3.2。行为接近，`_files -/` 更「原生」。

### 6.5 🟢 fish 的目录补全混入普通文件

见 §4.1。dir 应该 `-r -f -a "(__fish_complete_directories)"`，我们写的是 `-r -F`。

---

## 7. 结论：换不换 clap？—— **不换**，但抄 4 条

**不换的理由**（诚实评估）：

1. 迁移成本：clap derive 要重写 37 个选项的定义，而我们的解析器已经**过测**（`tests/cli_spec_match.sh`
   三方对账 + 29 个探针），迁移会把「已验收的路」重开一遍，收益只有「少维护一个 Python 生成器」。
2. 我们的 `cli_spec.rs` 已经是**唯一真源**，补全 + man + `--help` + 参数解析四方一致 —— 这正是
   clap 生态想要的效果，而**我们已经用 295 行数据表拿到了**。
3. 引入 clap 会让二进制体积增加（clap_builder + clap_lex 约 +0.4 MB），对我们「常用版尽量小」的
   取舍不利（0.9.28 deb 9.8 MB 里已经塞了 GUI）。

**要抄的 4 条（按优先级）**：

| # | 抄什么 | 落点 | 优先级 |
|---|---|---|---|
| 1 | `IFS=$'\n'` 包 `compgen -f` | `gen-cli-assets.py:295`、`:312` | 🔴 真 bug，下次动生成器就改 |
| 2 | `complete` 加 `-o nosort -o bashdefault -o default`（带 bash 4.4 判断） | `gen-cli-assets.py:316` | 🟡 |
| 3 | `cur=$2 / prev=$3`（bash≥4 分支） | `gen-cli-assets.py:246-247` | 🟢 |
| 4 | fish 目录用 `(__fish_complete_directories)`、zsh 目录用 `_files -/` | `:391`、`:332` | 🟢 |

**不抄**：`engine/`（动态补全）与 `env/`（`COMPLETE=` 协议）—— 它们要求程序内实现补全引擎、
并在用户 shell 配置里加 `complete -C`，对我们「装完就能用」的目标是倒退。记在这里当知识即可。

---

## 8. 验收方式（改完之后怎么证明对了）

1. `bash tests/cli_spec_match.sh` —— 三方对账必须仍绿（补全 36 项 = `--help` = man 页）。
2. **新增探针**（建议 `tests/completions.sh`）：
   ```bash
   mkdir -p "$TMP/有 空格"         # 目录名带空格
   touch  "$TMP/有 空格.zip"
   out=$(bash -c 'source completions/yshjy.bash; COMP_WORDS=(yshjy "有"); COMP_CWORD=1;
                  _yshjy_complete; printf "%s\0" "${COMPREPLY[@]}"' | tr -d '\0')
   # 断言：只出现一条候选，且等于「有 空格.zip」
   ```
3. `zsh -n scripts/completions/_yshjy`（语法检查）、`fish -n` 若本机装了 fish。

> 探针的写法照 `tests/` 里既有风格：**每条断言都打印「实测值 vs 期望值」**，失败就 `exit 1`。
