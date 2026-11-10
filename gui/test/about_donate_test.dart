// 关于页「赞赏」的回归
//
// 这条盯三件事（都是坏了也看不出来的那种）：
//   ① 卡片和按钮在 —— 功能被删掉页面照样能开，只有测试能卡住；
//   ② 点按钮能弹出赞赏码 —— 弹不出来用户就等于看不到码；
//   ③ 二维码是**本地资源**，必须真的打进包里 —— 图片丢了界面只会显示一句提示，
//      用户会以为"这软件没做赞赏"，而所有别的测试都会绿。
//
// 明确不测的：微信能不能扫出这张图。那是图片内容的事，测不了也不该假装测了。
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/pages/about_page.dart';

import 'fake_engine.dart';

Future<void> pumpAbout(WidgetTester tester) async {
  // 视口要比原来高：2026-11-05 关于页多了「支持的格式」「快捷键」两张表
  // （从设置页搬来的），ListView 只构建可视区，矮了会让下面的赞赏按钮「找不到」。
  tester.view.physicalSize = const Size(1000, 5200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(const MaterialApp(
    home: Scaffold(body: AboutPage(engineVersion: '0.9.55')),
  ));
  await tester.pumpAndSettle();
}

void main() {
  FakeEngine? fake;

  setUp(() async {
    // 关于页现在内嵌「支持的格式」表，进页面就会问引擎 hello.formats。
    // 不装假引擎的话会去真 spawn 子进程 —— Process.start 的定时器在假时钟里
    // 永远不完成，退出时报「A Timer is still pending」（2026-11-05 撞到）。
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);
  });

  tearDownAll(restoreRealTransport);

  testWidgets('关于页有赞赏卡片和按钮', (tester) async {
    await pumpAbout(tester);
    expect(find.text('如果它帮到你了'), findsOneWidget);
    expect(find.text('查看赞赏码'), findsOneWidget);
  });

  testWidgets('点按钮弹出赞赏码，关掉能收回去', (tester) async {
    await pumpAbout(tester);

    await tester.tap(find.text('查看赞赏码'));
    await tester.pumpAndSettle();

    expect(find.text('赞赏支持'), findsOneWidget);
    expect(find.textContaining('微信扫一扫'), findsOneWidget);
    expect(find.textContaining('软件永远免费'), findsOneWidget);

    await tester.tap(find.text('关闭'));
    await tester.pumpAndSettle();
    expect(find.text('赞赏支持'), findsNothing, reason: '关掉后弹窗必须消失');
  });

  testWidgets('赞赏码图片真的打进包了（不是空占位）', (tester) async {
    final data = await rootBundle.load('assets/donate/wechat.png');
    expect(
      data.lengthInBytes,
      greaterThan(10000),
      reason: '赞赏码图小于 10KB，多半是占位图或压根没打进包',
    );
    final head = data.buffer.asUint8List(0, 4);
    expect(head, [0x89, 0x50, 0x4E, 0x47], reason: '文件头不是 PNG 魔数');
  });
}
