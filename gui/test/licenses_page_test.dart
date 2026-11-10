// 「开源许可」的回归（2026-11-05 加：用户说「许可必须做的」）。
//
// 为什么必须钉住：
//   MIT / BSD / Apache-2.0 都要求分发二进制时**保留版权声明与许可文本**。
//   清单要是空的、或是几个月前的旧货，页面看着正常，其实已经违规了 ——
//   这种「静默失效」只能靠测试拦。
//
// 钉四件事：
//   ① 关于页有两个入口（开源库清单 / 完整许可证文本）
//   ② 点「查看开源库清单」能推出清单页
//   ③ 清单页能把 json 渲染出来：计数、库名、许可证、关键库都在
//   ④ 搜索能过滤、能复原
//
// ⚠️ ③④ 走**注入 loader**（读仓库里那个真实文件），不走 rootBundle：
//    `rootBundle` 在 widget 测试里跨用例不稳定 —— 同一次跑里第一个用例读得到，
//    后面几个一直停在加载中（2026-11-05 实测，见 lib/pages/licenses_page.dart 的注释）。
//    注入之后测的还是同一份数据（gui/assets/third_party.json），只是绕开了平台通道。

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/desktop/pages/about_page.dart';
import 'package:yshjy_gui/desktop/pages/licenses_page.dart';

import 'fake_engine.dart';

/// 仓库里的真实清单文件（测试的工作目录是 gui/）
String _realJson() => File('assets/third_party.json').readAsStringSync();

Future<void> _pumpAbout(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1000, 6000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  await tester.runAsync(() async {
    await tester.pumpWidget(
      const MaterialApp(home: Scaffold(body: AboutPage(engineVersion: 'test'))),
    );
    // 关于页内嵌格式表，要问引擎；等它出结果
    for (var i = 0; i < 120; i++) {
      if (find.text('zip').evaluate().isNotEmpty ||
          find.text('重试').evaluate().isNotEmpty) {
        break;
      }
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
  });
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 50));
}

Future<void> _pumpLicenses(WidgetTester tester, {String? json}) async {
  tester.view.physicalSize = const Size(1000, 3000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final data = json ?? _realJson();
  await tester.pumpWidget(
    MaterialApp(home: LicensesPage(loader: () async => data)),
  );
  await tester.pump(); // 让 _load 的 microtask 落地
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

  testWidgets('关于页有「开源许可」卡片和两个入口', (tester) async {
    await _pumpAbout(tester);

    expect(find.text(t('about.license.title')), findsOneWidget);
    expect(find.text(t('about.license.deps')), findsOneWidget);
    expect(find.text(t('about.license.full')), findsOneWidget,
        reason: '「查看完整许可证文本」是 pub 包那一半的唯一入口，不能少');
  });

  testWidgets('点「查看开源库清单」能推出清单页', (tester) async {
    await _pumpAbout(tester);

    await tester.tap(find.text(t('about.license.deps')));
    await tester.pumpAndSettle();

    expect(find.byType(LicensesPage), findsOneWidget, reason: '按钮没把清单页推出来');
  });

  testWidgets('清单页把 json 渲染出来了（计数 / 库名 / 许可证）', (tester) async {
    await _pumpLicenses(tester);

    expect(find.text(t('licenses.app')), findsOneWidget);
    expect(find.text('yshjy · MIT'), findsOneWidget, reason: '本软件的许可证要写出来');
    // 标题里的计数来自 json 的 _count（当前 190）
    expect(find.textContaining('190'), findsWidgets);
    // 排序后第一条是 adler2（能出现说明列表真渲染了）
    expect(find.textContaining('adler2'), findsWidgets);
    expect(find.textContaining('MIT'), findsWidgets, reason: '许可证文本要显示，不能只有库名');
  });

  testWidgets('清单里真有在用的关键库（json 不是空的/过期的）', (tester) async {
    await _pumpLicenses(tester);

    // ⚠️ 190 个库在 ListView 里，靠后的不会被构建（虚拟化），
    //    所以用**搜索框**定位，而不是指望它出现在首屏。
    for (final name in ['rars', 'sevenz-rust2', 'zip']) {
      await tester.enterText(find.byType(TextField), name);
      await tester.pump();
      expect(find.textContaining(name), findsWidgets,
          reason: '$name 不在清单里 —— third_party.json 可能没重新生成'
              '（跑 python3 scripts/gen-third-party.py）');
    }
  });

  testWidgets('搜索能过滤，清空能复原', (tester) async {
    await _pumpLicenses(tester);

    await tester.enterText(find.byType(TextField), 'sevenz');
    await tester.pump();
    expect(find.textContaining('sevenz-rust2'), findsWidgets, reason: '搜索该命中');
    expect(find.textContaining('adler2'), findsNothing, reason: '不该把别的库也留下');

    await tester.enterText(find.byType(TextField), 'zzz-not-exist');
    await tester.pump();
    expect(find.text(t('licenses.empty')), findsOneWidget,
        reason: '没有匹配时要说「没有匹配的库」，不能白屏');

    await tester.enterText(find.byType(TextField), '');
    await tester.pump();
    expect(find.textContaining('adler2'), findsWidgets, reason: '清空后要复原全量列表');
  });

  testWidgets('数据是坏的时候如实说读不到（不白屏、不假装空表）', (tester) async {
    await _pumpLicenses(tester, json: '这不是 json');
    expect(find.textContaining(t('licenses.loadFailed', {'reason': ''})), findsOneWidget,
        reason: '解析失败要说清，不能显示成「0 个库」');
  });
}
