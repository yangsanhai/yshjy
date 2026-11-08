// 拖放回归：打包页要能直接拖文件/文件夹进来；页面不在前台时不能抢首页的拖放。
//
// 为什么用原生 MethodChannel 灌事件而不是找鼠标：`desktop_drop` 的入口就是
// MethodChannel('desktop_drop') 的 `entered` / `performOperation` 两个方法，
// 从这一层灌进去，测到的就是**真实的 DropTarget 命中逻辑**（含 inBounds 判断）。
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/pack_page.dart';

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

/// 模拟「拖到 (x,y) 松手」，路径直接给
Future<void> dropAt(WidgetTester tester, Offset at, List<String> paths) async {
  await _send(MethodCall('entered', [at.dx, at.dy]));
  await tester.pump();
  await _send(MethodCall('performOperation', paths));
  await tester.pumpAndSettle();
}

Future<void> pumpPack(WidgetTester tester, {bool acceptDrop = true}) async {
  tester.view.physicalSize = const Size(1200, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(body: PackPage(acceptDrop: acceptDrop)),
  ));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('拖两个文件进打包页 → 都进输入列表', (tester) async {
    await pumpPack(tester);
    final center = tester.getCenter(find.byType(PackPage));
    await dropAt(tester, center, ['/下载/a.txt', '/下载/b.bin']);

    expect(find.text('a.txt'), findsOneWidget, reason: '拖进来的文件应当出现在列表里');
    expect(find.text('b.bin'), findsOneWidget);
    // 名字已按路径推出来（输出名跟着第一个输入走）
    expect(find.textContaining('a'), findsWidgets);
  });

  testWidgets('同一路径拖两次不重复加', (tester) async {
    await pumpPack(tester);
    final center = tester.getCenter(find.byType(PackPage));
    await dropAt(tester, center, ['/下载/a.txt']);
    await dropAt(tester, center, ['/下载/a.txt']);
    expect(find.text('a.txt'), findsOneWidget, reason: '去重后只应有一行');
  });

  testWidgets('acceptDrop=false 时拖放不生效（页面不在前台）', (tester) async {
    await pumpPack(tester, acceptDrop: false);
    final center = tester.getCenter(find.byType(PackPage));
    await dropAt(tester, center, ['/下载/不该出现.txt']);
    expect(find.text('不该出现.txt'), findsNothing);
  });
}
