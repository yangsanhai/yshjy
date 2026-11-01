// 批量完成卡的 widget 回归（按块拆分的第 2 块）。
//
// 为什么值得单测：这张卡原来埋在 home_page.dart 的 2976 行 State 里，**一条测试都没有**
// （全库 grep batch-done 只在生产代码里出现）。抽成独立 widget 之后，
// 只要喂一份「引擎 batch 结果」，就能验「成功/失败怎么显示、按钮接没接上」——
// 不用起假引擎、不碰文件 I/O。
//
// 数据形状照引擎真实回包抄（见 docs/PROTOCOL.md 的 batch 结果）：
//   {ok, failed, partial, elapsed_ms, bytes, jobs: [{path, dest, outcome}]}

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_batch_done_view.dart';

Map<String, dynamic> batchResult({
  int ok = 1,
  int failed = 0,
  int partial = 0,
  List<Map<String, dynamic>>? jobs,
}) =>
    {
      'ok': ok,
      'failed': failed,
      'partial': partial,
      'elapsed_ms': 1500,
      'bytes': 2048,
      'jobs': jobs ??
          [
            {'path': '/tmp/a.zip', 'dest': '/tmp/out', 'outcome': 'ok'},
          ],
    };

Future<Map<String, List<String>>> pumpCard(
  WidgetTester tester,
  Map<String, dynamic> batch,
) async {
  final calls = <String, List<String>>{
    'openFolder': [],
    'reveal': [],
    'reset': [],
  };
  tester.view.physicalSize = const Size(1200, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: BatchDoneView(
        batch: batch,
        onOpenFolder: (d) => calls['openFolder']!.add(d),
        onRevealInFolder: (p) => calls['reveal']!.add(p),
        onReset: () => calls['reset']!.add('-'),
      ),
    ),
  ));
  await tester.pump();
  return calls;
}

void main() {
  testWidgets('全成功：对勾 + 「解压完成」+ 摘要写成功数', (tester) async {
    await pumpCard(tester, batchResult(ok: 3));
    expect(find.text('解压完成'), findsOneWidget);
    expect(find.textContaining('3 成功'), findsOneWidget);
    expect(find.byIcon(Icons.check_circle), findsOneWidget,
        reason: '全成功却显示感叹号，用户会以为出了问题');
    expect(find.byIcon(Icons.info_outline), findsNothing);
  });

  testWidgets('有失败：换感叹号 + 列出失败项 + 点一下去定位', (tester) async {
    final calls = await pumpCard(
      tester,
      batchResult(ok: 1, failed: 1, jobs: [
        {'path': '/tmp/好的.zip', 'dest': '/tmp/out', 'outcome': 'ok'},
        {'path': '/tmp/坏掉的.zip', 'outcome': 'bad_password'},
      ]),
    );
    expect(find.byIcon(Icons.info_outline), findsOneWidget);
    expect(find.text('坏掉的.zip'), findsOneWidget,
        reason: '失败项不列出来，用户不知道哪个包没解开');
    expect(find.textContaining('1 失败'), findsOneWidget);

    await tester.tap(find.text('坏掉的.zip'));
    await tester.pump();
    expect(calls['reveal'], ['/tmp/坏掉的.zip'],
        reason: '调研 07:135 —— 静默隔离是反面教材，失败项必须能一点就定位');
  });

  testWidgets('「打开输出目录」用的是第一个有产物的目录', (tester) async {
    final calls = await pumpCard(tester, batchResult());
    await tester.tap(find.text('打开输出目录'));
    await tester.pump();
    expect(calls['openFolder'], ['/tmp/out']);
  });

  testWidgets('「再来一批」接的是重置回调', (tester) async {
    final calls = await pumpCard(tester, batchResult());
    await tester.tap(find.text('再来一批'));
    await tester.pump();
    expect(calls['reset'], hasLength(1));
  });

  testWidgets('一个产物目录都没有时，不显示「打开输出目录」', (tester) async {
    await pumpCard(
      tester,
      batchResult(ok: 1, jobs: [
        {'path': '/tmp/a.zip', 'outcome': 'ok'},
      ]),
    );
    expect(find.text('打开输出目录'), findsNothing,
        reason: '没有目录却给个按钮，点下去只会报错');
    expect(find.text('再来一批'), findsOneWidget);
  });

  // ── T11 无障碍：键盘 ─────────────────────────────────────────────
  testWidgets('Enter = 打开输出目录（本屏主操作）', (tester) async {
    final calls = await pumpCard(
      tester,
      batchResult(ok: 1, jobs: [
        {'path': '/tmp/a.zip', 'dest': '/tmp/out', 'outcome': 'ok'},
      ]),
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(calls['openFolder'], ['/tmp/out'],
        reason: '批量做完最想干的就是看产物，键盘按了没反应等于白做');
  });

  testWidgets('全失败时 Enter 不打开目录（键盘不能造出界面上没有的入口）', (tester) async {
    final calls = await pumpCard(
      tester,
      batchResult(ok: 0, failed: 1, jobs: [
        {'path': '/tmp/a.zip', 'outcome': 'failed'},
      ]),
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(calls['openFolder'], isEmpty,
        reason: '一个成功的都没有还去打开目录，用户会看到空文件夹或报错');
  });

  testWidgets('Esc = 再来一批（取消/返回）', (tester) async {
    final calls = await pumpCard(tester, batchResult());
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(calls['reset'], hasLength(1),
        reason: 'Esc 没接上，键盘用户做不了第二批');
  });
}
