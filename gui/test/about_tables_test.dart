// 「支持的格式」表与「快捷键」表 —— 2026-11-05 从设置页搬到关于页之后的回归。
//
// 起因（用户原话）：「设置里面一大段话的那些能不能移动到关于里面，因为它们也没啥设置」。
// 搬的是两张**只读**表：格式能力表（问引擎 hello.formats 拿）和快捷键表
// （行来自 lib/keyboard.dart 的 keyHelpRows）。
//
// 这个文件钉四件事：
//   ① 关于页一进去就有格式表，且没有「读取各格式能力」按钮（T17-1 的老要求）
//   ② 引擎太旧 / 拿不到 formats → 如实说读不到，并给「重试」（不许假装是空表）
//   ③ 关于页能看到完整快捷键表（6 行 + 那句「键位固定」说明）
//   ④ **设置页里已经没有了** —— 反向验证，防止哪天又被搬回去、两处各画一份
//
// 原来在 settings_perf_widget_test.dart（①②）和 keys_help_test.dart（③）里，
// 搬表时一起转移过来；那两个文件只留设置页/主按钮自己的断言。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/keyboard.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/desktop/pages/about_page.dart';
import 'package:yshjy_gui/desktop/pages/settings_page.dart';

import 'fake_engine.dart';

Future<void> _pumpAbout(WidgetTester tester) async {
  // 关于页比原来长了两张表，视口给足（ListView 只构建可视区）
  tester.view.physicalSize = const Size(1200, 5200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  bool ready() =>
      find.text('zip').evaluate().isNotEmpty ||
      find.text('重试').evaluate().isNotEmpty;

  await tester.runAsync(() async {
    await tester.pumpWidget(
      const MaterialApp(home: Scaffold(body: AboutPage(engineVersion: 'test'))),
    );
    for (var i = 0; i < 120 && !ready(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
  });
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 50));
}

void main() {
  FakeEngine? fake;

  setUp(() async {
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);
  });

  tearDownAll(restoreRealTransport);

  group('格式表（搬来关于页）', () {
    testWidgets('进关于页就出表格，且没有「读取各格式能力」按钮', (tester) async {
      await _pumpAbout(tester);
      // fake 的 hello 给了两份能力行
      expect(find.text('zip'), findsOneWidget, reason: '格式表应当自动显示（不用点按钮）');
      expect(find.text('7z'), findsOneWidget);
      expect(find.text('读取各格式能力'), findsNothing,
          reason: 'T17-1 要求去掉那个按钮');
      expect(find.text('支持的格式'), findsOneWidget, reason: '表格要有标题');
    });

    testWidgets('引擎报错时如实说，并给一个「重试」', (tester) async {
      fake!.helloResult = {'engine': 'test'}; // 没有 formats → 走「引擎太旧」分支
      await _pumpAbout(tester);
      expect(find.textContaining('这个引擎版本太旧'), findsOneWidget);
      expect(find.text('重试'), findsOneWidget);
    });
  });

  group('快捷键表（搬来关于页）', () {
    testWidgets('关于页能看到完整的按键表', (tester) async {
      await _pumpAbout(tester);

      expect(find.byKey(const ValueKey('keys-table')), findsOneWidget,
          reason: '关于页没有快捷键表 —— 用户根本不知道能用键盘');
      expect(find.text('快捷键'), findsOneWidget);
      for (final r in keyHelpRows) {
        expect(find.text(r.keys), findsWidgets, reason: '${r.keys} 这一行没显示出来');
      }
      expect(find.text(t('keys.note')), findsOneWidget);
    });
  });

  group('反向验证：设置页不该再有这两张表', () {
    testWidgets('设置页里既没有格式表也没有快捷键表', (tester) async {
      tester.view.physicalSize = const Size(1200, 5200);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);

      bool loaded() {
        final f = find.byKey(const ValueKey('settings-save'));
        if (f.evaluate().isEmpty) return false;
        return tester.widget<FloatingActionButton>(f).onPressed != null;
      }

      await tester.runAsync(() async {
        await tester.pumpWidget(
          const MaterialApp(home: Scaffold(body: SettingsPage())),
        );
        for (var i = 0; i < 120 && !loaded(); i++) {
          await Future<void>.delayed(const Duration(milliseconds: 25));
          await tester.pump();
        }
      });
      await tester.pump();

      expect(find.byKey(const ValueKey('keys-table')), findsNothing,
          reason: '快捷键表已经搬到关于页了，设置页不该再画一份'
              '（两份真源会漂：改一处忘另一处）');
      expect(find.text('支持的格式'), findsNothing,
          reason: '格式表已经搬到关于页了');
      // 但真正能改的东西必须还在
      expect(find.text('主题'), findsWidgets);
      expect(find.byKey(const ValueKey('settings-save')), findsOneWidget);
    });
  });
}
