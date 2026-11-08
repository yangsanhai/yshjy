// 首页拖放回归：拖一个文件 = 自动识别进预览；首页不在前台时不接。
//
// 事件入口与 pack_drop_test 相同：`desktop_drop` 的 MethodChannel
// （`entered` / `performOperation`），测到的是真实 DropTarget 的命中逻辑。
//
// ⚠️ 必须用 `tester.runAsync`：`_open` 里第一句是 `await Directory(path).exists()`
// （真实 I/O），在 widget 测试的假时钟里**永远不会完成** —— 这是本项目
// 2026-11-02 记下的坑，别把它当「拖放坏了」。
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_page.dart';

import 'fake_engine.dart';

const _channel = 'desktop_drop';
const _codec = StandardMethodCodec();

Future<void> _send(MethodCall call) async {
  await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .handlePlatformMessage(
    _channel,
    _codec.encodeMethodCall(call),
    (_) {},
  );
}

Map<String, dynamic> previewJson(String name) => {
      'format': 'zip',
      'complete': true,
      'total_size': 100,
      'encrypted': 0,
      'enc_kind': null,
      'entries': [
        {
          'index': 1,
          'path': name,
          'size': 100,
          'packed': 90,
          'is_dir': false,
          'encrypted': false,
          'is_risky': false,
        }
      ],
      'hints': [],
    };

void main() {
  late FakeEngine fake;

  setUp(() {
    fake = FakeEngine();
    installFakeEngine(fake);
  });
  tearDownAll(restoreRealTransport);

  Future<void> pumpHome(WidgetTester tester, {bool acceptDrop = true}) async {
    tester.view.physicalSize = const Size(1200, 1700);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: HomePage(autoStart: false, acceptDrop: acceptDrop),
      ),
    ));
    await tester.pumpAndSettle();
  }

  testWidgets('拖一个文件到首页 → 自动识别进预览', (tester) async {
    fake.autoReply['list'] = {'ok': true, 'result': previewJson('甲.txt')};
    await pumpHome(tester);

    await _send(const MethodCall('entered', [600.0, 850.0]));
    await tester.pump();
    expect(find.text('松手就行'), findsOneWidget, reason: '拖进来时标题要变');

    await tester.runAsync(() async {
      await _send(const MethodCall('performOperation', ['/下载/样本.zip']));
      await Future<void>.delayed(const Duration(milliseconds: 300));
    });
    await tester.pumpAndSettle();

    final listCall = fake.sent.where((c) => c['cmd'] == 'list').toList();
    expect(listCall.length, 1, reason: '拖一个文件应当发一次 list');
    expect(listCall.first['path'], '/下载/样本.zip');
    expect(find.text('甲.txt'), findsWidgets, reason: '预览清单应当显示条目');
  });

  testWidgets('acceptDrop=false 时首页不接拖放（页面不在前台）', (tester) async {
    await pumpHome(tester, acceptDrop: false);
    await _send(const MethodCall('entered', [600.0, 850.0]));
    await tester.pump();
    await tester.runAsync(() async {
      await _send(const MethodCall('performOperation', ['/下载/不该识别.zip']));
      await Future<void>.delayed(const Duration(milliseconds: 200));
    });
    await tester.pumpAndSettle();
    expect(fake.sent.any((c) => c['cmd'] == 'list'), isFalse);
    expect(find.text('松手就行'), findsNothing);
  });
}
