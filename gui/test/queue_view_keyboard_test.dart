// 队列视图的键盘回归（T11 按块推进的第一块）。
//
// 为什么测 QueueView 而不是整个 HomePage：这一块抽出来之后只依赖「一个队列列表 +
// 六个回调」，构造一个假队列就能验键盘 —— 不用起假引擎、不碰文件 I/O，跑得快也不会
// 在假时钟里留下 pending Timer。
//
// 钉的是四件事：
//   ① Del / Backspace 移出当前行（D4 的老行为，搬移后不能丢）
//   ② ↑ / ↓ 在行间移动焦点
//   ③ Ctrl+Enter = 开始解压（和大按钮同一件事）
//   ④ 一个可解的都没有时，Ctrl+Enter 什么都不做（按钮那时也是禁用的）

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_queue_view.dart';

QueueItem item(String name, {bool archive = true}) => QueueItem(
      path: '/tmp/$name',
      name: name,
      size: 1024,
      kind: 'Zip',
      volumes: 1,
      archive: archive,
    );

/// 装一块队列视图，把六个回调换成可数的记录器
Future<Map<String, List<int>>> pumpQueue(
  WidgetTester tester,
  List<QueueItem> queue,
) async {
  final calls = <String, List<int>>{
    'reset': [],
    'remove': [],
    'cancelRow': [],
    'tryOther': [],
    'startAll': [],
  };
  tester.view.physicalSize = const Size(1200, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: QueueView(
        queue: queue,
        foldersScanned: 0,
        onReset: () => calls['reset']!.add(-1),
        onRemove: (i) => calls['remove']!.add(i),
        onCancelRow: (i) => calls['cancelRow']!.add(i),
        onTryOtherCandidates: (i) => calls['tryOther']!.add(i),
        onStartAll: () => calls['startAll']!.add(-1),
      ),
    ),
  ));
  await tester.pump();
  return calls;
}

/// 按 Enter（不带修饰键）
Future<void> pressEnter(WidgetTester tester) async {
  await tester.sendKeyEvent(LogicalKeyboardKey.enter);
  await tester.pump();
}

/// 按 Ctrl+E（PeaZip 的惯例键位）
Future<void> pressCtrlE(WidgetTester tester) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyE);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.pump();
}

void main() {
  testWidgets('Enter = 开始解压（UI-SPEC 4b.6 定的「Enter 确认」）', (tester) async {
    final calls = await pumpQueue(tester, [item('a.zip')]);
    await pressEnter(tester);
    expect(calls['startAll'], hasLength(1),
        reason: 'UI-SPEC §4b.6 写的是「Enter 确认」，按了没反应就是没接上');
  });

  testWidgets('Ctrl+E 也能开始解压（对齐 PeaZip 的惯例键位）', (tester) async {
    final calls = await pumpQueue(tester, [item('a.zip')]);
    await pressCtrlE(tester);
    expect(calls['startAll'], hasLength(1),
        reason: '从 PeaZip 迁过来的用户会先按 Ctrl+E');
  });

  testWidgets('一个能解的都没有时 Enter / Ctrl+E 都不动作', (tester) async {
    final calls = await pumpQueue(tester, [item('说明.txt', archive: false)]);
    await pressEnter(tester);
    await pressCtrlE(tester);
    expect(calls['startAll'], isEmpty,
        reason: '按钮这时是禁用的；快捷键绕过禁用状态发请求，引擎会收到一个空批次');
  });

  testWidgets('Del 把当前行移出队列（D4 老行为，搬移后不能丢）', (tester) async {
    final calls = await pumpQueue(tester, [item('a.zip'), item('b.zip')]);
    await tester.sendKeyEvent(LogicalKeyboardKey.delete);
    await tester.pump();
    expect(calls['remove'], [0], reason: '默认选中第一行，Del 应该是移走它');
  });

  testWidgets('↓ / ↑ 移动选中的行，看得见（不是滚页面）', (tester) async {
    await pumpQueue(tester, [item('a.zip'), item('b.zip')]);
    bool sel(int i) =>
        tester.widget<ListTile>(find.byType(ListTile).at(i)).selected;
    expect(sel(0), isTrue, reason: '打开第一行就该是选中的');

    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    expect(sel(0), isFalse);
    expect(sel(1), isTrue,
        reason: '↓ 之后选中行没动，屏幕上一点变化都没有 —— 用户会以为键盘坏了（真机第一版就是这样）');

    await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await tester.pump();
    expect(sel(0), isTrue);

    // 已经在第一行还按 ↑，不该越界
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await tester.pump();
    expect(sel(0), isTrue, reason: '第一行再往上走，说明没做边界保护');
  });

  testWidgets('普通包只看得到状态，不该出现「换个地方试」', (tester) async {
    final calls = await pumpQueue(tester, [item('a.zip')]);
    expect(find.text('换个地方试'), findsNothing);
    expect(calls['tryOther'], isEmpty);
  });
}
