// 预览页面包屑的回归（按块拆分的第 5a 块）。
//
// 这块的价值有两个：
//   ① 面包屑是「用户能在包里认清自己在哪」的唯一提示（调研 07:616-631：PeaZip 的面包屑被夸，
//      7-Zip 的下拉框被吐槽），所以第一节必须是**包名**而不是「全部」；
//   ② 第一节的包名是从**本地文件路径**取的 —— 这里原来写的是 `split('/').last`，
//      Windows 路径是反斜杠，会把整条路径当文件名显示出来。现在换成 baseName()，这条测试钉住它。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/pages/preview_crumb_bar.dart';

Future<List<String>> pumpCrumb(
  WidgetTester tester, {
  String? crumb,
  String? filePath,
}) async {
  final taps = <String>[];
  tester.view.physicalSize = const Size(1000, 600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: CrumbBar(
        crumb: crumb,
        filePath: filePath,
        onCrumbTap: taps.add,
      ),
    ),
  ));
  await tester.pump();
  return taps;
}

void main() {
  testWidgets('根层：第一节显示包名，不是「全部」', (tester) async {
    await pumpCrumb(tester, filePath: '/home/user/下载/伪装.mp4');
    expect(find.text('伪装.mp4'), findsOneWidget,
        reason: '路径一长，用户最先要认出的是「这是哪个包」；写「全部」等于没说');
    expect(find.text('全部'), findsNothing);
  });

  testWidgets('没有包路径时才退回「全部」', (tester) async {
    await pumpCrumb(tester);
    expect(find.text('全部'), findsOneWidget);
  });

  testWidgets('Windows 反斜杠路径也要只取文件名（原来只认 / ，会显示整条路径）', (tester) async {
    await pumpCrumb(tester, filePath: r'C:\Users\x\桌面\伪装包.mp4');
    expect(find.text('伪装包.mp4'), findsOneWidget,
        reason: '只认 / 的实现会把 C:\\Users\\x\\桌面\\伪装包.mp4 整条显示出来，第一节被撑爆');
    expect(find.textContaining(r'C:\Users'), findsNothing);
  });

  testWidgets('进到二级目录：三节都在，点某一节回那一层', (tester) async {
    final taps = await pumpCrumb(
      tester,
      crumb: '视频/第一季',
      filePath: '/home/x/合集.zip',
    );
    expect(find.text('合集.zip'), findsOneWidget);
    expect(find.text('视频'), findsOneWidget);
    expect(find.text('第一季'), findsOneWidget);

    // 点「视频」这一节 → 回到 '视频'
    await tester.tap(find.text('视频'));
    await tester.pump();
    expect(taps, ['视频']);

    // 点第一节（包名）→ 回根 ''
    await tester.tap(find.text('合集.zip'));
    await tester.pump();
    expect(taps, ['视频', ''],
        reason: '点包名应该回到根；回错层用户会以为面包屑是死的');
  });

  testWidgets('最后一节加粗、前面的节用主题色（当前层要看得出来）', (tester) async {
    await pumpCrumb(tester, crumb: '视频/第一季', filePath: '/home/x/合集.zip');
    final last = tester.widget<Text>(find.text('第一季'));
    final up = tester.widget<Text>(find.text('视频'));
    expect(last.style?.fontWeight, FontWeight.w600,
        reason: '最后一级不加粗，用户分不清「我在哪一层」和「我能点去哪」');
    expect(up.style?.fontWeight, FontWeight.normal);
  });
}
