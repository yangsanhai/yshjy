// 预览页顶部两块小信息的回归 —— 按块拆分的第 5d-2 第 3、4 段。
//
// 这两块都是纯展示，但各有**一个必须守住的「不该出现」**：
// 摘要行：没加密就不许出现锁 chip（凭空一个「0 个加密」会让人以为包坏了）；
// 备注区：没有备注就不许占位置（多一块空白，列表就少一行）。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/preview_header.dart';

EntryItem f(String path, int index, int size) => EntryItem(
      index: index,
      path: path,
      size: size,
      packed: size,
      isDir: false,
      encrypted: false,
      risky: false,
    );

Preview pv({
  String format = 'zip',
  int files = 3,
  int totalSize = 2048,
  int encrypted = 0,
}) =>
    Preview(
      format: format,
      complete: true,
      totalSize: totalSize,
      encryptedCount: encrypted,
      encKind: null,
      entries: [
        for (var i = 0; i < files; i++) f('f$i.txt', i, 10),
      ],
      hints: const [],
    );

Future<void> pumpSummary(WidgetTester tester, Preview p) async {
  tester.view.physicalSize = const Size(900, 600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(body: PreviewSummary(preview: p)),
  ));
  await tester.pump();
}

Future<void> pumpNotes(WidgetTester tester, List<String> notes) async {
  tester.view.physicalSize = const Size(900, 600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(body: PreviewNotes(notes: notes)),
  ));
  await tester.pump();
}

void main() {
  testWidgets('摘要：格式大写 + 文件数 + 大小', (tester) async {
    await pumpSummary(tester, pv(format: '7z', files: 5, totalSize: 2048));
    expect(find.text('7Z'), findsOneWidget,
        reason: '格式写小写也就算了，写成原始码或空白就无从判断能不能解');
    expect(find.textContaining('5'), findsWidgets);
  });

  testWidgets('摘要：没加密就不出现锁 chip', (tester) async {
    await pumpSummary(tester, pv(encrypted: 0));
    expect(find.byIcon(Icons.lock_outline), findsNothing,
        reason: '没加密却画一把锁，用户会去找并不存在的解压码');
    expect(find.textContaining('个加密'), findsNothing);
  });

  testWidgets('摘要：有加密就写明几个', (tester) async {
    await pumpSummary(tester, pv(encrypted: 3));
    expect(find.byIcon(Icons.lock_outline), findsOneWidget);
    expect(find.textContaining('3 个加密'), findsOneWidget,
        reason: '只说「有密码」不说几个，用户不知道该准备几个码');
  });

  testWidgets('备注：空列表不占位置', (tester) async {
    await pumpNotes(tester, const []);
    expect(find.byType(Row), findsNothing, reason: '一条备注都没有却渲染一行空白');
    expect(find.byIcon(Icons.info_outline), findsNothing);
  });

  testWidgets('备注：每条都显示出来', (tester) async {
    await pumpNotes(tester, const ['这个包有 1/256 误判概率', '文件可能损坏']);
    expect(find.text('这个包有 1/256 误判概率'), findsOneWidget);
    expect(find.text('文件可能损坏'), findsOneWidget);
    expect(find.byIcon(Icons.info_outline), findsNWidgets(2),
        reason: '每条备注都该带个图标，纯文字容易被当成正文忽略');
  });
}
