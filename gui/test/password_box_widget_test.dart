// 「加密包失败后必须出现解压码输入框」的 widget 回归（2026-10-30 补）。
//
// 为什么非要有这个文件：这个 bug 在真机上表现为「只有一个报错、没有输入框」，
// 而当时 `flutter test` 43 条全绿 —— 因为**没有任何一条测试覆盖密码失败态**。
// 光测纯函数（humanize_test.dart）不够：那个测的是「判断对不对」，
// 这里测的是「判断对了之后界面真的画出来了」。
//
// 反向验证过：把 home_page 的 `_needsPassword` 改回老写法（PASSWORD_REQUIRED），
// 本文件三条正向用例当场红。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';

/// 造一份预览结果（加密包：3 个文件加密、AES-256）
Preview encPreview({int encryptedCount = 3}) => Preview(
      format: 'zip',
      complete: true,
      totalSize: 100,
      encryptedCount: encryptedCount,
      encKind: encryptedCount > 0 ? 'aes256' : null,
      entries: [
        EntryItem(
          index: 1,
          path: '说明.txt',
          size: 100,
          packed: 90,
          isDir: false,
          encrypted: encryptedCount > 0,
          risky: false,
        ),
      ],
      hints: const [],
    );

/// 进「预览页」（不是失败页）
Future<void> pumpPreview(WidgetTester tester, {int encryptedCount = 3}) async {
  tester.view.physicalSize = const Size(1200, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugPreview: encPreview(encryptedCount: encryptedCount),
      ),
    ),
  ));
  await tester.pump();
}

/// 直接灌一个失败态（生产代码永远不传，见 HomePage.debugErrorCode）
Future<void> pumpError(WidgetTester tester, String code, {String note = ''}) async {
  // 视口放宽：失败页有错误详情/按钮一长串，800x600 下输入框会被挤出视口
  tester.view.physicalSize = const Size(1200, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugErrorCode: code,
        debugErrorNote: note,
      ),
    ),
  ));
  await tester.pump();
}

void main() {
  group('缺解压码 → 必须给出输入框', () {
    for (final code in ['NEED_PASSWORD', 'WRONG_PASSWORD', 'HEAD_ENCRYPTED']) {
      testWidgets('$code 时出现「用这个解压码重试」和输入框', (tester) async {
        await pumpError(tester, code);
        expect(find.text('用这个解压码重试'), findsOneWidget,
            reason: '$code 是引擎真发的码，界面必须给输入框');
        expect(find.byType(TextField), findsWidgets,
            reason: '输入框没画出来，用户没地方输解压码');
        // 输入的框是密文显示（眼睛图标能切明文）
        expect(find.byTooltip('显示解压码'), findsOneWidget);
      });
    }

    testWidgets('驼峰名也照样给输入框（协议 golden 给的是枚举名）', (tester) async {
      await pumpError(tester, 'WrongPassword');
      expect(find.text('用这个解压码重试'), findsOneWidget);
    });

    testWidgets('错误文案是人话，不露原始码', (tester) async {
      await pumpError(tester, 'NEED_PASSWORD');
      expect(find.textContaining('这个包需要解压码'), findsWidgets);
      expect(find.textContaining('NEED_PASSWORD'), findsNothing);
    });
  });

  group('不是密码问题 → 不许给输入框', () {
    for (final code in ['NOT_ARCHIVE', 'MISSING_VOLUME', 'DATA_LOST', 'TRUNCATED']) {
      testWidgets('$code 时没有解压码输入框', (tester) async {
        await pumpError(tester, code);
        expect(find.text('用这个解压码重试'), findsNothing,
            reason: '$code 跟密码无关，给输入框只会让人白试');
      });
    }
  });

  group('救援按钮按真码出现', () {
    testWidgets('DATA_LOST 给「试着救出来」', (tester) async {
      await pumpError(tester, 'DATA_LOST');
      expect(find.textContaining('救'), findsWidgets,
          reason: '数据丢了应该给救援入口');
    });

    testWidgets('NEED_PASSWORD 不给救援按钮', (tester) async {
      await pumpError(tester, 'NEED_PASSWORD');
      expect(find.text('试着救出来（索引坏了也能抢救）'), findsNothing);
    });
  });

  group('预览页就该给解压码入口（用户 2026-10-30 真机反馈）', () {
    testWidgets('加密包一预览就能输密码，不用等解压失败', (tester) async {
      await pumpPreview(tester);
      expect(find.textContaining('解压码（这个包有 3 个文件加密）'), findsOneWidget,
          reason: '引擎预览里已经给了 encryptedCount，没理由拖到失败才问');
      expect(find.byType(TextField), findsWidgets);
    });

    testWidgets('没加密的包不乱出现解压码框', (tester) async {
      await pumpPreview(tester, encryptedCount: 0);
      expect(find.textContaining('解压码（这个包有'), findsNothing);
    });
  });
}
