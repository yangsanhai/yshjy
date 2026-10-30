// S5「空间不够」的 widget 回归（2026-10-30 补，T4/T6）。
//
// 为什么非要有：这个状态以前是**死路** —— 引擎的 `SPACE_SHORTAGE` 事件全库只有定义、
// 零发射点，界面那段处理代码永远收不到；`--force` 界面也一个参数都没传。
// 所以「空间差一点」在界面上只有一句「磁盘空间不够」，用户不知道差多少、也没有出路。
//
// 这里测三件事：
//   ① 三个数真的画出来了（需要 / 只有 / 还差）
//   ② 三条出路都在（换个位置 / 继续试试；只解压部分只在有预览时给）
//   ③ 别的错误码不许出现这张卡（反向）

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_page.dart';

/// 真实量级：3 GiB 的包解到只有 2 GiB 的盘（2026-10-30 CLI 实测用的同一组数）
const needB = 3221225472; // 3.00 GB
const availB = 2085396480; // 1.94 GB

Future<void> pumpError(
  WidgetTester tester,
  String code, {
  Map<String, dynamic>? detail,
}) async {
  tester.view.physicalSize = const Size(1200, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(
        autoStart: false,
        debugErrorCode: code,
        debugErrorNote: '',
        debugErrorDetail: detail,
      ),
    ),
  ));
  await tester.pump();
}

void main() {
  group('空间不够 → 三个数 + 三条出路', () {
    testWidgets('三个数都画出来了（引擎给的是字符串数字也得认）', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE', detail: {
        'need': '$needB', // 协议里引擎 with() 传的是字符串
        'available': '$availB',
      });
      expect(find.text('磁盘空间不够'), findsOneWidget);
      // 需要 3.00 GB / 只有 1.94 GB / 还差 1.06 GB
      expect(find.textContaining('需要 3.00 GB'), findsOneWidget,
          reason: '「需要多少」没画出来，用户不知道该清多少');
      expect(find.textContaining('只有 1.94 GB'), findsOneWidget);
      expect(find.textContaining('还差 1.06 GB'), findsOneWidget,
          reason: '「还差多少」是这条卡最核心的一句话');
    });

    testWidgets('数字是 int 时也认', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE', detail: {
        'need': needB,
        'available': availB,
      });
      expect(find.textContaining('还差 1.06 GB'), findsOneWidget);
    });

    testWidgets('缺参数就退成通用建议，绝不编一个算错的数', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE');
      expect(find.text('磁盘空间不够'), findsOneWidget);
      expect(find.textContaining('清一点空间再试'), findsOneWidget);
      expect(find.textContaining('还差'), findsNothing,
          reason: '没有 available 时算不出「还差」，算出来的数会比不显示更糟');
    });

    testWidgets('「换个位置」和「继续试试」都在', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE', detail: {
        'need': '$needB',
        'available': '$availB',
      });
      expect(find.text('换个位置'), findsOneWidget);
      expect(find.text('继续试试'), findsOneWidget);
      // 没预览时不该出现「只解压部分」（否则点了没反应）
      expect(find.text('只解压部分'), findsNothing);
    });

    testWidgets('说明白了 force 的代价（不做空间检查）', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE', detail: {'need': '$needB'});
      expect(find.textContaining('不做空间检查'), findsOneWidget,
          reason: '「继续试试」会让用户拿内存/换盘风险，得先说清');
    });

    testWidgets('不许说「认不出的文件」（包是好的，别把人引去重下）', (tester) async {
      await pumpError(tester, 'NOT_ENOUGH_SPACE', detail: {
        'need': '$needB',
        'available': '$availB',
      });
      expect(find.textContaining('认不出的文件'), findsNothing,
          reason: '2026-10-30 真机截图里就有这句：用户会以为包坏了跑去重新下载');
    });
  });

  group('不是空间问题 → 不许出现这张卡', () {
    for (final code in ['NOT_ARCHIVE', 'DATA_LOST', 'NEED_PASSWORD', 'TRUNCATED']) {
      testWidgets('$code 时没有空间卡片', (tester) async {
        await pumpError(tester, code, detail: {'need': '$needB'});
        expect(find.text('磁盘空间不够'), findsNothing,
            reason: '$code 跟空间无关，画这张卡会把人引去清盘');
      });
    }
  });
}
