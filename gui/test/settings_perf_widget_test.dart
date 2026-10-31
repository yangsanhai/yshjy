// T17-2（并行任务数三档 / 可执行提醒开关 / 本机磁盘建议）+ T17-1（格式表自动显示）的回归
//
// 为什么非要有（每条都对应一个"看起来对、其实没用"的坑）：
//   ① `jobs` 必须写成**整数**（`jobs = 2`）。写成 `jobs = "2"` 时引擎
//      `as_integer()` 读不到 → 静默用默认 1 → 用户在界面选了 4 核并行，
//      实际上还是一个一个解。这是本页最经典的「假开关」。
//   ② 配置里手写成三档以外的值（比如 3）不能让下拉崩 —— Flutter 的
//      DropdownButtonFormField 遇到「value 不在 items 里」会直接 assert 失败。
//   ③ 「本机磁盘：机械硬盘」这句必须来自引擎真探测（`hello.disk`），
//      不是写死的文案。所以这里塞一个 fake 引擎，反过来验证"引擎说是固态，
//      界面就说固态"。
//   ④ 格式表改成进页面就显示（T17-1）—— 不能再出现「读取各格式能力」这个按钮。
//
// 注意：本文件会真写 config.toml，必须隔离（all.sh 已经设 XDG_CONFIG_HOME）：
//   XDG_CONFIG_HOME=/tmp/yshjy-test-config flutter test

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/settings_page.dart';
import 'package:yshjy_gui/platform_paths.dart';

import 'config_guard.dart';
import 'fake_engine.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  // 页面比 T17 之前长了一截（多了一个三档下拉 + 磁盘建议 + 一个开关），
  // 视口给足 —— ListView 只构建可视区，太矮会让下面的控件"找不到"。
  tester.view.physicalSize = const Size(1200, 4200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  // ⚠️ 这里**轮询**，不睡固定时长。
  // 第一版写的是「runAsync 里等 300ms」—— 全量跑（8 个文件并发）时机器很忙，
  // 300ms 读不完配置，于是「jobs = 4」那条偶发变红，单跑又绿（最费时间的那种假红）。
  // 结束条件：格式表出结果（成功 → 出现 zip；失败 → 出现「重试」）。
  bool ready() =>
      find.text('zip').evaluate().isNotEmpty ||
      find.text('重试').evaluate().isNotEmpty;

  await tester.runAsync(() async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: SettingsPage())));
    for (var i = 0; i < 120 && !ready(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
  });
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 50));
}

void _writeConfig(String text) {
  File(_cfgPath)
    ..createSync(recursive: true)
    ..writeAsStringSync(text);
}

/// 并行数下拉当前选中的那一档（返回 '1' / '2' / '4'）
String _jobsValue(WidgetTester tester) {
  final f = find.byKey(const ValueKey('jobs-select'));
  expect(f, findsOneWidget, reason: '设置页应该有「并行任务数」下拉');
  return tester.widget<DropdownButtonFormField<String>>(f).initialValue!;
}

/// 可执行提醒开关
bool _warnValue(WidgetTester tester) {
  final f = find.byKey(const ValueKey('warn-exec-toggle'));
  expect(f, findsOneWidget, reason: '设置页应该有「解出可执行文件时提醒」开关');
  return tester.widget<SwitchListTile>(f).value;
}

Future<void> _tapSave(WidgetTester tester) async {
  await tester.runAsync(() async {
    await tester.tap(find.widgetWithText(FilledButton, '保存'));
    await Future<void>.delayed(const Duration(milliseconds: 200));
  });
  await tester.pump();
}

void main() {
  FakeEngine? fake;

  setUpAll(() => requireIsolatedConfigDir('设置页性能段测试'));

  setUp(() async {
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);
    final f = File(_cfgPath);
    if (f.existsSync()) f.deleteSync();
  });

  tearDownAll(restoreRealTransport);

  group('并行任务数（T17-2）', () {
    testWidgets('没有配置文件：默认是 1（设计口径不动）', (tester) async {
      await _pump(tester);
      expect(_jobsValue(tester), '1');
    });

    testWidgets('配置写 jobs = 4 → 下拉显示 4', (tester) async {
      _writeConfig('[general]\njobs = 4\n');
      await _pump(tester);
      expect(_jobsValue(tester), '4');
    });

    testWidgets('配置手写成 3（不在三档里）→ 夹到 2，界面不能崩', (tester) async {
      _writeConfig('[general]\njobs = 3\n');
      await _pump(tester);
      expect(_jobsValue(tester), '2',
          reason: '三档只有 1/2/4，3 必须被夹到最近的一档（否则下拉 assert 崩）');
    });

    testWidgets('配置手写成 16 → 夹到 4', (tester) async {
      _writeConfig('[general]\njobs = 16\n');
      await _pump(tester);
      expect(_jobsValue(tester), '4');
    });

    testWidgets('选 2 保存 → 文件里写的是整数 jobs = 2（不是字符串）', (tester) async {
      _writeConfig('# 我自己的注释\n[general]\nclose_action = "tray"\n');
      await _pump(tester);

      // 打开下拉选 2
      await tester.tap(find.byKey(const ValueKey('jobs-select')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('2').last);
      await tester.pumpAndSettle();

      await _tapSave(tester);

      final text = File(_cfgPath).readAsStringSync();
      expect(text, contains('jobs = 2'),
          reason: '引擎用 as_integer() 读 jobs，写成 "2" 会被静默忽略（假开关）');
      expect(text, isNot(contains('jobs = "2"')));
      // 用户手写的注释与同段其它键不能被冲掉
      expect(text, contains('# 我自己的注释'));
      expect(text, contains('close_action = "tray"'));
    });
  });

  group('可执行提醒开关（T17-2）', () {
    testWidgets('没有配置：默认开（与引擎 config.rs 一致）', (tester) async {
      await _pump(tester);
      expect(_warnValue(tester), isTrue);
    });

    testWidgets('配置写 "off" / false 两种写法都显示关', (tester) async {
      _writeConfig('[safety]\nwarn_executables = "off"\n');
      await _pump(tester);
      expect(_warnValue(tester), isFalse);

      _writeConfig('[safety]\nwarn_executables = false\n');
      await _pump(tester);
      expect(_warnValue(tester), isFalse);
    });

    testWidgets('关掉再保存 → 文件里是 warn_executables = "off"', (tester) async {
      _writeConfig('[safety]\nflush_movable = "on"\n');
      await _pump(tester);

      await tester.tap(find.byKey(const ValueKey('warn-exec-toggle')));
      await tester.pump();
      expect(_warnValue(tester), isFalse);

      await _tapSave(tester);

      final text = File(_cfgPath).readAsStringSync();
      expect(text, contains('warn_executables = "off"'));
      // 同段另一个开关不能被连累
      expect(text, contains('flush_movable = "on"'));
    });
  });

  group('本机磁盘建议（T17-2）', () {
    // ⚠️ 别用 textContaining('机械硬盘')：并行数下面那行常驻建议里也写着
    // 「机械硬盘、U 盘、网盘选 1…」，会一次命中两处（第一版就是这么红的）。
    // 这里用**整句精确匹配**，顺带把文案本身也钉住。
    testWidgets('引擎说机械盘 → 界面就写机械盘', (tester) async {
      fake!.helloResult = {...fake!.helloResult, 'disk': 'rotational'};
      await _pump(tester);
      expect(find.text('本机磁盘：机械硬盘 —— 建议保持 1'), findsOneWidget,
          reason: '这句话必须来自 hello.disk，不能是写死的文案');
    });

    testWidgets('引擎说固态 → 界面就写固态', (tester) async {
      fake!.helloResult = {...fake!.helloResult, 'disk': 'solid'};
      await _pump(tester);
      expect(find.text('本机磁盘：固态硬盘 —— 可以试试 2 或 4'), findsOneWidget);
    });

    testWidgets('引擎没给 disk（老引擎）→ 说没探到，绝不假装是固态', (tester) async {
      final h = {...fake!.helloResult};
      h.remove('disk');
      fake!.helloResult = h;
      await _pump(tester);
      expect(find.text('本机磁盘类型没探到 —— 保守起见保持 1'), findsOneWidget);
    });
  });

  group('格式表自动显示（T17-1）', () {
    testWidgets('进页面就出表格，且没有「读取各格式能力」按钮', (tester) async {
      await _pump(tester);
      // fake 的 hello 给了两份能力行
      expect(find.text('zip'), findsOneWidget, reason: '格式表应当自动显示（不用点按钮）');
      expect(find.text('7z'), findsOneWidget);
      expect(find.text('读取各格式能力'), findsNothing,
          reason: 'T17-1 要求去掉那个按钮');
    });

    testWidgets('引擎报错时如实说，并给一个「重试」', (tester) async {
      // 让 hello 抛错：传输层直接回一个错误信封
      fake!.helloResult = {'engine': 'test'}; // 没有 formats → 走「引擎太旧」分支
      await _pump(tester);
      expect(find.textContaining('这个引擎版本太旧'), findsOneWidget);
      expect(find.text('重试'), findsOneWidget);
    });
  });
}
