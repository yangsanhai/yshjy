// 「封面 / 内容」勾选卡的独立回归 —— 按块拆分的第 5d-2 第 2 段。
//
// 这块界面的形状是用户亲口要求过的（GUI-PLAN §5.2①）：
// 「伪装封面」和「压缩包内容」要**上下并列、各带勾选**，勾哪个做哪个。
// 所以测试要钉的不只是「能勾」，还有「没有封面时不许凭空多出一行」。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/preview_pick_card.dart';

Preview pv({
  Map<String, dynamic>? cover,
  int fileCount = 3,
  int totalSize = 2048,
}) =>
    Preview(
      format: 'zip',
      complete: true,
      totalSize: totalSize,
      encryptedCount: 0,
      encKind: null,
      cover: cover,
      // ★ fileCount 在 Preview 上是 **getter**（= entries 里非目录的个数），不是构造参数 ——
      //   所以「几个文件」必须靠真造 entries 才生效，传个 int 是不起作用的（第一版就这么错的）。
      entries: [
        for (var i = 0; i < fileCount; i++)
          EntryItem(
            index: i,
            path: 'f$i.txt',
            size: 10,
            packed: 10,
            isDir: false,
            encrypted: false,
            risky: false,
          ),
      ],
      hints: const [],
    );

class Rec {
  bool? cover;
  bool? extract;
}

Future<Rec> pumpCard(
  WidgetTester tester, {
  Map<String, dynamic>? cover,
  bool wantCover = true,
  bool wantExtract = true,
  int fileCount = 3,
  int totalSize = 2048,
}) async {
  final rec = Rec();
  tester.view.physicalSize = const Size(900, 1200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: PreviewPickCard(
        preview: pv(cover: cover, fileCount: fileCount, totalSize: totalSize),
        wantCover: wantCover,
        wantExtract: wantExtract,
        onWantCoverChanged: (v) => rec.cover = v,
        onWantExtractChanged: (v) => rec.extract = v,
      ),
    ),
  ));
  await tester.pump();
  return rec;
}

void main() {
  testWidgets('有封面：两行都在，且各带自己的勾选', (tester) async {
    await pumpCard(tester, cover: {'size': 361000000, 'ext': 'mp4'});
    expect(find.byKey(const ValueKey('want-cover')), findsOneWidget);
    expect(find.byKey(const ValueKey('want-extract')), findsOneWidget);
    expect(find.textContaining('伪装封面'), findsOneWidget);
    expect(find.textContaining('压缩包内容'), findsOneWidget);
    // 封面那行必须写出格式（mp4/png），否则用户不知道切出来的是什么
    expect(find.textContaining('mp4'), findsOneWidget);
  });

  testWidgets('没有封面：只剩内容一行，不许凭空多出一行空壳', (tester) async {
    await pumpCard(tester);
    expect(find.byKey(const ValueKey('want-cover')), findsNothing,
        reason: '普通压缩包没有伪装封面，多摆一行会让人以为包里有视频');
    expect(find.byKey(const ValueKey('want-extract')), findsOneWidget);
    expect(find.textContaining('伪装封面'), findsNothing);
  });

  testWidgets('勾/取消封面 → 报给父层', (tester) async {
    final rec1 = await pumpCard(
      tester,
      cover: {'size': 100, 'ext': 'png'},
      wantCover: true,
    );
    await tester.tap(find.byKey(const ValueKey('want-cover')));
    await tester.pump();
    expect(rec1.cover, isFalse, reason: '原来勾着，点一下应该报 false（取消）');

    final rec2 = await pumpCard(tester, cover: {'size': 100, 'ext': 'png'}, wantCover: false);
    await tester.tap(find.byKey(const ValueKey('want-cover')));
    await tester.pump();
    expect(rec2.cover, isTrue, reason: '原来没勾，点一下应该报 true');
  });

  testWidgets('勾/取消内容 → 报给父层', (tester) async {
    final rec = await pumpCard(tester);
    await tester.tap(find.byKey(const ValueKey('want-extract')));
    await tester.pump();
    expect(rec.extract, isFalse);
  });

  testWidgets('内容那行写明文件数和总大小', (tester) async {
    await pumpCard(tester, fileCount: 7, totalSize: 2048);
    expect(find.textContaining('7'), findsWidgets,
        reason: '要说清里面有几个文件，用户才知道解压要花多久');
  });
}
