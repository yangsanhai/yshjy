// 预览页的键盘回归（T11 无障碍）。
//
// 为什么要单独一个文件：预览页的键位分散在三层（页面主体管 Enter/Ctrl+E/Esc，
// 文件树管 Ctrl+A，密码框管自己的回车），而它们靠「焦点冒泡」协作 ——
// 任何一层抢错键，表现都是「按了没反应」，只能靠真按一次才看得出来。
//
// 键位出处：UI-SPEC §4b.6（Enter 确认 / Esc 取消 / Ctrl+A 全选）
//          + PeaZip en.txt:1414（Ctrl+E 解压整包）。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';

import 'fake_engine.dart';

EntryItem file(String path, int index, int size) => EntryItem(
      index: index,
      path: path,
      size: size,
      packed: size,
      isDir: false,
      encrypted: false,
      risky: false,
    );

/// 3 个文件 / 2 个目录（和 preview_tree_widget_test 用同一份，便于对照）
Preview sample() => Preview(
      format: 'zip',
      complete: true,
      totalSize: 60,
      encryptedCount: 0,
      encKind: null,
      entries: [
        file('合集A/第一季/一.txt', 1, 10),
        file('合集A/第一季/二.txt', 2, 20),
        file('另一个/三.txt', 3, 30),
      ],
      hints: const [],
    );

Finder rowOf(String name) => find
    .ancestor(of: find.text(name), matching: find.byType(ListTile))
    .first;

Finder checkOf(String name) =>
    find.descendant(of: rowOf(name), matching: find.byType(Checkbox));

late FakeEngine fake;

Future<void> pumpPreview(WidgetTester tester) async {
  // 解压完会发桌面通知（notify-send 走 Process.run）—— 测试里必须关掉，
  // 否则进程定时器挂到测试结束，报「A Timer is still pending」
  Notifier.enabled = false;
  fake = FakeEngine();
  installFakeEngine(fake);
  // 解压立刻回一个成功结果：不回包的话 `extract` 的 6 小时超时定时器会挂着，
  // 测试结束时 flutter_test 报「A Timer is still pending」
  fake.autoReply['extract'] = {
    'ok': true,
    'result': {
      'outcome': 'Success',
      'entries_done': 3,
      'entries_total': 3,
      'bytes_written': 60,
      'dest': '/tmp/out',
      'millis': 10,
    },
  };
  // 视口要够大：列表是 ListView，看不见的行不 build（同 preview_tree_widget_test）
  tester.view.physicalSize = const Size(1200, 2000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(body: HomePage(autoStart: false, debugPreview: sample())),
  ));
  await tester.pumpAndSettle();
}

void main() {
  tearDownAll(restoreRealTransport);

  testWidgets('Enter = 开始解压（本屏主操作）', (tester) async {
    await pumpPreview(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(fake.lastCmd('extract'), isNotNull,
        reason: '预览页主操作键没接上，键盘用户只能去点那个青色按钮');
  });

  testWidgets('Ctrl+E = 开始解压（PeaZip 惯例）', (tester) async {
    await pumpPreview(tester);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyE);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(fake.lastCmd('extract'), isNotNull,
        reason: 'Ctrl+E 是 PeaZip 用户的老肌肉记忆，没接上会以为「这软件不专业」');
  });

  testWidgets('Esc = 取消回首页空态', (tester) async {
    await pumpPreview(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.text('把伪装包交给我'), findsOneWidget,
        reason: 'Esc 不能退，用户在预览页想「我不解了」只能去找鼠标');
  });

  testWidgets('Ctrl+A = 全选（取消几个之后能一键勾回来）', (tester) async {
    await pumpPreview(tester);
    // 先取消一个目录（连带它下面两个文件）
    await tester.tap(checkOf('合集A'));
    await tester.pumpAndSettle();
    expect(find.textContaining('已选 1 / 3'), findsOneWidget,
        reason: '前提没成立：取消勾选本身就没生效，后面的全选断言会失去意义');

    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pumpAndSettle();

    expect(find.textContaining('已选 3 / 3'), findsOneWidget,
        reason: 'Ctrl+A 没接上（或只勾了当前层），用户得一个个点回来');
  });
}
