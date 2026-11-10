// E6「诊断包存到哪」的回归（T7 第 5 项）
//
// 病根：协议 `report` 的 `out_dir` 引擎早就支持，界面却只传 `path`。
// 引擎不传 `out_dir` 时落在**进程当前目录** —— GUI 从哪启动就落在哪（可能是 `/` 或 `~`），
// 用户点完看到「诊断包已生成」，回头满电脑找不着文件。比报错还气人。
//
// 两条要钉的：
//   ① 请求里必须有 `out_dir`，且是「下载」这种用户一眼能找到的地方；
//   ② 界面要**明确告诉他存哪了**（不能只丢一个路径字符串了事）。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';
import 'package:yshjy_gui/common/platform_paths.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

Future<void> pumpError(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(const MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugErrorCode: 'NOT_ARCHIVE',
        debugErrorNote: '这个文件认不出来，可能不是压缩包',
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

Future<void> pumpFrames(WidgetTester tester) async {
  for (var i = 0; i < 25; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

void main() {
  _FakeEngine? fake;

  setUp(() async {
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = _FakeEngine();
    EngineClient.transportFactory = (_, __) => fake!;
    Notifier.enabled = false;
  });

  tearDownAll(() =>
      EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args));

  testWidgets('错误页点「生成诊断包」→ 请求带 out_dir，落在好找的目录里', (tester) async {
    await pumpError(tester);
    expect(find.textContaining('生成诊断包'), findsOneWidget,
        reason: '错误页必须有这个按钮，否则用户报障时无从下手');

    await tester.tap(find.textContaining('生成诊断包'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('report');
    expect(cmd, isNotNull, reason: '点了按钮必须真发一条 report');
    expect(cmd!['out_dir'], isNotNull,
        reason: '不给 out_dir 引擎就落在进程当前目录（GUI 从哪启动就在哪），用户根本找不到');
    expect(cmd['out_dir'], downloadsDir,
        reason: '默认落「下载」——用户找得到的地方');
    expect(cmd['no_paths'], isTrue, reason: '报障包默认必须脱敏');
  });

  testWidgets('生成完要说清「存到哪了」，不能只丢一串路径', (tester) async {
    await pumpError(tester);
    await tester.tap(find.textContaining('生成诊断包'));
    await pumpFrames(tester);

    final text = tester.widget<Text>(find.textContaining('诊断包已存到')).data ?? '';
    expect(text.contains('下载') || text.contains('Downloads') || text.contains('~'),
        isTrue,
        reason: '要用人话告诉他去哪个文件夹找，实际文案：「$text」');
    expect(find.textContaining('把它发给我'), findsOneWidget,
        reason: '下一步要交代清楚（发给开发者）');
  });
}

class _FakeEngine implements EngineTransport {
  final _out = StreamController<String>.broadcast();
  final _err = StreamController<String>.broadcast();
  final List<Map<String, dynamic>> sent = [];

  @override
  Future<void> start() async {}

  @override
  Stream<String> get stdoutLines => _out.stream;

  @override
  Stream<String> get stderrLines => _err.stream;

  @override
  Future<int> get exitCode async => 0;

  @override
  Future<void> closeInput() async {}

  @override
  void kill() {}

  void dispose() {
    _out.close();
    _err.close();
  }

  Map<String, dynamic>? lastCmd(String name) {
    for (final m in sent.reversed) {
      if (m['cmd'] == name) return m;
    }
    return null;
  }

  @override
  Future<void> writeLine(String line) async {
    final m = (jsonDecode(line) as Map).cast<String, dynamic>();
    sent.add(m);
    switch (m['cmd']) {
      case 'hello':
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {'engine': 'test', 'protocol': 1},
        }));
      case 'report':
        final dir = (m['out_dir'] as String?) ?? '/tmp';
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {'path': '$dir/yshjy-report-20261031.zip'},
        }));
    }
  }
}
