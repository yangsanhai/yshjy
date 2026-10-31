// T18-5「历史页排序」的回归
//
// UI-SPEC 4b.2 画了「排序：时间 ▾」，但历史页一直固定顺序（`sort` 零命中）。
// 现在加了「时间 / 名称 / 大小」+ 一个方向切换按钮。
//
// 这里钉四件事：
//   ① 默认还是「时间，新的在前」—— 和存储顺序完全一致，老用户看不出变化
//   ② 切「大小」→ 大的在前；点方向按钮 → 反过来
//   ③ 切「名称」→ A→Z（**不承诺拼音序**：中文按码点排，界面上也没这么承诺）
//   ④ 排序只影响显示，不动存储文件（切回去还能按原顺序）
//
// ⚠️ 会真写 `stateDir/gui-history.json`，必须隔离（all.sh 已设 XDG_STATE_HOME）：
//   XDG_STATE_HOME=/tmp/yshjy-test-state flutter test

import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/history_page.dart';
import 'package:yshjy_gui/platform_paths.dart';

import 'config_guard.dart';

String get _histPath => '$stateDir/gui-history.json';

/// 三条记录：时间 a < b < c，大小 b(300) > c(200) > a(100)，名字 a < b < c，
/// 三个维度故意排成不同的顺序，这样"按什么排"一眼能验出来。
const _entries = [
  {
    'time': '2026-01-01T10:00:00+08:00',
    'input': '/tmp/a.txt',
    'dest': null,
    'ok': true,
    'bytes': 100,
  },
  {
    'time': '2026-01-02T10:00:00+08:00',
    'input': '/tmp/b.txt',
    'dest': null,
    'ok': true,
    'bytes': 300,
  },
  {
    'time': '2026-01-03T10:00:00+08:00',
    'input': '/tmp/c.txt',
    'dest': null,
    'ok': false,
    'bytes': 200,
  },
];

void _writeHistory() {
  File(_histPath)
    ..createSync(recursive: true)
    ..writeAsStringSync(jsonEncode(_entries));
}

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1000, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  // 读历史是真 I/O → 必须 runAsync（假时钟里 File 的 Future 永远不完成）。
  // 等「列表真的出条目」而不是睡固定时长（并发跑时固定 sleep 会偶发红）。
  bool ready() => find.byType(ListTile).evaluate().isNotEmpty;

  await tester.runAsync(() async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: HistoryPage())));
    for (var i = 0; i < 120 && !ready(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
  });
  await tester.pump();
}

/// 列表里从上到下的文件名
List<String> _names(WidgetTester tester) => tester
    .widgetList<ListTile>(find.byType(ListTile))
    .map((w) => (w.title as Text?)?.data ?? '')
    .toList();

Future<void> _pickSort(WidgetTester tester, String label) async {
  await tester.tap(find.byKey(const ValueKey('history-sort')));
  await tester.pumpAndSettle();
  await tester.tap(find.text(label).last);
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(() => requireIsolatedStateDir('历史页排序测试'));

  setUp(() {
    final f = File(_histPath);
    if (f.existsSync()) f.deleteSync();
  });

  tearDownAll(() {
    final f = File(_histPath);
    if (f.existsSync()) f.deleteSync();
  });

  testWidgets('默认按时间：最新的在最上面（和存储顺序一致）', (tester) async {
    _writeHistory();
    await _pump(tester);
    expect(_names(tester), ['c.txt', 'b.txt', 'a.txt']);
    // 默认就是「时间」这一档
    expect(find.text('时间'), findsOneWidget);
  });

  testWidgets('切「大小」→ 大的在前；再点方向按钮 → 反过来', (tester) async {
    _writeHistory();
    await _pump(tester);

    await _pickSort(tester, '大小');
    expect(_names(tester), ['b.txt', 'c.txt', 'a.txt'],
        reason: '300 > 200 > 100');

    await tester.tap(find.byKey(const ValueKey('history-sort-dir')));
    await tester.pumpAndSettle();
    expect(_names(tester), ['a.txt', 'c.txt', 'b.txt'],
        reason: '方向按钮要真的翻过来');
  });

  testWidgets('切「名称」→ A→Z（换字段时方向自动复位）', (tester) async {
    _writeHistory();
    await _pump(tester);

    await _pickSort(tester, '名称');
    expect(_names(tester), ['a.txt', 'b.txt', 'c.txt'],
        reason: '名称默认升序（时间/大小才是降序），换字段要复位方向');
  });

  testWidgets('排序只影响显示：存储文件一个字节都不改', (tester) async {
    _writeHistory();
    final before = File(_histPath).readAsStringSync();
    await _pump(tester);

    await _pickSort(tester, '大小');
    await tester.tap(find.byKey(const ValueKey('history-sort-dir')));
    await tester.pumpAndSettle();

    expect(File(_histPath).readAsStringSync(), before,
        reason: '排序是纯视图层的，不该回写历史文件');
  });
}
