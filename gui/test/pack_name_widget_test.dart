// T18-2「自定义显示名」的界面回归
//
// 设计稿（DESIGN §14.2）：伪装成 `电影.2026.1080p.mp4` 这种，**用户填**。
// 以前输出名是一段只读文字（注释写着「不用 TextField：每帧 new controller 会丢状态」），
// 现在换成 State 里的持久 controller + 直接编辑文件名。
//
// 四条断言：
//   ① 输入框里放的是**文件名**（不是完整路径）；
//   ② 改成 `电影.2026.1080p` → 请求的 out 是 `/tmp/电影.2026.1080p.mp4`
//      （reExt 追加后缀，不能把 1080p 当扩展名吃掉 —— 那是本项实测撞到的形状）；
//   ③ 非法名（带 /）→ 输入框红字 + **不发 pack 命令**（引擎那头 out 是路径，
//      别让「改名」静默变成「换位置」）；
//   ④ 空名字 → 同样的拦截。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/pack_page.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

const nameField = ValueKey('output-name');

Future<void> pumpReady(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(const MaterialApp(
    home: Scaffold(
      body: PackPage(
        debugInputs: ['/tmp/内容'],
        debugOut: '/tmp/成品.mp4',
        debugCover: '/tmp/封面.mp4',
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

/// 真文件 I/O 的路径不能用 pumpAndSettle（假时钟下 Future 永不完成）—— 定次 pump 推完就算
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

  testWidgets('输出名是个可编辑的输入框，里面只有文件名', (tester) async {
    await pumpReady(tester);

    expect(find.byKey(nameField), findsOneWidget,
        reason: '显示名要能改，不能还是只读文字');
    final field = tester.widget<TextField>(find.byKey(nameField));
    expect(field.controller?.text, '成品.mp4',
        reason: '输入框里该是文件名，不是 /tmp/成品.mp4 整条路径');
  });

  testWidgets('改成 电影.2026.1080p → 请求 out 补成 .mp4 而不是吃掉 1080p',
      (tester) async {
    await pumpReady(tester);

    await tester.enterText(find.byKey(nameField), '电影.2026.1080p');
    await tester.pumpAndSettle();
    expect(find.textContaining('不能用'), findsNothing, reason: '这个名字是合法的');

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull, reason: '合法名字必须能发出去');
    expect(cmd!['out'], '/tmp/电影.2026.1080p.mp4',
        reason: 'reExt 要追加后缀；用 withExt 会把 1080p 当扩展名换成 mp4');
  });

  testWidgets('名字里带 / → 红字，且一条 pack 命令都不发', (tester) async {
    await pumpReady(tester);

    await tester.enterText(find.byKey(nameField), 'a/b.mp4');
    await tester.pumpAndSettle();
    expect(find.textContaining('这个名字不能用'), findsOneWidget,
        reason: '非法名字要即时红字，不能等打包时才炸');

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    expect(fake!.lastCmd('pack'), isNull,
        reason: '名字里的路径分隔符会被引擎当成位置 —— 必须拦在界面，不许发出去');
  });

  testWidgets('名字清空 → 红字提示，不发命令', (tester) async {
    await pumpReady(tester);

    await tester.enterText(find.byKey(nameField), '');
    await tester.pumpAndSettle();
    expect(find.textContaining('先给产物起个名字'), findsOneWidget);

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    expect(fake!.lastCmd('pack'), isNull, reason: '空名字打包出来没法找');
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
            'out': m['out'],
            'bytes': 123456,
            'layout': 'FreeAtom',
            'decoy_segments': 0,
            'millis': 12,
            'warnings': <dynamic>[],
          },
        }));
    }
  }
}
