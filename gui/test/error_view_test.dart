// 失败页的回归 —— 按块拆分的第 8 块（空间不足卡 / 错误头 / 密码重试）。
//
// 失败页是用户撞上最多的一屏，所以这里钉的不是「能显示」，而是**每种失败该说什么**：
// 缺分卷要说缺哪一块、空间不够要给三个真实数字、密码错要让常用码一点就跑。
// 还有一条不是「显示」而是「不显示」：分卷中间块和空间不足时**不许**冒出「这文件其实是…」
// （用户会以为包坏了跑去重下）。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_error_view.dart';
import 'package:yshjy_gui/pages/home_space_shortage.dart';

Future<void> pumpAny(WidgetTester tester, Widget child) async {
  tester.view.physicalSize = const Size(1000, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: Scaffold(body: child)));
  await tester.pump();
}

Future<List<Widget>> pumpSpace(
  WidgetTester tester, {
  String line = '还差 1 GB',
  bool hasPreview = true,
}) async {
  final hits = <String>[];
  final widgets = spaceShortageBlock(
    ThemeData.light().colorScheme,
    line: line,
    hasPreview: hasPreview,
    onChangeDest: () => hits.add('dest'),
    onPickPart: () => hits.add('part'),
    onTryAnyway: () => hits.add('force'),
  );
  await pumpAny(tester, Column(children: widgets));
  return widgets;
}

void main() {
  group('spaceLine（三个数必须是真的）', () {
    test('detail 缺失 → 退成通用建议，不编数字', () {
      expect(spaceLine(null), contains('清一点空间'));
      expect(spaceLine(const {}), contains('清一点空间'));
    });

    test('数字是字符串也能读（协议里就是字符串）', () {
      final s = spaceLine({'need': '2000', 'available': '1000'});
      expect(s, contains('1000'), reason: '还差 1000 字节要算出来');
    });

    test('够用时不出现负数差额', () {
      final s = spaceLine({'need': '1000', 'available': '5000'});
      expect(s, isNot(contains('-')), reason: '算成负数差额会让人以为算错了');
    });
  });

  group('spaceShortageBlock', () {
    testWidgets('三条出路都在：换个位置 / 只解压部分 / 继续试试', (tester) async {
      await pumpSpace(tester);
      expect(find.text('换个位置'), findsOneWidget);
      expect(find.text('只解压部分'), findsOneWidget);
      expect(find.text('继续试试'), findsOneWidget);
      expect(find.textContaining('不做空间检查'), findsOneWidget,
          reason: '「继续试试」的风险必须写在旁边，不能让用户以为是安全选项');
    });

    testWidgets('没有预览时不给「只解压部分」（还没看到清单，挑不了）', (tester) async {
      await pumpSpace(tester, hasPreview: false);
      expect(find.text('只解压部分'), findsNothing);
      expect(find.text('换个位置'), findsOneWidget);
    });

    testWidgets('三个按钮各报各的', (tester) async {
      await pumpSpace(tester);
      await tester.tap(find.text('换个位置'));
      await tester.tap(find.text('继续试试'));
      await tester.pump();
      expect(find.text('磁盘空间不够'), findsOneWidget);
    });
  });

  group('ErrorHead', () {
    testWidgets('未知错误码也能给一句话（不许空白）', (tester) async {
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: null,
          errorNote: null,
          errorDetail: null,
          canRescue: false,
          hint: null,
          onRescue: () {},
          onMakeReport: () {},
        ),
      );
      expect(find.byIcon(Icons.error_outline), findsOneWidget);
      expect(find.textContaining('原因不明'), findsOneWidget,
          reason: '兜底文案必须存在，空白比说「原因不明」更差');
    });

    testWidgets('缺分卷：说清缺哪一块 + 已找到几块', (tester) async {
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: 'MISSING_VOLUME',
          errorNote: null,
          errorDetail: const {'missing': 'part3.rar', 'found': '2'},
          canRescue: false,
          hint: null,
          onRescue: () {},
          onMakeReport: () {},
        ),
      );
      expect(find.textContaining('part3.rar'), findsOneWidget,
          reason: '只说「分卷不全」用户不知道缺哪个文件');
      expect(find.textContaining('缺这一块'), findsOneWidget);
    });

    testWidgets('能救时给救援按钮，点了报父层', (tester) async {
      var rescued = 0;
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: 'BAD_INDEX',
          errorNote: null,
          errorDetail: null,
          canRescue: true,
          hint: null,
          onRescue: () => rescued++,
          onMakeReport: () {},
        ),
      );
      await tester.tap(find.textContaining('试着救出来'));
      await tester.pump();
      expect(rescued, 1);
    });

    testWidgets('没有技术详情时两个详情按钮都不出现', (tester) async {
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: 'BAD_INDEX',
          errorNote: null,
          errorDetail: null,
          canRescue: false,
          hint: null,
          onRescue: () {},
          onMakeReport: () {},
        ),
      );
      // ★ 用图标找，别用文字：兜底文案 err.unknown 是「…可以点「生成诊断包」查一下」，
      //   标题里就含这四个字，find.textContaining('生成诊断包') 必然误中标题。
      expect(find.byIcon(Icons.bug_report_outlined), findsNothing);
      expect(find.byIcon(Icons.copy), findsNothing);
    });

    testWidgets('有技术详情：生成诊断包可点、复制详情也行', (tester) async {
      var reports = 0;
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: 'BAD_INDEX',
          errorNote: 'Error { code: BAD_INDEX }',
          errorDetail: null,
          canRescue: false,
          hint: null,
          onRescue: () {},
          onMakeReport: () => reports++,
        ),
      );
      expect(find.byIcon(Icons.bug_report_outlined), findsOneWidget);
      expect(find.byIcon(Icons.copy), findsOneWidget,
          reason: '两个按钮：生成诊断包（报障用）、复制错误详情');
      await tester.tap(find.byIcon(Icons.bug_report_outlined));
      await tester.pump();
      expect(reports, 1);
    });

    testWidgets('warn 级提示用错误色，普通提示不用', (tester) async {
      await pumpAny(
        tester,
        ErrorHead(
          errorCode: 'BAD_INDEX',
          errorNote: null,
          errorDetail: null,
          canRescue: false,
          hint: (text: '这其实是 rar', warn: false),
          onRescue: () {},
          onMakeReport: () {},
        ),
      );
      final texts = tester.widgetList<Text>(find.text('这其实是 rar')).toList();
      expect(texts.single.style?.color, isNull,
          reason: '普通提示染成红色，用户会以为出了大事');
    });
  });

  group('ErrorPasswordRetry', () {
    Future<Rec> pumpRetry(
      WidgetTester tester, {
      List<String> recent = const [],
      bool showPlain = false,
    }) async {
      final rec = Rec();
      final ctrl = TextEditingController();
      addTearDown(ctrl.dispose);
      await pumpAny(
        tester,
        ErrorPasswordRetry(
          controller: ctrl,
          showPlain: showPlain,
          recent: recent,
          onToggleShowPlain: () => rec.toggles++,
          onRetry: () => rec.retries++,
          onPickRecent: rec.picked.add,
        ),
      );
      return rec;
    }

    testWidgets('重试按钮 + 回车都能触发重试', (tester) async {
      final rec = await pumpRetry(tester);
      await tester.tap(find.text('用这个解压码重试'));
      await tester.pump();
      expect(rec.retries, 1);

      await tester.enterText(find.byType(TextField), 'abc');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pump();
      expect(rec.retries, 2, reason: '输完密码按回车是直觉动作，必须能跑');
    });

    testWidgets('★ 常用码点一下只报父层（填入+重试由父层做，和预览页不同）', (tester) async {
      final rec = await pumpRetry(tester, recent: ['123456']);
      await tester.tap(find.text('123456'));
      await tester.pump();
      expect(rec.picked, ['123456']);
      expect(rec.retries, 0, reason: '组件只报「点了哪个码」，重试由父层决定（避免两份真相）');
    });

    testWidgets('密码框默认自动聚焦（用户就是要来输密码的）', (tester) async {
      await pumpRetry(tester);
      final tf = tester.widget<TextField>(find.byType(TextField));
      expect(tf.autofocus, isTrue,
          reason: '失败页让用户自己点一下输入框，多一步就多一次放弃');
    });

    testWidgets('眼睛按钮切换', (tester) async {
      final rec = await pumpRetry(tester);
      await tester.tap(find.byTooltip('显示解压码'));
      await tester.pump();
      expect(rec.toggles, 1);
    });
  });
}

class Rec {
  int retries = 0;
  int toggles = 0;
  final picked = <String>[];
}
