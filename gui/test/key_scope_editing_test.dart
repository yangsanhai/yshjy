// 「输入框里的回车归输入框」—— T11 收尾时补的正确性问题
//
// 场景：打包页的「输出文件名」、设置页的一堆输入框。用户在框里改完按回车想确认，
// 如果 KeyActionScope 把 Enter 截走，整个打包就被启动了（几十秒还写盘）。
//
// 判定必须是「焦点此刻在不在 EditableText 上」，不能是「这个页面有没有 TextField」——
// 同一页面既有输入框也有非输入状态，按页面判会把正常状态一起废掉。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/key_action_scope.dart';
import 'package:yshjy_gui/desktop/keyboard.dart';

void main() {
  testWidgets('焦点在输入框里时，Enter 归输入框（不触发主操作）', (tester) async {
    var fired = 0;
    final other = FocusNode();
    addTearDown(other.dispose);

    await tester.pumpWidget(MaterialApp(
      home: KeyActionScope(
        takeFocus: false,
        handlers: {KeyAction.primary: () => fired++},
        child: Scaffold(
          body: Column(
            children: [
              const TextField(key: ValueKey('tf')),
              // 非输入的可聚焦区：用来把焦点移出输入框又不停在按钮上
              // （按钮自己有 Enter 处理，会把事件吃掉，测不出冒泡）
              Focus(focusNode: other, child: const SizedBox(width: 20, height: 20)),
            ],
          ),
        ),
      ),
    ));

    // ① 点进输入框 → 焦点落在 EditableText 上
    await tester.tap(find.byKey(const ValueKey('tf')));
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(fired, 0,
        reason: '在输入框里按回车必须归输入框（提交/换行），'
            '否则用户在「输出文件名」里按个回车就把整个打包启动了');

    // ② 焦点离开输入框 → Enter 才该触发主操作
    other.requestFocus();
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(fired, 1, reason: '焦点不在输入框时 Enter 必须能用');
  });

  testWidgets('焦点不在输入框时，Enter 一次只触发一次（抬起/重复不算）', (tester) async {
    var fired = 0;
    await tester.pumpWidget(MaterialApp(
      home: KeyActionScope(
        takeFocus: true, // 自己拿焦点
        handlers: {KeyAction.primary: () => fired++},
        child: const Scaffold(body: SizedBox.expand()),
      ),
    ));
    await tester.pump(); // 等 postFrame requestFocus
    await tester.sendKeyDownEvent(LogicalKeyboardKey.enter);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(fired, 1);
  });

  testWidgets('handlers 里没有的键不吞（要冒泡给上层）', (tester) async {
    var fired = 0;
    await tester.pumpWidget(MaterialApp(
      home: KeyActionScope(
        takeFocus: true,
        handlers: {KeyAction.cancel: () => fired++},
        child: const Scaffold(body: SizedBox.expand()),
      ),
    ));
    await tester.pump();
    // 只认 Esc，Enter 应该原样放行
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(fired, 0);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(fired, 1);
  });
}
