/// 桌面键盘操作 —— 键位判定的**唯一真源**（T11 无障碍 · 键盘那一半）
///
/// # 键位从哪来（一个都不自己拍）
///
/// `docs/UI-SPEC.md` §4b.6「键鼠操作约定」+ §七「无障碍与细节」：
///   Enter 确认（= 本屏主操作）/ Del 删除选中（要确认）/ Esc 取消当前操作或关面板
///   Ctrl+A 全选 / Ctrl+V 粘贴路径（自动开始识别）
/// `docs/REFERENCE-INDEX.md` 的「键盘快捷键」行补了 Ctrl+E：
///   `refs/外部-归档软件/PeaZip/peazip-sources/res/share/lang/en.txt:1414`
///   原文 `keyboard shortcuts: Ctrl+E (or F12) extracts the entire archive`
///   —— 我们只抄 Ctrl+E，不抄 F12（F 键区留给以后做「F 键全家桶」时统一规划）。
///
/// # 为什么手写 `keyActionFor` 而不是 Flutter 的 Shortcuts + Actions
///
/// 参考项目里 LocalSend 用的是标准 `Shortcuts` + `SingleActivator` + `Actions`
/// （`refs/外部-归档软件/LocalSend/app/lib/widget/watcher/shortcut_watcher.dart`）。
/// 那套机制多出来的价值只有两个：**键位能重映射**、**Intent 能被任意上层截获**。
/// 我们两样都不需要（单窗口小工具、键位按规范固定），而多出来的成本是每个动作都要
/// 配一对 Intent + Action 类。所以选 `Focus.onKeyEvent` —— 和队列块
/// （`pages/home_queue_view.dart`）同一套写法，全项目只有一种范式，不混两套。
///
/// ★ 但 LocalSend 那份文件里留了两个真坑，抄机制的时候必须记住：
///   1. **不能用 `LogicalKeySet`**：它要求按键组合**精确匹配**，Linux 上开着
///      Num Lock 时 Enter 会带上 numpad 修饰，导致匹配失败
///      （localsend#3481）。所以这里判的是「逻辑键 + 现在按着哪几个修饰键」，
///      由 [HardwareKeyboard] 现读，不建 key set。
///   2. macOS/Linux 的「假 Meta 键」（localsend#2037）—— 我们不接 Meta 组合，
///      天然无关。
library;

import 'package:flutter/services.dart';

/// 一次按键要触发什么
///
/// 名字是**动作**不是**按键**：同一个动作以后可以换键位，调用方不用跟着改。
enum KeyAction {
  /// 本屏主操作（UI-SPEC §4b.6「Enter 确认」）
  primary,

  /// 解压整包（PeaZip `Ctrl+E`）
  extract,

  /// 取消当前操作 / 返回上一屏 / 关面板（`Esc`）
  cancel,

  /// 全选（`Ctrl+A`）
  selectAll,

  /// 把剪贴板里的路径粘进来并自动识别（`Ctrl+V`）
  pastePath,

  /// 删除选中项（`Del`，删除前必须再确认）
  deleteSelected,
}

/// 按键 → 动作（纯函数，白盒可测）
///
/// `ctrl` / `shift` / `alt` 由调用方从 [HardwareKeyboard] 现读传进来 ——
/// 不读全局状态，测试才能把每种组合都钉一遍。
///
/// 规则（照抄上面的出处，别在这里加「顺手也支持一下」的键）：
///   Ctrl+E → 解压 · Ctrl+A → 全选 · Ctrl+V → 粘贴路径
///   Esc    → 取消 · Enter / 小键盘 Enter → 主操作 · Del → 删除选中
/// 带 Shift / Alt 的组合**一律不接**：规范里没有，接了就是自己拍键位。
KeyAction? keyActionFor(
  LogicalKeyboardKey key, {
  bool ctrl = false,
  bool shift = false,
  bool alt = false,
}) {
  // 带 Shift / Alt 的一律让路（包括 Shift+Enter：输入框换行、多选扩展都靠它）
  if (shift || alt) return null;

  if (ctrl) {
    if (key == LogicalKeyboardKey.keyE) return KeyAction.extract;
    if (key == LogicalKeyboardKey.keyA) return KeyAction.selectAll;
    if (key == LogicalKeyboardKey.keyV) return KeyAction.pastePath;
    return null;
  }

  if (key == LogicalKeyboardKey.escape) return KeyAction.cancel;
  if (key == LogicalKeyboardKey.enter ||
      key == LogicalKeyboardKey.numpadEnter) {
    return KeyAction.primary;
  }
  if (key == LogicalKeyboardKey.delete) return KeyAction.deleteSelected;
  return null;
}

/// 从一次真实按键事件里取动作（修饰键现读，避开 Num Lock 精确匹配的坑）
KeyAction? actionForEvent(KeyEvent event) {
  final k = HardwareKeyboard.instance;
  return keyActionFor(
    event.logicalKey,
    ctrl: k.isControlPressed,
    shift: k.isShiftPressed,
    alt: k.isAltPressed,
  );
}
