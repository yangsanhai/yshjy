// E1「防网盘扫描（撒假数据）」的回归（T7 第 2 项）
//
// 两半：① 开关的状态机；② 接线 —— 勾上之后协议里真的带 `decoy`。
//
// 为什么两半都要：这一项最容易做成假开关。开关能点、界面看着对，但
// `pack()` 一个参数都没传，用户以为撒了假数据、其实什么都没撒（网盘照样认出来）。
// 第 ② 半用 `debugInputs` / `debugCover` 直接进「已选好」状态，点一次「开始打包」，
// 由假引擎把发出去的命令原样记下来 —— 真机选文件要走系统对话框，widget 测试点不到
// 那个对话框，不这么测这条接线就没人守。
//
// 三条状态机铁律：
//   ① 默认「普通压缩」模式下它必须**禁用**：引擎里 decoy 与 mode:"plain" 互斥，
//      能点就是给用户一个必然失败的东西；
//   ② 切到「伪装成文件」才可用；
//   ③ 关总开关要把「假特征码」子开关一起清掉 —— 它只在总开关开着时才渲染，
//      留着 true 就是「用户看不见的地方还在撒假特征码」。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/notify.dart';
import 'package:yshjy_gui/pages/pack_page.dart';
import 'package:yshjy_gui/transport/engine_transport.dart';

const decoy = ValueKey('decoy-toggle');
const decoySig = ValueKey('decoy-sig-toggle');

Future<void> pumpPack(WidgetTester tester, {bool ready = false}) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: ready
          // 「已经选好」的状态：输入 + 输出 + 外壳（生产代码永远不传这三个）
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

/// 定次 pump：`pumpAndSettle` 在带真文件 I/O 的成功路径上会等到超时
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
  // 假引擎放局部：顶层私有类型会触发 library_private_types_in_public_api
  _FakeEngine? fake;

  setUp(() async {
    // 每条测试一个干净假引擎；关掉上一条的流让单例允许重起
    // （EngineClient 只起一次，复用会表现为「点了按钮但一条命令都没发」）
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = _FakeEngine();
    EngineClient.transportFactory = (_, __) => fake!;
    Notifier.enabled = false; // 别在跑测试时真弹桌面通知
  });

  tearDownAll(() =>
      EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args));

  // ── ① 状态机 ──────────────────────────────────────────────
  testWidgets('默认是普通压缩 → 防网盘扫描禁用，并说明为什么', (tester) async {
    await pumpPack(tester);

    expect(find.byKey(decoy), findsOneWidget);
    expect(switchEnabled(tester, decoy), isFalse,
        reason: '普通 ZIP 模式下引擎会回 BAD_PARAM，能点就是假开关');
    final title =
        (tester.widget<SwitchListTile>(find.byKey(decoy)).title as Text).data ?? '';
    expect(title, contains('只有伪装成文件时可用'),
        reason: 'E2 的不改封面开关也用同一句文案，所以按 key 取标题，别按文案数个数');
    expect(find.byKey(decoySig), findsNothing,
        reason: '总开关都没开，子开关不该出现');
  });

  testWidgets('切到「伪装成文件」→ 开关可用；勾上才出「假特征码」子开关', (tester) async {
    await pumpPack(tester);

    await tester.tap(find.text('伪装成文件'));
    await tester.pumpAndSettle();

    expect(switchEnabled(tester, decoy), isTrue, reason: '伪装模式下引擎支持干扰数据');
    expect(find.textContaining('只有伪装成文件时可用'), findsNothing);
    expect(find.byKey(decoySig), findsNothing);

    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();

    expect(switchValue(tester, decoy), isTrue);
    expect(find.byKey(decoySig), findsOneWidget,
        reason: '开了总开关才该露出「连文件指纹也伪造」');
  });

  testWidgets('关掉总开关要把子开关一起清掉（下次打开不能偷偷带假特征码）',
      (tester) async {
    await pumpPack(tester);

    await tester.tap(find.text('伪装成文件'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(decoySig));
    await tester.pumpAndSettle();
    expect(switchValue(tester, decoySig), isTrue);

    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();
    expect(find.byKey(decoySig), findsNothing, reason: '子开关随总开关一起藏起来');

    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();
    expect(switchValue(tester, decoySig), isFalse,
        reason: '子开关藏在界面上时还是开的话，用户根本看不见就在撒假特征码');
  });

  testWidgets('切回普通压缩 → 开关重新禁用（不留能点但无效的状态）', (tester) async {
    await pumpPack(tester);

    await tester.tap(find.text('伪装成文件'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();
    expect(switchValue(tester, decoy), isTrue);

    await tester.tap(find.text('普通压缩（.zip）'));
    await tester.pumpAndSettle();
    expect(switchEnabled(tester, decoy), isFalse);
    expect(switchValue(tester, decoy), isFalse,
        reason: '禁用状态下还显示「开」，用户会以为它生效了');
    expect(find.byKey(decoySig), findsNothing);
  });

  // ── ② 接线：开关 → 协议字段 ─────────────────────────────────
  testWidgets('勾上防网盘扫描 + 打包 → 协议收到 decoy:4 和 decoy_signatures',
      (tester) async {
    await pumpPack(tester, ready: true);
    expect(switchEnabled(tester, decoy), isTrue, reason: '带外壳进来就该是伪装模式');

    await tester.tap(find.byKey(decoy));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(decoySig));
    await tester.pumpAndSettle();

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull, reason: '点了打包必须真发一条 pack');
    expect(cmd!['decoy'], 4, reason: '界面写「撒 4 段」，请求里就得是 4');
    expect(cmd['decoy_signatures'], isTrue,
        reason: '子开关开了却没传，等于没打开指纹伪造');
    expect(cmd.containsKey('decoy_size'), isFalse,
        reason: '每段大小用引擎默认 16K，界面不瞎传');
    expect(cmd['mode'], 'disguise');
  });

  testWidgets('不勾就不带 decoy（反向验证：别默认撒垃圾）', (tester) async {
    await pumpPack(tester, ready: true);

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!.containsKey('decoy'), isFalse,
        reason: '默认不撒干扰数据 —— 白多 64 KB 是用户不该付的');
    expect(cmd.containsKey('decoy_signatures'), isFalse);
  });
}

/// 假引擎：把界面发来的命令记下来，按命令回一条形状正确的结果
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
            'total_size': 123456,
            'entries': 2,
            'layout': 'FreeAtom',
            'mode': m['mode'] ?? 'disguise',
          },
        }));
    }
  }
}
