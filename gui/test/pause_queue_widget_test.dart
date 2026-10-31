// T18-4 队列「全部暂停 / 继续」的界面回归。
//
// 为什么非要 widget 测试：
//  · 「按钮只该在批量时出现」是**反向断言** —— 单个文件解压时多出一个「全部暂停」，
//    用户点了引擎会说「没有任务在跑」，看起来就是坏按钮。手点验不出「不该出现的没出现」。
//  · 「已暂停」这一屏要等引擎发 `PAUSED` 事件才出现，测试里没有引擎，
//    所以用 `debugPaused` 直接注入那个状态。
//
// 协议那一层（真暂停、文件数不再增长、产物 sha256 一致）由探针
// `tests/t18_4_pause.py` 守着，这里只管界面：按钮在不在、字对不对。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_page.dart';

Future<void> pumpHome(WidgetTester tester, HomePage page) async {
  tester.view.physicalSize = const Size(1200, 2000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: Scaffold(body: page)));
  await tester.pump();
}

void main() {
  group('T18-4 队列「全部暂停」什么时候才该出现', () {
    testWidgets('批量在跑 → 有「全部暂停」按钮', (tester) async {
      await pumpHome(tester, const HomePage(autoStart: false, debugBatchCount: 3));
      expect(find.text('全部暂停'), findsOneWidget,
          reason: '批量跑起来却没暂停入口，这个功能等于没做');
      // 同一屏的既有按钮不能被挤掉（溢出 / 条件写反都会让它们消失）
      expect(find.text('跳过这项'), findsOneWidget);
      expect(find.text('取消整批'), findsOneWidget);
    });

    testWidgets('只解一个文件 → 一个暂停入口都不许有', (tester) async {
      await pumpHome(tester, const HomePage(autoStart: false, debugBatchCount: 1));
      expect(find.text('全部暂停'), findsNothing,
          reason: '单个文件暂停没意义：它本来就快跑完了，给了按钮反而像卡住');
      expect(find.text('继续'), findsNothing);
    });

    testWidgets('引擎已确认暂停 → 按钮变「继续」，并说明现在是什么状态', (tester) async {
      await pumpHome(tester,
          const HomePage(autoStart: false, debugBatchCount: 2, debugPaused: true));
      expect(find.text('继续'), findsOneWidget,
          reason: '暂停住了却没有「继续」，用户只能取消重来');
      expect(find.text('全部暂停'), findsNothing);
      expect(find.textContaining('已暂停'), findsOneWidget,
          reason: '不说一句「已暂停」，用户会以为卡死了');
    });

    testWidgets('首页空态不出现暂停按钮', (tester) async {
      await pumpHome(tester, const HomePage(autoStart: false));
      expect(find.text('全部暂停'), findsNothing);
      expect(find.text('继续'), findsNothing);
    });
  });
}
