// 单个文件完成屏的回归 —— 按块拆分的第 7 块。
//
// ★ 这块最容易和「批量完成卡」（第 2 块 BatchDoneView）混淆：那个是整批的总账，
//   这个是单个文件的一张卡。测试文件也分开放，别合并。
//
// 三条断言直接对应真机踩过的坑：
// ① 校验模式**没有产物**：不能显示产物大小（bytesWritten 是解码字节数不是写盘量）、
//    不能给「打开目录」（dest 是回显路径，目录根本没建）；
// ② 只勾「伪装封面」时解出 0 个文件是正常的，说「解开了 0 个文件」会吓人；
// ③ 嵌套包不自动解，要用户再点一次。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/pages/home_done_view.dart';

ExtractResult res({
  String outcome = 'Success',
  int done = 5,
  int total = 5,
  int bytes = 2048,
  String? dest = '/home/user/下载/产物',
  int millis = 1200,
  Map<String, dynamic>? cover,
}) =>
    ExtractResult(
      outcome: outcome,
      entriesDone: done,
      entriesTotal: total,
      bytesWritten: bytes,
      dest: dest,
      millis: millis,
      cover: cover,
    );

class Rec {
  int resets = 0;
  final opened = <String?>[];
  final scanned = <List<String>>[];
}

Future<Rec> pumpDone(
  WidgetTester tester, {
  ExtractResult? result,
  bool verifyMode = false,
  List<String> nested = const [],
  List<String> notes = const [],
  String? fileName = 'a.zip',
  String? password,
  String? entryHint,
}) async {
  final rec = Rec();
  tester.view.physicalSize = const Size(1000, 1400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: DoneView(
        result: result ?? res(),
        verifyMode: verifyMode,
        nestedArchives: nested,
        notes: notes,
        fileName: fileName,
        password: password,
        entryHint: entryHint,
        onScanNested: rec.scanned.add,
        onOpenFolder: rec.opened.add,
        onReset: () => rec.resets++,
      ),
    ),
  ));
  await tester.pump();
  return rec;
}

void main() {
  testWidgets('普通模式全成功：打勾图标 + 解开 N 个 + 产物路径 + 三个按钮', (tester) async {
    await pumpDone(tester, result: res(done: 5, total: 5));
    expect(find.byIcon(Icons.check_circle_outline), findsOneWidget);
    expect(find.textContaining('5'), findsWidgets);
    expect(find.text('/home/user/下载/产物'), findsOneWidget);
    expect(find.text('打开目录'), findsOneWidget);
    expect(find.text('复制解压方法'), findsOneWidget);
    expect(find.text('再处理一个'), findsOneWidget);
  });

  testWidgets('★ 校验模式：没有「打开目录」、不显示产物路径、不显示产物大小', (tester) async {
    await pumpDone(tester, verifyMode: true);
    expect(find.byIcon(Icons.verified_outlined), findsOneWidget);
    expect(find.text('打开目录'), findsNothing,
        reason: '校验模式一个字节都没写，dest 是引擎顺手回显的路径，点了只会报错');
    expect(find.text('/home/user/下载/产物'), findsNothing);
    expect(find.textContaining('没写任何文件'), findsOneWidget,
        reason: '说清「没写文件」，否则用户会去磁盘上找并不存在的产物');
  });

  testWidgets('★ 校验模式失败：报问题图标 + 说清几个没过', (tester) async {
    await pumpDone(
      tester,
      verifyMode: true,
      result: res(outcome: 'Failed', done: 3, total: 5),
    );
    expect(find.byIcon(Icons.report_problem_outlined), findsOneWidget);
    expect(find.textContaining('3/5'), findsOneWidget);
  });

  testWidgets('★ 只勾了伪装封面（0 个文件 + 有封面）：说「封面已提取」不吓人', (tester) async {
    await pumpDone(
      tester,
      result: res(done: 0, total: 0, cover: {'path': '/tmp/封面.mp4', 'size': 100}),
    );
    expect(find.textContaining('封面已提取'), findsOneWidget);
    expect(find.textContaining('解开了 0 个文件'), findsNothing,
        reason: '解出 0 个文件在这儿是正常的（用户只要封面），这么说会让人以为失败了');
  });

  testWidgets('部分解出：说清 M/N', (tester) async {
    // ok 的判据是 outcome.startsWith('Success')，所以「部分」必须给非 Success 的 outcome，
    // 否则走的是「解开了 N 个文件」那条分支（第一版就这么写错的）
    await pumpDone(tester, result: res(outcome: 'Failed', done: 3, total: 8));
    expect(find.textContaining('3/8'), findsOneWidget,
        reason: '只说「解出了一部分」不给数字，用户不知道缺多少');
    expect(find.byIcon(Icons.info_outline), findsOneWidget);
  });

  testWidgets('嵌套包：显示个数，点了才继续解（不自动）', (tester) async {
    final rec = await pumpDone(tester, nested: ['a/1.zip', 'a/2.7z']);
    expect(find.textContaining('2 个压缩包'), findsOneWidget);
    await tester.tap(find.text('继续解压'));
    await tester.pump();
    expect(rec.scanned.single, ['a/1.zip', 'a/2.7z']);
  });

  testWidgets('备注逐条显示', (tester) async {
    await pumpDone(tester, notes: ['密码像是对的但文件全坏', '这个包有 1/256 误判']);
    expect(find.text('· 密码像是对的但文件全坏'), findsOneWidget);
    expect(find.text('· 这个包有 1/256 误判'), findsOneWidget);
  });

  testWidgets('「接下来点这个」只在非校验模式显示', (tester) async {
    await pumpDone(tester, entryHint: '启动.exe');
    expect(find.textContaining('启动.exe'), findsOneWidget);

    await pumpDone(tester, verifyMode: true, entryHint: '启动.exe');
    expect(find.textContaining('启动.exe'), findsNothing,
        reason: '校验模式没有产物，指路等于骗人去点一个不存在的东西');
  });

  testWidgets('「再处理一个」回到空态', (tester) async {
    final rec = await pumpDone(tester);
    await tester.tap(find.text('再处理一个'));
    await tester.pump();
    expect(rec.resets, 1);
  });

  testWidgets('「打开目录」报的是产物目录', (tester) async {
    final rec = await pumpDone(tester, result: res(dest: '/tmp/out'));
    await tester.tap(find.text('打开目录'));
    await tester.pump();
    expect(rec.opened, ['/tmp/out']);
  });

  // ── T11 无障碍：键盘 ─────────────────────────────────────────────
  testWidgets('Enter = 打开目录（本屏主操作）', (tester) async {
    final rec = await pumpDone(tester, result: res(dest: '/tmp/out'));
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(rec.opened, ['/tmp/out'],
        reason: '解完最想干的事就是看产物，键盘按了没反应等于白做');
  });

  testWidgets('校验模式 Enter 不打开目录 —— 和按钮同一口径', (tester) async {
    final rec = await pumpDone(tester, verifyMode: true);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(rec.opened, isEmpty,
        reason: '校验模式根本没建目录，键盘却去打开 → 用户看到报错框，以为解压失败了');
  });

  testWidgets('Esc = 再处理一个', (tester) async {
    final rec = await pumpDone(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(rec.resets, 1, reason: 'Esc 没接上，键盘用户回了不到空态，只能去点鼠标');
  });
}
