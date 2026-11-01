# 杨三海解压（yshjy）—— 给编程助手的速查卡

**先读这一页就够，不要一上来翻 2000 行的 home_page.dart。** 需要细节再去看 `HANDOFF.md` 顶部和 `docs/TODO.md`。

## 这是什么

还原伪装成视频/图片的压缩包，Rust 引擎 + Flutter Linux 界面。**版本号以 `Cargo.toml` 为准**（别在这页写死，写死必过期；写这份时是 `0.9.52`）。

- 引擎：`src/*.rs`（CLI + 管道协议，零第三方解压库，自己写的 zip/7z/rar/tar 解析）
- 界面：`gui/`（Flutter，Linux 桌面）
- 协议：引擎和界面的唯一通道，`docs/PROTOCOL.md`（stdin/stdout 一行一个 JSON）

## 仓库在两台机器上（同一份，走移动硬盘）

> 用户 2026-10-30 定的：**这个项目要在两台电脑上一起做**，仓库放在**移动硬盘**里，
> 哪台机器上开工就把硬盘插上去。**同一份 `.git` 历史**，两边都能提交、都能 `git pull`。

| 机器 | 仓库路径 | 系统 | 能做什么 |
|---|---|---|---|
| 开发机（旧） | `~/hermes/yshjy` | Linux | 完整（引擎 + Flutter 界面 + 出 deb 包 + 跑 `tests/all.sh`） |
| 这台（新） | `E:\yshjy-20261030` | Windows 11 | 引擎已通（MSVC）；**图形界面是 Windows 的主战场** |

**跨机器的四条**：
1. **`target/` 不要同步、也不要当备份** —— 它是按平台生成的（Linux 那份 14 G 在 Windows 上一个也用不上），删了随时重建。
2. **别删 `.git`** —— 两边靠它同步。
3. **NTFS 不支持 Unix 软链**：Linux 上 69 个软链拷过来已被展开成实体文件（文件数比源多 343 个），
   **反向拷回 Linux 时注意这一点**，别以为是多了什么。
   **同样被丢掉的还有可执行位**：`git status` 里那些 `scripts/*.sh`、`tests/*.sh`、
   `assets/sfx/yshjy-sfx.exe` 的「修改」，全是 `mode change 100755 => 100644`，
   **内容一个字节没变**（2026-10-30 核对过）。拷回 Linux 后要 `chmod +x` 回来，
   **别在 Windows 上把这批 mode change 提交掉** —— 那会把 Linux 那边的执行位也一起改坏。
4. 开工前先 `git status` 看一眼有没有另一台机器没提交的改动；提交仍按「硬规矩 3」
   （改完停手，监工审过再提），别因为是新机器就自己 commit。

## 建置与验收（照抄就对了）

```bash
# Windows 这台 —— 编译前先看 docs/TODO.md 的 W10（代理要置空、沙箱要关）
cd /e/yshjy-20261030
CARGO_HTTP_PROXY= cargo build --release     # 产物 target/release/yshjy.exe
CARGO_HTTP_PROXY= cargo test
# 引擎（Rust）
cd ~/hermes/yshjy
cargo test                    # 单元测试，300+ 条
cargo clippy -- -D warnings   # 必须干净
bash tests/all.sh             # 一条命令全回归（编译→clippy→单测→协议探针→冒烟→界面测试）

# 界面（Flutter）—— 注意 flutter 不在非交互 shell 的 PATH 里
export PATH="$HOME/flutter/bin:$PATH"
cd ~/hermes/yshjy/gui
flutter test                  # 现在 9 条
flutter build linux --debug   # CMake 开了 -Wall -Werror，任何警告都是错误
```

改完一处就只跑相关的那条，**别每次全跑**：改引擎跑 `cargo test`，改界面跑 `flutter test`，跨两边的才 `bash tests/all.sh`。

## 目录地图（改之前先定位，别瞎逛）

| 要找什么 | 去哪 |
|---|---|
| 解压主流程 | `src/extract.rs`（zip/7z/单流）、`src/extract_rar.rs`、`src/unpack.rs` |
| 原子写入（`.yshjy-part` → 正式名） | `src/io/atomic.rs` |
| 管道协议 / 命令分发 | `src/serve.rs`、`src/api.rs`（`事件种类` 常量是事件名的真值） |
| 任务与取消 | `src/job.rs`（CancelToken 按任务号） |
| 打包 | `src/pack/mod.rs`（`PACK_PROGRESS` 数据源在 223 行附近） |
| 界面外壳 / 托盘 / 关窗 | `gui/lib/app_shell.dart`、`gui/linux/runner/my_application.cc` |
| 界面各页 | `gui/lib/pages/`（home_page 最大，2000 行，按需搜关键字） |
| 引擎客户端 | `gui/lib/engine.dart`（`EngineClient.instance.events` 是事件流） |
| 人类可读文案 | `gui/lib/humanize.dart`（错误码/诊断码→中文，查不到不许露原始码） |

## 硬规矩（踩过线的）

1. **注释写"为什么"**，中文，不写"这行做了什么"。看现有代码风格再动手。
2. **Dart 和 bash 都不认中文变量名**（Rust 认）。别用。
3. **不许 `git commit`** —— 改完停手，diff 交给监工审。
4. **只改被点名的那几个文件**，顺手看到的问题记下来汇报，不要动手。
5. **版本号别动**，打包发版是单独流程。
6. 界面文案一律中文；错误原因说人话（"没有写入权限"而不是 `EACCES`）。

## 跟用户（监工）怎么配合 —— 他反复强调过的，别踩

> 这几条是这几个月一起干活踩出来的，比任何技术细节都优先。**违反哪一条，活干得再漂亮也白干。**

1. **交付要分层证据**：改了界面就必须有"有人真点过 + 截图"这一层。汇报写清**哪层验了、哪层没验**；
   没跑就写"**未验**"，不许写"应该没问题"。他反感反复保证然后返工。
   （正文见 `docs/AI-验收方案.md`，但注意那份里的"AI 自己点"已被后来的规矩取代 —— 见下条。）
2. **截屏和点击由他本人做**：把要点的位置说到像素级（"左侧导航最后一项""底部按钮上面那行最右边"），
   他自己点、自己截图回传。**即使工具能截能点，也绝不自己操纵他的窗口** —— 他会当场质问"谁让你截屏了"。
   要动界面之前先说清：现在在干什么、为什么、还剩几步。
3. **大活一步一步来**：先只列清单，再按顺序修。别自作主张扩范围、别提前动手、别把计划写成一堆新清单。
   过程中发现的新问题**记下来下一轮说**。
4. **报 bug 要顺藤查同类**：不只修报的那一处，要在全库扫同款错位，并补一条能反向验证的回归测试。
5. **说"拆规则"就要能落成 if 判断的规则规格书**（六节），不是战略分析。AI 产出的东西跑不通时，
   他只想要**问题清单**，别擅自改它的代码（改我们自己的脚本可以）。
6. **大活收尾**：更新记忆 + 项目文档 + 清过程文件。临时产物直接清；成品/旧安装包先列清单问过再动。
7. **待办只留一个真源**：一个新清单出现，旧的就改成指针（本项目就是 `docs/TODO.md`）。
   翻旧账前先看该文件顶部的「已裁定不做」—— 已裁定 = 事实，别再当欠账提出来。

## 已知的坑（省你半小时）

- `flutter` 不在 PATH：先 `export PATH="$HOME/flutter/bin:$PATH"`。
- C++ 侧 `-Wall -Werror`：未使用的参数要 `(void)x;`。
- `GtkMenuItem` 没有 `"clicked"` 信号，要连 `"activate"`。
- GTK3 新建的 widget 默认 hidden，菜单项不放 `gtk_widget_show` 菜单就是空的。
- `app_indicator_new` 在 libayatana-appindicator3 0.5.94 被标 deprecated，`-Werror` 下要在那一行就地 `#pragma GCC diagnostic ignored "-Wdeprecated-declarations"`。
- 系统托盘要 `libayatana-appindicator3-dev`（已装），GNOME 需要 appindicator 扩展（已启用）。
- 引擎所有落盘都走原子写入，写盘代码必须留 `.yshjy-part` 中间态，别直接写正式名。
- 测试里 MethodChannel 没有原生实现会抛 `MissingPluginException`，包装类必须吞掉异常，否则 widget 测试全红。

## 干活流程

1. 先看 `HANDOFF.md` 顶部 20 行 + `docs/TODO.md` 第一行，确认现在这一轮在做什么。
2. 只做被点名的那一项，做完跑对应测试。
3. 报告：改了哪几个文件、为什么、测试结果。不要长篇大论。
