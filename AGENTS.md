# 杨三海解压（yshjy）—— 给编程助手的速查卡

**先读这一页就够，不要一上来翻 2000 行的 home_page.dart。** 需要细节再去看 `HANDOFF.md` 顶部和 `docs/TODO.md`。

## 这是什么

还原伪装成视频/图片的压缩包，Rust 引擎 + Flutter Linux 界面。当前版本 `0.9.28`。

- 引擎：`src/*.rs`（CLI + 管道协议，零第三方解压库，自己写的 zip/7z/rar/tar 解析）
- 界面：`gui/`（Flutter，Linux 桌面）
- 协议：引擎和界面的唯一通道，`docs/PROTOCOL.md`（stdin/stdout 一行一个 JSON）

## 建置与验收（照抄就对了）

```bash
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
