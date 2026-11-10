// 键位判定的单测（T11 键盘那一半的底座）。
//
// 这张表是全项目键位的**唯一真源**：它错了，所有页面的快捷键一起错，
// 而且错得隐蔽 —— 按了没反应，用户只会以为「这个功能没做」。
// 所以这里只测纯函数，不 pump 界面：键位规则和「谁在响应」是两件事。

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/keyboard.dart';

void main() {
  test('UI-SPEC §4b.6 + PeaZip 锚点那七个动作一个不少', () {
    expect(keyActionFor(LogicalKeyboardKey.escape), KeyAction.cancel);
    expect(keyActionFor(LogicalKeyboardKey.enter), KeyAction.primary);
    // 小键盘 Enter 必须和主 Enter 等价：外接小键盘/数字区很常见，
    // 少了这一条，用户在数字区按 Enter 会觉得「主操作键坏了」
    expect(keyActionFor(LogicalKeyboardKey.numpadEnter), KeyAction.primary,
        reason: '小键盘 Enter 没接上，用户按了没反应');
    expect(keyActionFor(LogicalKeyboardKey.delete), KeyAction.deleteSelected);
    expect(keyActionFor(LogicalKeyboardKey.keyA, ctrl: true),
        KeyAction.selectAll);
    expect(keyActionFor(LogicalKeyboardKey.keyV, ctrl: true),
        KeyAction.pastePath);
    // PeaZip en.txt:1414「Ctrl+E (or F12) extracts the entire archive」
    expect(keyActionFor(LogicalKeyboardKey.keyE, ctrl: true),
        KeyAction.extract);
  });

  test('带 Shift / Alt 的组合一律不接（规范里没有，接了就是自己拍键位）', () {
    for (final k in [
      LogicalKeyboardKey.enter,
      LogicalKeyboardKey.escape,
      LogicalKeyboardKey.delete,
    ]) {
      expect(keyActionFor(k, shift: true), isNull,
          reason: 'Shift+${k.keyLabel} 不在 UI-SPEC 里，接了会抢输入框的键');
      expect(keyActionFor(k, alt: true), isNull,
          reason: 'Alt+${k.keyLabel} 不在 UI-SPEC 里');
    }
    expect(keyActionFor(LogicalKeyboardKey.keyE, ctrl: true, shift: true),
        isNull);
    expect(keyActionFor(LogicalKeyboardKey.keyA, ctrl: true, alt: true), isNull);
  });

  test('裸字符键不接（否则打字时界面会跟着乱跳）', () {
    for (final k in [
      LogicalKeyboardKey.space,
      LogicalKeyboardKey.keyE,
      LogicalKeyboardKey.keyA,
      LogicalKeyboardKey.keyV,
      LogicalKeyboardKey.backspace, // 队列块的 Del/Backspace 由它自己管
    ]) {
      expect(keyActionFor(k), isNull, reason: '${k.keyLabel} 单独按不该触发动作');
    }
  });

  test('Ctrl+Enter 不算主操作 —— 「多按了个修饰键」必须当没按，不能凑合', () {
    expect(keyActionFor(LogicalKeyboardKey.enter, ctrl: true), isNull);
  });

  test('Ctrl+E 与裸 Esc 各自独立：一个要修饰键，一个是裸键', () {
    expect(keyActionFor(LogicalKeyboardKey.keyE), isNull,
        reason: 'E 单按是打字，不能去解压');
    expect(keyActionFor(LogicalKeyboardKey.keyE, ctrl: true),
        KeyAction.extract);
  });
}
