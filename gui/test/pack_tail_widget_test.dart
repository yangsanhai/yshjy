// E2「不改封面，接在文件末尾」的回归（T7 第 4 项）
//
// 协议 `pack.tail` 引擎早实现了（api.rs: `tail:true` → `PackLayout::TailAppend`），
// 界面却只**显示**结果（「free 原子布局 / 尾部追加布局」）、从来不传 ——
// 想强制尾部追加的人只能在命令行加 `--tail`。
//
// 三条铁律：
//   ① 默认**关**、且请求里**不出现** `tail` 键：默认要交给引擎按封面文件头判断
//      （MP4 塞 free 原子视频还能播，图片只能追加）。默认传 true 会把所有 MP4 包
//      都变成「播不了的视频」—— 这是本项最大的坑；
//   ② 勾上之后请求里必须是 `tail: true`（开关是假的就没意义）；
//   ③ 普通压缩 / 自解压没有封面 → 禁用并说明，不做点了没反应的开关。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/notify.dart';
import 'package:yshjy_gui/pages/pack_page.dart';
import 'package:yshjy_gui/transport/engine_transport.dart';

const tail = ValueKey('tail-toggle');

Future<void> pumpPack(WidgetTester tester, {bool ready = false}) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: ready
          ? const PackPage(
              debugInputs: ['/tmp/内容'],
              debugOut: '/tmp/成品.mp4',
              debugCover: '/tmp/封面.mp4',
            )
          : const PackPage(),
    ),
  ));
  await tester.pumpAndSettle();
}

Future<void> pumpFrames(WidgetTester tester) async {
  for (var i = 0; i < 25; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

bool switchEnabled(WidgetTester tester, Key key) =>
    tester.widget<SwitchListTile>(find.byKey(key)).onChanged != null;

bool switchValue(WidgetTester tester, Key key) =>
    tester.widget<SwitchListTile>(find.byKey(key)).value;

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

  testWidgets('默认普通压缩 → 禁用，并说明「没有封面，不涉及布局」', (tester) async {
    await pumpPack(tester);

    expect(find.byKey(tail), findsOneWidget);
    expect(switchEnabled(tester, tail), isFalse,
        reason: '普通 ZIP 没有封面，布局选项无意义，能点就是假开关');
    final title =
        (tester.widget<SwitchListTile>(find.byKey(tail)).title as Text).data ?? '';
    expect(title, contains('只有伪装成文件时可用'),
        reason: '禁用时必须写明原因，不能让用户以为是坏了');
  });

  testWidgets('伪装成文件 → 可用，默认关，副标题说「默认让封面装数据」', (tester) async {
    await pumpPack(tester);

    await tester.tap(find.text('伪装成文件'));
    await tester.pumpAndSettle();

    expect(switchEnabled(tester, tail), isTrue);
    expect(switchValue(tester, tail), isFalse,
        reason: '默认必须交给引擎按封面类型判断，不能替用户做决定');
    expect(find.textContaining('默认让封面的视频结构装数据'), findsOneWidget);
  });

  testWidgets('不勾 → 请求里不许出现 tail（否则 MP4 包会变成播不了的视频）',
      (tester) async {
    await pumpPack(tester, ready: true);
    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!.containsKey('tail'), isFalse,
        reason: '默认传 tail 会把所有 MP4 伪装包都改成尾部追加（视频播不了）');
  });

  testWidgets('勾上 → 副标题变「一个字节都不动」；请求带 tail: true', (tester) async {
    await pumpPack(tester, ready: true);

    await tester.tap(find.byKey(tail));
    await tester.pumpAndSettle();

    expect(switchValue(tester, tail), isTrue);
    expect(find.textContaining('封面文件一个字节都不动'), findsOneWidget);

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!['tail'], isTrue, reason: '界面勾了却没传给引擎 —— 用户以为改了、其实没改');
  });
}

/// 假引擎：记下收到的命令，按命令回一条形状正确的结果
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
      case 'pack':
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {
            'out': '/tmp/成品.mp4',
            'bytes': 123456,
            'layout': 'TailAppend',
            'decoy_segments': 0,
            'millis': 12,
            'warnings': <dynamic>[],
          },
        }));
    }
  }
}
