// E5「智能解压（不重复套文件夹）」的回归（T7 第 3 项）
//
// 三层都要钉：
//   ① 默认开着，且解压请求里 `smart_dir == true`（不传也行，但显式传出去更可控）；
//   ② 在高级选项里关掉 → 请求里必须 `smart_dir == false`（关不掉就是假开关）；
//   ③ 选了「输出到指定目录」时**必须禁用并说明** —— 引擎给了 dest 就两种模式
//      都把产物直接铺进去，开关这时没有任何作用，能点就是骗人。
//
// ⚠️ 协议层（`api.rs` 的 extract 以前把 smart_dir 写死 true）由
// `tests/smart_dir_proto.py` 用真进程验，那个探针已挂进 `tests/all.sh`。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

const smartDir = ValueKey('smart-dir');

Preview sample() => Preview(
      format: 'zip',
      complete: true,
      totalSize: 13440,
      encryptedCount: 0,
      encKind: null,
      entries: [
        EntryItem(
            index: 1,
            path: '顶层/甲.txt',
            size: 13440,
            packed: 900,
            isDir: false,
            encrypted: false,
            risky: false),
      ],
      hints: const [],
    );

Future<void> pumpPreview(WidgetTester tester, {String? outDir}) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugPreview: sample(),
        debugOutDir: outDir,
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

/// 打开「高级选项」对话框
Future<void> openAdvanced(WidgetTester tester) async {
  await tester.tap(find.textContaining('高级：'));
  await tester.pumpAndSettle();
}

Future<void> closeAdvanced(WidgetTester tester) async {
  await tester.tap(find.text('知道了'));
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

  testWidgets('默认开着、可用；打包请求里带 smart_dir: true', (tester) async {
    await pumpPreview(tester);
    await openAdvanced(tester);

    final w = tester.widget<SwitchListTile>(find.byKey(smartDir));
    expect(w.value, isTrue, reason: '默认必须与引擎/命令行一致（智能）');
    expect(w.onChanged, isNotNull, reason: '没选输出目录时它必须能点');
    expect(find.textContaining('包里只有一个文件夹时不再多套一层'), findsOneWidget);

    await closeAdvanced(tester);
    await tester.tap(find.textContaining('开始解压'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('extract');
    expect(cmd, isNotNull);
    expect(cmd!['smart_dir'], isTrue);
  });

  testWidgets('关掉 → 请求里 smart_dir: false，副标题说明「永远建同名文件夹」',
      (tester) async {
    await pumpPreview(tester);
    await openAdvanced(tester);

    await tester.tap(find.byKey(smartDir));
    await tester.pumpAndSettle();

    final w = tester.widget<SwitchListTile>(find.byKey(smartDir));
    expect(w.value, isFalse);
    expect(find.textContaining('永远建一个和包同名的文件夹'), findsOneWidget);

    await closeAdvanced(tester);
    await tester.tap(find.textContaining('开始解压'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('extract');
    expect(cmd, isNotNull);
    expect(cmd!['smart_dir'], isFalse,
        reason: '界面关了却没传给引擎 —— 用户以为改了、其实没改');
  });

  testWidgets('选了输出目录 → 开关禁用并写明不生效（不做点了没反应的开关）',
      (tester) async {
    await pumpPreview(tester, outDir: '/tmp/我选的目录');
    await openAdvanced(tester);

    final w = tester.widget<SwitchListTile>(find.byKey(smartDir));
    expect(w.onChanged, isNull,
        reason: '给了 dest 时引擎忽略 smart_dir，能点就是骗人');
    expect(find.textContaining('选了输出目录时不生效'), findsOneWidget);
  });
}

/// 假引擎：记录命令，按命令回一条形状正确的结果
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
      case 'extract':
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {
            'outcome': 'Success',
            'entries_done': 1,
            'entries_total': 1,
            'bytes_written': 13440,
            'dest': '/tmp/测试包/顶层',
            'millis': 12,
            'failed': <dynamic>[],
          },
        }));
    }
  }
}
