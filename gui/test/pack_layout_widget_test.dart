// 布局三档的回归（E2 的「接在文件末尾」+ T18-1 的「插在视频中间」）
//
// 协议：`pack.layout` = free / tail / mid（老写法 `pack.tail: true` 等于 tail）。
// 界面只有**一个**下拉 —— 开关 + 下拉并存就是两套优先级，说不清谁赢。
//
// 五条铁律：
//   ① 默认**绝不能传 tail**：默认传 tail 会把所有 MP4 伪装包变成「播不了的视频」——
//      这是 E2 那轮最大的坑，留在这里当护栏；
//   ② 默认值是 free：对 MP4 封面它等于引擎默认（藏进 free 原子，视频照常播）；
//      对图片/文本封面引擎会自己降级成「接在末尾」（`build()` 里按魔数判断）；
//   ③ 选「接在文件末尾」→ 请求必须是 `layout: 'tail'`（假选项没意义）；
//   ④ 选「插在视频中间」→ 请求必须是 `layout: 'mid'`；
//   ⑤ 普通压缩 / 自解压没有封面 → 下拉禁用并说明；外壳不是视频 → mid 那档点不了。
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/pack_page.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

const layoutTile = ValueKey('layout-select');
const layoutDrop = ValueKey('layout-dropdown');

Future<void> pumpPack(WidgetTester tester,
    {bool ready = false, String cover = '/tmp/封面.mp4'}) async {
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: ready
          ? PackPage(
              debugInputs: const ['/tmp/内容'],
              debugOut: '/tmp/成品.mp4',
              debugCover: cover,
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

/// 当前选中的档（下拉的 value）
String? chosen(WidgetTester tester) =>
    tester.widget<DropdownButton<String>>(find.byKey(layoutDrop)).value;

/// 副标题全文（布局说明在这）
String subtitle(WidgetTester tester) {
  final tile = tester.widget<ListTile>(find.byKey(layoutTile));
  return (tile.subtitle as Text?)?.data ?? '';
}

bool dropEnabled(WidgetTester tester) =>
    tester.widget<DropdownButton<String>>(find.byKey(layoutDrop)).onChanged !=
    null;

/// 某一档能不能点（外壳不是视频时 mid 要灰掉）
bool itemEnabled(WidgetTester tester, String value) {
  final dd = tester.widget<DropdownButton<String>>(find.byKey(layoutDrop));
  final item = dd.items!.firstWhere((i) => i.value == value);
  return item.enabled;
}

Future<void> pick(WidgetTester tester, String label) async {
  await tester.tap(find.byKey(layoutDrop));
  await tester.pumpAndSettle();
  await tester.tap(find.text(label).last);
  await tester.pumpAndSettle();
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

  testWidgets('默认普通压缩 → 下拉禁用，并说明「没有封面，不涉及布局」', (tester) async {
    await pumpPack(tester);

    expect(find.byKey(layoutTile), findsOneWidget);
    expect(dropEnabled(tester), isFalse,
        reason: '普通 ZIP 没有封面，布局选项无意义，能点就是假控件');
    expect(subtitle(tester), contains('不涉及布局'),
        reason: '禁用时必须写明原因，不能让用户以为是坏了');
  });

  testWidgets('伪装成文件 → 可用，默认 free，副标题说「默认让封面装数据」', (tester) async {
    await pumpPack(tester);

    await tester.tap(find.text('伪装成文件'));
    await tester.pumpAndSettle();

    expect(dropEnabled(tester), isTrue);
    expect(chosen(tester), 'free', reason: '默认是 free（与引擎默认一致，视频还能播）');
    expect(subtitle(tester), contains('默认让封面的视频结构装数据'));
  });

  testWidgets('默认打包 → 请求带 layout: free，且绝不带 tail', (tester) async {
    await pumpPack(tester, ready: true);
    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd, isNotNull);
    expect(cmd!['layout'], 'free');
    expect(cmd.containsKey('tail'), isFalse,
        reason: '默认传 tail 会把所有 MP4 伪装包都改成尾部追加（视频播不了）');
  });

  testWidgets('选「接在文件末尾」→ 副标题变「一个字节都不动」，请求带 tail',
      (tester) async {
    await pumpPack(tester, ready: true);

    await pick(tester, '接在文件末尾');

    expect(chosen(tester), 'tail');
    expect(subtitle(tester), contains('封面文件一个字节都不动'));

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd!['layout'], 'tail',
        reason: '界面选了却没传给引擎 —— 用户以为改了、其实没改');
  });

  testWidgets('选「插在视频中间」→ 请求带 mid（T18-1 / L4）', (tester) async {
    await pumpPack(tester, ready: true);

    await pick(tester, '插在视频中间');

    expect(chosen(tester), 'mid');
    expect(subtitle(tester), contains('夹在视频数据中间'),
        reason: '选了高级档必须说清代价（别的解压软件多半认不出）');

    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('pack');
    expect(cmd!['layout'], 'mid', reason: '选了 mid 却没传 —— 界面是假的');
  });

  testWidgets('引擎降级了必须说出来：要 mid、回来 FreeAtom → 结果卡片有红字',
      (tester) async {
    // 假引擎固定回 FreeAtom：模拟「外壳结构插不进中间，引擎自动降级」
    await pumpPack(tester, ready: true);
    await pick(tester, '插在视频中间');
    await tester.tap(find.text('开始打包（伪装）'));
    await pumpFrames(tester);

    expect(find.textContaining('插不进视频中间'), findsOneWidget,
        reason: '不说明的话，用户以为选中的那档生效了，实际是另一种包');
  });

  testWidgets('外壳是图片 → mid 那档点不了（引擎按魔数会降级）', (tester) async {
    // 注意必须 ready: true —— 只有灌了 debugInputs 才会把 debugCover 也吃进去
    // （第一版忘了传，页面还是普通压缩模式，测的是「下拉整体禁用」，假绿）
    await pumpPack(tester, ready: true, cover: '/tmp/封面.png');

    expect(dropEnabled(tester), isTrue, reason: '布局整体还能选（free / 接末尾）');
    expect(itemEnabled(tester, 'free'), isTrue);
    expect(itemEnabled(tester, 'tail'), isTrue);
    expect(itemEnabled(tester, 'mid'), isFalse,
        reason: 'PNG 里没有 mdat/moov，塞中间是骗人的选项');
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
            // 永远回 FreeAtom：正好用来验「要 mid 却降级了」的提示
            'layout': 'FreeAtom',
            'decoy_segments': 0,
            'millis': 12,
            'warnings': <dynamic>[],
          },
        }));
    }
  }
}
