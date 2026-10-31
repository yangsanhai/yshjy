// T17-4「打包格式可选」的回归
//
// 引擎侧的能力（zip / tar.gz / tar.zst）由 tests/format_proto.py 真产物验，
// 这里只盯**界面该做、做错了用户会踩的三件事**：
//   ① 默认是 zip，普通压缩下格式下拉在、密码框能用（老行为不变）；
//   ② 选 tar.zst 之后：密码框置灰 + 写明原因、分卷开关禁用、输出名换成 .tar.zst，
//      点「开始压缩」时请求里 format 必须是 tar.gz 且**不出现** password / split
//      （用户在 zip 下填过的密码不能偷偷带到 tar 包里）；
//   ③ 伪装模式不给格式下拉 —— 伪装只能是 zip（引擎会拒，界面别让人选出错）。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/notify.dart';
import 'package:yshjy_gui/pages/pack_page.dart';
import 'package:yshjy_gui/transport/engine_transport.dart';

/// 格式下拉的 key（与 pack_page.dart 里 `ValueKey('pack-format')` 对齐）
const kFormatKey = ValueKey('pack-format');

/// 密码框的标签前缀（l10n zh-CN 的 pack.password）
const kPasswordLabel = '解压码';

Future<void> pumpPack(WidgetTester tester, {bool ready = false}) async {
  tester.view.physicalSize = const Size(1100, 1900);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: ready
          ? const PackPage(
              debugInputs: ['/tmp/内容'],
              debugOut: '/tmp/成品.zip',
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

/// 页面上那个「解压码」输入框现在能不能输入
bool passwordEnabled(WidgetTester tester) {
  final boxes = tester
      .widgetList<TextField>(find.byType(TextField))
      .where((w) => w.decoration?.labelText?.contains(kPasswordLabel) == true)
      .toList();
  expect(boxes, isNotEmpty, reason: '页面上应该有解压码输入框');
  // TextField.enabled == null 表示「跟随主题」= 可用
  return boxes.first.enabled ?? true;
}

/// 分卷开关（tar 下必须置灰）—— 页面上还有校验和 / 说明书等别的开关，
/// 所以按 key 精确找分卷那个，别一刀切「一个可点的都不能有」
bool splitEnabled(WidgetTester tester) {
  final f = find.byKey(const ValueKey('split-toggle'));
  expect(f, findsOneWidget, reason: '打包页应该有分卷开关');
  return tester.widget<SwitchListTile>(f).onChanged != null;
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

  testWidgets('默认普通压缩：格式下拉在、值是 zip、密码框能用', (tester) async {
    await pumpPack(tester, ready: true);

    expect(find.byKey(kFormatKey), findsOneWidget, reason: '普通压缩模式下要有「打包格式」下拉');
    expect(tester.widget<DropdownButton<String>>(find.byKey(kFormatKey)).value,
        'zip');
    expect(passwordEnabled(tester), isTrue, reason: 'zip 支持加密，密码框必须能用');
  });

  testWidgets('选 tar.zst：密码框置灰 + 写明原因、开关禁用、输出名跟着换', (tester) async {
    await pumpPack(tester, ready: true);

    await tester.tap(find.byKey(kFormatKey));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('tar.zst').last);
    await tester.pumpAndSettle();

    expect(
        tester.widget<DropdownButton<String>>(find.byKey(kFormatKey)).value,
        'tar.zst');
    expect(passwordEnabled(tester), isFalse,
        reason: 'tar 不支持加密：不能让人输了密码才发现没生效');
    expect(find.textContaining('tar 格式不支持加密'), findsOneWidget);
    expect(find.textContaining('.tar.zst'), findsWidgets,
        reason: '输出名必须跟着格式换，不然存下来是个后缀不符的文件');
    expect(splitEnabled(tester), isFalse,
        reason: 'tar 不支持分卷，开关要禁用');
  });

  testWidgets('选 tar.gz 后点「开始压缩」：请求带 format，且不带 password / split',
      (tester) async {
    await pumpPack(tester, ready: true);

    // 先在 zip 下填一个密码（模拟用户的真实动作顺序）
    final pw = tester
        .widgetList<TextField>(find.byType(TextField))
        .firstWhere((w) => w.decoration?.labelText?.contains(kPasswordLabel) == true);
    await tester.enterText(find.byWidget(pw), 'old-password');
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(kFormatKey));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('tar.gz').last);
    await tester.pumpAndSettle();

    await tester.tap(find.text('开始压缩'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!['format'], 'tar.gz', reason: '选了 tar.gz 却没传给引擎');
    expect(cmd['mode'], 'plain');
    expect(cmd.containsKey('password'), isFalse,
        reason: 'zip 下填过的密码不能偷偷带进 tar 包（引擎也会拒）');
    expect(cmd.containsKey('split'), isFalse);
  });

  testWidgets('伪装模式：没有格式下拉（伪装只能是 zip）', (tester) async {
    await pumpPack(tester, ready: true);

    await tester.tap(find.textContaining('伪装成').first);
    await tester.pumpAndSettle();

    expect(find.byKey(kFormatKey), findsNothing,
        reason: '伪装包的定位靠 ZIP 的 EOCD，tar 系打出来自己也解不开');
  });

  // ── T18-3 防炸链 ────────────────────────────────────────────────
  const tweak = ValueKey('tweak-time-toggle');

  testWidgets('防炸链默认关：请求里不出现 tweak_time', (tester) async {
    await pumpPack(tester, ready: true);

    expect(tester.widget<SwitchListTile>(find.byKey(tweak)).value, isFalse,
        reason: '默认必须关 —— 自己备份时真实时间戳是有用的，别默默改掉');

    await tester.tap(find.text('开始压缩'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!.containsKey('tweak_time'), isFalse,
        reason: '没开就不该传，传了会让「两次打包一致」这条约定莫名其妙地失效');
  });

  testWidgets('勾上防炸链：请求带 tweak_time: true', (tester) async {
    await pumpPack(tester, ready: true);

    await tester.tap(find.byKey(tweak));
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(find.byKey(tweak)).value, isTrue);

    await tester.tap(find.text('开始压缩'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!['tweak_time'], isTrue,
        reason: '开关是假的就没意义 —— 用户勾了，引擎必须收到');
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
      case 'pack':
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {
            'out': '/tmp/成品.tar.gz',
            'format': m['format'] ?? 'zip',
            'total_size': 123456,
            'entries': 3,
            'layout': 'TailAppend',
            'noise_segments': 0,
            'millis': 12,
            'warnings': <dynamic>[],
          },
        }));
    }
  }
}
