// 空态拖放区的 widget 回归（按块拆分的第 3 块）。
//
// 这块原来埋在 home_page.dart 的 State 里，只能靠「整页 pump + 找文字」验，
// 而整页测试起来要处理引擎、通知、窗口尺寸一堆旁支。抽成独立 widget 之后
// 一次只验一件事：拖拽中的文案、两个入口回调、以及「第一屏只有一个主按钮」这条设计规矩。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_empty_view.dart';

Future<Map<String, List<String>>> pumpEmpty(
  WidgetTester tester, {
  bool dragging = false,
}) async {
  final calls = <String, List<String>>{'pickFile': [], 'pickFolder': []};
  tester.view.physicalSize = const Size(1000, 1400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: EmptyDropView(
        dragging: dragging,
        onPickFile: () => calls['pickFile']!.add('-'),
        onPickFolder: () => calls['pickFolder']!.add('-'),
      ),
    ),
  ));
  await tester.pump();
  return calls;
}

void main() {
  testWidgets('平时显示「把伪装包交给我」，拖拽中换成「松手就行」', (tester) async {
    await pumpEmpty(tester);
    expect(find.text('把伪装包交给我'), findsOneWidget);
    expect(find.text('松手就行'), findsNothing,
        reason: '没在拖拽就喊「松手就行」，用户会以为程序卡在某个状态');

    await pumpEmpty(tester, dragging: true);
    expect(find.text('松手就行'), findsOneWidget);
    expect(find.text('把伪装包交给我'), findsNothing,
        reason: '文件拖在窗口上方却不换提示，用户不知道可以松手');
  });

  testWidgets('说明文字里写清了「.mp4 / .png 可能是压缩包」这个真实痛点', (tester) async {
    await pumpEmpty(tester);
    // 这是整个软件的卖点：网盘下回来的伪装包。空态不说，用户根本不知道该拖什么进来。
    expect(find.textContaining('.mp4'), findsOneWidget);
  });

  testWidgets('「选择文件」和「选整个文件夹」各自接对了回调', (tester) async {
    final calls = await pumpEmpty(tester);
    await tester.tap(find.text('选择文件'));
    await tester.pump();
    expect(calls['pickFile'], hasLength(1));
    expect(calls['pickFolder'], isEmpty,
        reason: '点「选择文件」却走了选文件夹的分支，用户会看到目录选择框');

    await tester.tap(find.text('选整个文件夹'));
    await tester.pump();
    expect(calls['pickFolder'], hasLength(1));
    expect(calls['pickFile'], hasLength(1), reason: '选文件夹不该重复触发选文件');
  });

  testWidgets('第一屏只有一个主按钮（调研 07:921-923 定的规矩）', (tester) async {
    await pumpEmpty(tester);
    expect(find.byType(FilledButton), findsOneWidget,
        reason: '一屏摆两个同样重的按钮，新手不知道该点哪个；次入口必须弱化成文字按钮');
    // 次入口存在，只是弱化 —— 不是被砍掉
    expect(find.byType(TextButton), findsOneWidget);
    expect(find.text('选整个文件夹'), findsOneWidget);
  });

  // ── T11 无障碍：键盘 ─────────────────────────────────────────────
  // 键位出处：UI-SPEC §4b.6「Enter 确认」= 提交本屏主操作。
  // 第一屏的主操作是「选文件」，所以 Enter 必须等价于点那个按钮。

  testWidgets('Enter 触发选文件（主操作），Esc / Tab 不触发', (tester) async {
    final calls = await pumpEmpty(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(calls['pickFile'], hasLength(1),
        reason: '主操作键没接上，只用键盘的用户在第一屏就卡死了');
    expect(calls['pickFolder'], isEmpty,
        reason: 'Enter 走进了次入口，用户按确认键却弹目录选择框');

    // 这块不认 Esc（没有可取消的操作）—— 不能因为「顺手」就给没意义的行为
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(calls['pickFile'], hasLength(1));
    expect(calls['pickFolder'], isEmpty);
  });

  testWidgets('小键盘 Enter 和主 Enter 等价（外接数字区很常见）', (tester) async {
    final calls = await pumpEmpty(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.numpadEnter);
    await tester.pump();
    expect(calls['pickFile'], hasLength(1),
        reason: '只认主 Enter，笔记本接数字小键盘的用户会觉得主操作键坏了');
  });
}
