// 解压进行中那一屏的回归 —— 按块拆分的第 6 块 B 段（进度条 / 状态提示 / 按钮行）。
//
// 这块原来是一个 115 行、直接读 14 个 State 字段的方法。拆成三段之后终于能分别验：
// 进度条算得对不对、四种提示该不该出现、按钮在批量/单个两种情况下该有几个。
//
// 两条来自真机验收的教训（写在断言理由里）：
// ① 暂停画面不许再挂速度（用户原话抓到「停都停了还显示 1.04 GB/s」）；
// ② 按钮点下去到真停之间有延迟，所以「正在暂停」和「已暂停」必须是两句不同的话。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/pages/home_progress_view.dart';

Progress prog({int done = 0, int total = 1000, int df = 0, int tf = 10}) =>
    Progress(doneBytes: done, totalBytes: total, doneFiles: df, totalFiles: tf);

Future<void> pumpAny(WidgetTester tester, Widget child) async {
  tester.view.physicalSize = const Size(1000, 900);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: Scaffold(body: child)));
  await tester.pump();
}

double? barValue(WidgetTester tester) =>
    tester.widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator)).value;

void main() {
  group('etaText', () {
    test('暂停时不报速度', () {
      expect(etaText(prog(done: 500), paused: true, speed: 1000), '',
          reason: '用户真机抓到过「停都停了还挂着 1.04 GB/s」，看着像没停住');
    });

    test('速度未知（<=0）时不报', () {
      expect(etaText(prog(done: 500), paused: false, speed: 0), '');
    });

    test('已经解完（剩余 0）时不报', () {
      expect(etaText(prog(done: 1000), paused: false, speed: 500), '');
    });

    test('剩余不到一分钟 → 「N 秒」', () {
      final s = etaText(prog(done: 900, total: 1000), paused: false, speed: 100);
      expect(s, contains('1 秒'), reason: '剩 100 字节 / 100 B/s = 1 秒');
    });

    test('剩余超过一分钟 → 「M 分 S 秒」', () {
      final s = etaText(prog(done: 0, total: 100000), paused: false, speed: 1000);
      expect(s, contains('分'), reason: '剩 100 秒要写成 1 分 40 秒，不能写成 100 秒');
    });

    test('总大小未知（0）时不报', () {
      expect(etaText(prog(total: 0), paused: false, speed: 100), '');
    });
  });

  group('ExtractProgressBar', () {
    testWidgets('有解压进度时用它的比例', (tester) async {
      await pumpAny(
        tester,
        ExtractProgressBar(
          progress: prog(done: 250, total: 1000),
          scanDone: 0,
          scanTotal: 0,
          current: 'a.txt',
          speed: 0,
          paused: false,
        ),
      );
      expect(barValue(tester), closeTo(0.25, 1e-9));
      expect(find.text('a.txt'), findsOneWidget);
    });

    testWidgets('扫描阶段没有进度时用扫描数字画条', (tester) async {
      await pumpAny(
        tester,
        const ExtractProgressBar(
          progress: null,
          scanDone: 500,
          scanTotal: 2000,
          current: null,
          speed: 0,
          paused: false,
        ),
      );
      expect(barValue(tester), closeTo(0.25, 1e-9),
          reason: '扫描阶段不给比例，用户会以为没在动');
      expect(find.textContaining('准备中'), findsOneWidget,
          reason: '没有当前文件时要说明「还没开始」，不能空着');
    });

    testWidgets('两个数字都没有 → 不确定进度条（null）', (tester) async {
      await pumpAny(
        tester,
        const ExtractProgressBar(
          progress: null,
          scanDone: 0,
          scanTotal: 0,
          current: null,
          speed: 0,
          paused: true,
        ),
      );
      expect(barValue(tester), isNull);
    });
  });

  group('ExtractStatusLines', () {
    testWidgets('什么都没发生：只剩步骤说明一行', (tester) async {
      await pumpAny(
        tester,
        const ExtractStatusLines(
          stage: 'WRITE',
          scanDone: 0,
          scanTotal: 0,
          pwTry: 0,
          pwTotal: 0,
          pauseRequested: false,
          paused: false,
        ),
      );
      expect(find.textContaining('第 3 步'), findsOneWidget);
      expect(find.textContaining('正在试第'), findsNothing);
      expect(find.textContaining('正在扫'), findsNothing);
    });

    testWidgets('三个阶段各说各的第几步', (tester) async {
      for (final (stage, key) in [('LOCATE', '第 1 步'), ('LIST', '第 2 步'), ('WRITE', '第 3 步')]) {
        await pumpAny(
          tester,
          ExtractStatusLines(
            stage: stage,
            scanDone: 0,
            scanTotal: 0,
            pwTry: 0,
            pwTotal: 0,
            pauseRequested: false,
            paused: false,
          ),
        );
        expect(find.textContaining(key), findsOneWidget, reason: '$stage 阶段应说 $key');
      }
    });

    testWidgets('正在试密码时说出来', (tester) async {
      await pumpAny(
        tester,
        const ExtractStatusLines(
          stage: 'WRITE',
          scanDone: 0,
          scanTotal: 0,
          pwTry: 2,
          pwTotal: 5,
          pauseRequested: false,
          paused: false,
        ),
      );
      expect(find.textContaining('2/5'), findsOneWidget,
          reason: '同时在试几个码，不说用户以为卡住了');
    });

    testWidgets('扫描中要说扫了多少', (tester) async {
      await pumpAny(
        tester,
        const ExtractStatusLines(
          stage: 'LOCATE',
          scanDone: 1024,
          scanTotal: 2048,
          pwTry: 0,
          pwTotal: 0,
          pauseRequested: false,
          paused: false,
        ),
      );
      expect(find.textContaining('正在扫'), findsOneWidget);
    });

    testWidgets('「正在暂停」和「已暂停」是两句话', (tester) async {
      await pumpAny(
        tester,
        const ExtractStatusLines(
          stage: 'WRITE',
          scanDone: 0,
          scanTotal: 0,
          pwTry: 0,
          pwTotal: 0,
          pauseRequested: true,
          paused: false,
        ),
      );
      expect(find.textContaining('正在暂停'), findsOneWidget);

      await pumpAny(
        tester,
        const ExtractStatusLines(
          stage: 'WRITE',
          scanDone: 0,
          scanTotal: 0,
          pwTry: 0,
          pwTotal: 0,
          pauseRequested: true,
          paused: true,
        ),
      );
      expect(find.textContaining('已暂停'), findsOneWidget,
          reason: '真停了还说「正在暂停」，用户会一直等');
    });
  });

  group('ExtractActionRow', () {
    Future<Map<String, int>> pumpRow(WidgetTester tester,
        {required int batchSize, bool paused = false, bool pauseRequested = false}) async {
      final hits = {'pause': 0, 'skip': 0, 'cancel': 0};
      await pumpAny(
        tester,
        ExtractActionRow(
          batchSize: batchSize,
          paused: paused,
          pauseRequested: pauseRequested,
          onPauseOrResume: () => hits['pause'] = hits['pause']! + 1,
          onSkipCurrent: () => hits['skip'] = hits['skip']! + 1,
          onCancel: () => hits['cancel'] = hits['cancel']! + 1,
        ),
      );
      return hits;
    }

    testWidgets('单个文件只有一个「取消」', (tester) async {
      await pumpRow(tester, batchSize: 1);
      expect(find.text('取消'), findsOneWidget);
      expect(find.text('全部暂停'), findsNothing,
          reason: '单个文件本来就快跑完了，暂停只会让人以为卡住');
      expect(find.text('跳过这项'), findsNothing,
          reason: '跳过单个和取消整批是一回事，多一个按钮更乱');
    });

    testWidgets('批量时三个按钮都在，取消改叫「取消整批」', (tester) async {
      final hits = await pumpRow(tester, batchSize: 3);
      expect(find.text('全部暂停'), findsOneWidget);
      expect(find.text('跳过这项'), findsOneWidget);
      expect(find.text('取消整批'), findsOneWidget);
      await tester.tap(find.text('取消整批'));
      await tester.pump();
      expect(hits['cancel'], 1);
    });

    testWidgets('已暂停时按钮变「继续」', (tester) async {
      await pumpRow(tester, batchSize: 3, paused: true);
      expect(find.text('继续'), findsOneWidget);
      expect(find.text('全部暂停'), findsNothing);
    });

    testWidgets('暂停请求发出后按钮禁用（防连点）', (tester) async {
      await pumpRow(tester, batchSize: 3, pauseRequested: true);
      final btn = tester.widget<OutlinedButton>(find.ancestor(
        of: find.text('全部暂停'),
        matching: find.byType(OutlinedButton),
      ));
      expect(btn.onPressed, isNull,
          reason: '点了暂停还在等真停，这时还能再点就会重复发请求');
    });
  });
}
