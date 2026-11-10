// 真机复现：队列页在**真实 HomePage 结构里**按键盘有没有反应。
//
// 为什么单写一条：`queue_view_keyboard_test.dart` 是把 `QueueView` 直接当根 widget 挂的，
// 那种情况下「自己请求焦点」必然成功 —— 它证明的是「键位判断逻辑对」，
// 证明不了「真实页面里焦点真落到了队列上」。
//
// 用户在真机按键盘**完全没反应**，就是这一层出的问题：
// 第一版给每行挂 `Focus(autofocus: i == 0)`，真实流程（拖文件 → 队列才出现）里
// 焦点已经在别处，autofocus 抢不到；测试里当根挂所以是假绿。
// 现在实现改成「整块一个 Focus + 挂载后主动 requestFocus」，这条测试专门钉住它：
//   - 真实页面里 primaryFocus 必须就是队列的 FocusNode
//   - 真实页面里按 Delete 必须真能移出一行
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

Future<void> pumpRealQueue(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(const MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugScanItems: [
          {
            'path': '/tmp/一号.zip',
            'name': '一号.zip',
            'size': 1024,
            'kind': 'ZIP',
            'archive': true,
          },
          {
            'path': '/tmp/二号.zip',
            'name': '二号.zip',
            'size': 2048,
            'kind': 'ZIP',
            'archive': true,
          },
        ],
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

void main() {
  setUp(() => EngineClient.transportFactory =
      (_, __) => throw UnimplementedError('这条测试不该起引擎'));
  tearDownAll(() =>
      EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args));

  testWidgets('真实队列页：焦点必须落在队列上（否则键盘全废）', (tester) async {
    await pumpRealQueue(tester);
    expect(find.text('一号.zip'), findsOneWidget, reason: '队列没灌进来，先修这个');
    expect(FocusManager.instance.primaryFocus?.debugLabel, 'queue-list',
        reason: '焦点不在队列上 → 用户按 Del / ↓ / Enter 全都没反应（真机 bug 的复现点）');
  });

  testWidgets('真实队列页按 Delete 能移出一行', (tester) async {
    await pumpRealQueue(tester);
    expect(find.text('一号.zip'), findsOneWidget);
    expect(find.text('二号.zip'), findsOneWidget);

    await tester.sendKeyEvent(LogicalKeyboardKey.delete);
    await tester.pumpAndSettle();

    expect(find.text('一号.zip'), findsNothing,
        reason: '真机上按 Delete 没反应，测试里也删不掉 → 焦点没落到队列上');
    expect(find.text('二号.zip'), findsOneWidget);
  });
}
