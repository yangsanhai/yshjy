# 怎么发布到 GitHub（发布库专用说明）

这个文件夹（`~/hermes/yshjy-github`）是**发布库**：脱敏后、历史清干净的版本，专门用来推 GitHub。
开发库还是 `~/hermes/yshjy`（保留真实路径、完整历史、编译产物），两边不要混。

## 两边的差别（只有这些）

| | 开发库 `~/hermes/yshjy` | 发布库 `~/hermes/yshjy-github` |
|---|---|---|
| 家目录路径 | 真实 `/home/<作者>` | 脱敏成 `/home/user`（`assets/sfx/yshjy-sfx.exe` 里的 PE 调试串除外，改了会坏文件）|
| 历史 | 445 个提交，含早期误提交的 `target-full/`（原始 778MB） | 445 个提交，`target-full` 已从历史抹掉（打包 232MB → 31MB）|
| 编译产物 | `target*/`、`dist*/`、`gui/build` 都在 | 一个都不带（`.gitignore` 挡着，克隆时只带跟踪文件）|
| `.git` | 完整 | 独立历史（被 filter-repo 重写过），**与开发库不再共享提交号** |

## 每次更新（固定五步）

1. 在开发库改完，`bash tests/all.sh` 全绿，提交。
2. 同步到发布库（先 `--dry-run` 看清要搬什么）：

   rsync -rlt --no-perms --no-owner --no-group --delete \
     --exclude='.git/' --exclude='target*/' --exclude='dist*/' \
     --exclude='gui/build/' --exclude='gui/.dart_tool/' \
     --exclude='gui/linux/flutter/ephemeral/' --exclude='gui/android/.gradle/' \
     --exclude='ffi/target/' --exclude='spike/build/' \
     ~/hermes/yshjy/ ~/hermes/yshjy-github/

3. 脱敏（幂等，跑几次都一样）：
   `cd ~/hermes/yshjy-github && python3 scripts/desensitize.py`
   复核：`python3 scripts/desensitize.py --check` 应回「干净」。
4. 本地验证（发布库没有 `target/`，第一次要重新编，约 3 分钟）：
   - `cargo test` / `cargo clippy -- -D warnings` / `cargo deny check licenses`
   - `cd gui && flutter pub get && flutter analyze && flutter test`
5. 提交并推送：
   - `git add -A && git commit -m "sync: 从开发库同步（版本 x.y.z）"`
   - `git push`（首次要 `git remote add origin ...`；用 HTTPS 时记得走代理）

## 别在这边做的事

- 别在发布库直接改代码 —— 会被下次同步覆盖（唯一例外是专属于发布的东西，比如 `scripts/desensitize.py` 和这份说明）。
- 别拿两边的提交号互相对照：历史被重写过了，`git log` 的 sha 对不上是正常的。
- 别在开发库里跑脱敏脚本（会把本地开发用的真路径改掉，虽然脚本只在发布库放着）。

## CI 的三条约定（改 `.github/workflows/ci.yml` 前先读）

- `cargo fmt --check` **只当提示**：项目是手写紧凑风格 + 中文标识符，硬门禁必红（本机实测差异 4223 行），本地门禁也只记提示。
- Flutter 版本**钉在 3.44.9**：与开发机同版本，否则 lint 规则和渲染结果都会漂。
- 金标（像素比对）在 CI 里**跳过**（`YSHJY_SKIP_GOLDEN=1`）：runner 的中文字体与渲染栈和开发机不同，跨环境比必红；金标由开发机负责，各平台存各自的基准图（`goldens/mobile_home_empty_<平台>.png`）。

## 内容脱敏（2026-11-11 血的教训）

**路径脱敏 ≠ 内容脱敏。** 仓库公开上线后用户发现正文里还留着他真实的文件名、解压密码和网盘目录号
（README / DESIGN / CLI-SPEC / UI-SPEC / tests/README / 7 语界面文案，共 122 处），
而 `desensitize.py` 当时只处理了 `/home/<作者>`。

推送前必须多跑这一步（词表放在**库外**、权限 600，脚本本身不含任何敏感词）：

```
python3 scripts/desensitize.py --denylist ~/hermes/.private/yshjy-denywords.txt --check
```

返回 0 才能推。命中只报文件名与规则序号，不把词回显到终端/日志。

词表要覆盖：真实文件名、真实解压密码、网盘目录号、任何能定位到个人的路径片段。
新发现一个就追加一行，别只改文件不记账。

**历史里已经泄露过的词**只能 `git filter-repo --replace-text <表达式文件> --force` 重写历史再强推。
注意两点：
- GitHub **不会立刻回收旧对象**：force push 后旧提交号仍能通过 API 打开几天到几周
  （实测 2026-11-11）。要立刻彻底清，只能联系 GitHub Support 请求 purge，或删库重建。
- `--path` / `--path-glob` 是「**只保留**这些路径」，不是「只改这些路径」——
  2026-11-11 用 `--path-glob '*.md'` 想把替换限制在文档里，结果把 `src/`、`tests/`、`scripts/`
  全删了（1067 个文件只剩 114 个）。要么全库替换，要么写脚本精确替换。

**rsync 会带回大样本**：`spike/testdata/`（50MB）和 `tests/data/`（47MB）在开发库里是完整的，
同步后发布库工作树也会多出来。发布库的 `.gitignore` 已经挡住了它们，但提交前务必确认：

```
git status --short | grep -E 'spike/testdata|tests/data' | head
```

只应该有清单（`M`/`D`）或那 19 个放行的小夹具，**不能出现几十个 ??
（2026-11-11 就因为没检查，把 50MB 大样本 add 进库，白推了一次 31MB）。**
