# yshjy GUI（Flutter）

杨三海解压的桌面界面。**一套 UI 代码** → Linux 桌面 / Windows / macOS / Android。

## 跑起来

```bash
# 1. 让界面找得到引擎（打包后会随包带，这里开发时指到编译产物）
export YSHJY_BIN=$HOME/hermes/yshjy/target/release/yshjy

cd ~/hermes/yshjy/gui
flutter pub get
flutter run -d linux          # 开发模式（热重载）
flutter build linux --release # 出发布版：build/linux/x64/release/bundle/yshjy_gui
```

不设 `YSHJY_BIN` 时，界面会从 `PATH` 里找 `yshjy`。

## 它是怎么跟引擎说话的

**不走 FFI，走协议**（`docs/PROTOCOL.md`）：

```
Flutter 界面  ──Process.start('yshjy', ['--serve'])──►  Rust 引擎
     ▲                                                   │
     └──────────── 一行一个 JSON（stdin/stdout） ◄────────┘
```

好处（这也是选 Flutter + 协议、而不是 FFI 的原因）：

1. **引擎更新，界面不用改**：只要协议字段不删，加功能后端自己加
2. **界面换框架，引擎不用改**：哪天想换 Tauri，前端重写，Rust 那边一行不动
3. **跨平台一致**：Linux / Windows / macOS / Android 都是起子进程 + 读标准流，
   没有"每个平台写一份 FFI 绑定"的活
4. **调试简单**：`yshjy --serve` 可以手动喂 JSON，问题一眼看得出是前端还是后端

## 代码地图

```
lib/
├── main.dart        应用入口 + 主题（青色系，深色跟随系统）
├── engine.dart      协议客户端：Process 管理、事件解析、错误类型
│                    （同时是"别的框架怎么接"的参考实现）
└── home_page.dart   界面：空状态 / 预览列表 / 进度 / 完成 / 失败
```

## 已实现（第 1 批）

- [x] 启动引擎、握手（hello）
- [x] 选文件 → 列内容（序号 / 名称 / 大小 / 🔒 加密 / ⚠ 可执行）
- [x] 开始解压 → 进度条 + 当前文件 + 取消
- [x] 完成页（解开几个 / 用时 / 输出目录 / 打开目录）
- [x] 失败页（错误码 → 人话；技术备注可选中复制）
- [x] 诊断提醒（可执行文件、索引重建、分卷拼接…）
- [x] 深色模式

## 下一步（第 2 批）

- [ ] 拖文件进窗口（`desktop_drop`）
- [ ] 密码输入框（`PASSWORD_NEEDED` 事件 → 弹框 → 重试）
- [ ] 空间不足确认（`SPACE_SHORTAGE` → 提示能不能继续）
- [ ] 条目勾选（`--select` 的界面；协议 v1 已留字段）
- [ ] 批量队列（多个包排着跑）
- [ ] 打包面板（把目录打成伪装视频/图片）
- [ ] 设置页（语言 / 并发 / 密码本 / 报障）

## 注意

- 界面**不自己造中文句子之外的文案逻辑**：所有错误文案在这里按 `code` 查（`_errText`），
  想加语言就把它挪成表，加一层语言包即可。
- 事件里出现不认识的 `kind` **必须忽略**（现在的 `switch` 已经这么做了）——
  这是协议兼容性的底线，别改成 `default: throw`。
