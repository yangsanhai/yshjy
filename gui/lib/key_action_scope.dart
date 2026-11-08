// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 「这块界面认哪些键」的通用外壳
///
/// 用法：把一屏包起来，给它一张「动作 → 回调」表。
/// 表里**没有的动作直接放行**（`KeyEventResult.ignored`），让按键继续往上层冒泡 ——
/// 这条是嵌套能work的关键：文件树只认 `Ctrl+A`，别的键交给外层的预览页处理。
///
/// # 焦点怎么拿到（照抄队列块的教训，别重犯）
///
/// 第一版队列块给每行挂 `Focus(autofocus: i == 0)`，结果真机上按键盘完全没反应：
/// 真实流程（拖文件 → 队列才出现）里焦点已经在别处，`autofocus` 抢不过。
/// 后来改成「挂载后第一帧主动 requestFocus」，真机才真的能用
/// （`test/home_queue_keyboard_integration_test.dart` 钉着这件事）。
///
/// 所以这里**不用 autofocus**，用同一套 postFrame requestFocus。
///
/// ★ 嵌套时谁赢：父先挂载、子后挂载，postFrame 回调按注册顺序执行 → **子最后请求，子赢**。
/// 正好是想要的：文件树拿到焦点后只吃 `Ctrl+A`，其余键冒泡给外层的预览页。
///
/// ★ 只有**同一时刻独一份**的块能 `takeFocus: true`。`IndexedStack` 里那五个页面
/// 是同时挂着的，谁抢焦点谁出事 —— 所以非首页的页面一律传 false，
/// 等各自接线时再单独处理。
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'keyboard.dart';

class KeyActionScope extends StatefulWidget {
  /// 本块认的键。值为 null 表示「这个键我看见了但现在不该响应」
  /// （按钮禁用时就是这种情况）—— 那也按已处理吞掉，免得冒泡上去触发别的块的主操作。
  final Map<KeyAction, VoidCallback?> handlers;

  final Widget child;

  /// 挂载后主动拿焦点。同一时刻只有一个块在树上时才给 true。
  final bool takeFocus;

  const KeyActionScope({
    super.key,
    required this.handlers,
    required this.child,
    this.takeFocus = true,
  });

  @override
  State<KeyActionScope> createState() => _KeyActionScopeState();
}

class _KeyActionScopeState extends State<KeyActionScope> {
  final FocusNode _focus = FocusNode(debugLabel: 'key-action-scope');

  @override
  void initState() {
    super.initState();
    if (!widget.takeFocus) return;
    // 挂载后第一帧才请求：initState 阶段节点还没进树，requestFocus 会静默失败
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focus.requestFocus();
    });
  }

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode _, KeyEvent event) {
    // 只处理「按下」：抬起和重复会让长按反复触发
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final action = actionForEvent(event);
    if (action == null) return KeyEventResult.ignored;
    if (!widget.handlers.containsKey(action)) return KeyEventResult.ignored;
    // ★ 正在输入框里打字时，Enter 属于输入框（提交 / 换行），不能被我们截走。
    //   真实事故形态：用户在「输出文件名」里改个名字，按回车想确认 ——
    //   结果整个打包被启动了（打包是重操作，几十秒还写盘）。
    //   判定用「当前焦点是不是落在 EditableText 上（含祖先）」，
    //   **不是**「这个页面有没有 TextField」—— 同一个页面既有输入框也有非输入状态，
    //   按页面判会把正常状态一起废掉。
    if (action == KeyAction.primary && _isEditingText()) {
      return KeyEventResult.ignored;
    }
    widget.handlers[action]?.call();
    // 有这一条就算「我管了」—— 即使值为 null（按钮禁用中）也要吞掉，
    // 否则禁用的主操作键会冒泡上去触发别的块，那就成了「按了没用的键干了别的事」
    return KeyEventResult.handled;
  }

  /// 焦点此刻是不是在某个文本输入框里
  ///
  /// TextField 真正持有焦点的是内部的 `EditableText`，所以先看当前节点自身，
  /// 再顺着祖先链找 —— 有些场景（输入框里套了别的可聚焦件）焦点不在 EditableText 上，
  /// 但祖先是它，选区的光标照样在闪。
  bool _isEditingText() {
    final ctx = FocusManager.instance.primaryFocus?.context;
    if (ctx == null) return false;
    if (ctx.widget is EditableText) return true;
    var editing = false;
    ctx.visitAncestorElements((e) {
      if (e.widget is EditableText) {
        editing = true;
        return false; // 找到就停
      }
      return true;
    });
    return editing;
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      focusNode: _focus,
      onKeyEvent: _onKey,
      child: widget.child,
    );
  }
}
