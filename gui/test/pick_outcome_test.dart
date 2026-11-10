// 批 7d：文件挑选的三种走法（取消 / 单选 / 多选）
//
// 以前这段逻辑长在 `_pick()` 里，跟「弹系统对话框」搅在一起 —— widget 测试弹不出
// 系统对话框，于是「取消会不会误报错」「单选是不是真走预览」只能靠手点。
// 现在对话框可注入（`HomePage(pickFiles: ...)`），判断是纯函数 `pickOutcomeOf`。
//
// 反向验证（改坏看红，2026-11-01 实测）：
//   把 `if (paths.isEmpty) return PickOutcome.none;` 删掉 →
//   `pickOutcomeOf([])` 变成 queue / 抛错，第 1 条立刻红（取消会被当成「挑了 0 个包」）。

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';
import 'package:yshjy_gui/desktop/pages/home_pick.dart';

void main() {
  group('pickOutcomeOf（纯函数）', () {
    test('用户取消（空列表）→ none，什么都不做', () {
      expect(pickOutcomeOf(const []), PickOutcome.none,
          reason: '取消不是错误 —— 弹「出错了」会让用户以为程序坏了');
    });

    test('单选 → 直接预览（省一次点击）', () {
      expect(pickOutcomeOf(const ['/home/me/下载/样本.mp4']), PickOutcome.preview);
    });

    test('多选 → 进队列（网盘一次下 8 个是常态）', () {
      expect(pickOutcomeOf(const ['/a.zip', '/b.zip', '/c.zip']), PickOutcome.queue);
    });

    test('两个也算多选（边界不能写成 >= 3）', () {
      expect(pickOutcomeOf(const ['/a.zip', '/b.zip']), PickOutcome.queue);
    });

    test('中文与空格路径不影响判断（只看个数）', () {
      expect(pickOutcomeOf(const ['/home/me/下载/我的 包.zip']), PickOutcome.preview);
    });
  });

  group('注入到页面（对话框不真弹）', () {
    setUp(() => Notifier.enabled = false);

    testWidgets('多选 → 走 scan，队列出现两条', (tester) async {
      final fake = _FakeApi();
      tester.view.physicalSize = const Size(1100, 1700);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);

      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: HomePage(
            engine: fake,
            autoStart: false,
            // 假装用户在对话框里挑了两个包
            pickFiles: () async => ['/tmp/甲.zip', '/tmp/乙.zip'],
          ),
        ),
      ));
      await tester.pumpAndSettle();

      // 空态只有一个 FilledButton（有测试钉着），点它就是「选文件」
      await tester.tap(find.byType(FilledButton));
      for (var i = 0; i < 10; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }

      final scanned = fake.lastArgs('scan');
      expect(scanned, isNotNull, reason: '多选必须走 scan（批量），不是逐个 list');
      expect(scanned!['paths'], ['/tmp/甲.zip', '/tmp/乙.zip']);
      expect(find.textContaining('甲.zip'), findsWidgets);
    });

    testWidgets('取消 → 一条命令都不发，界面还停在空态', (tester) async {
      final fake = _FakeApi();
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: HomePage(
            engine: fake,
            autoStart: false,
            pickFiles: () async => const [],
          ),
        ),
      ));
      await tester.pumpAndSettle();

      await tester.tap(find.byType(FilledButton));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }

      expect(fake.calls, isEmpty,
          reason: '取消后既不该 scan 也不该 list，更不该弹错误提示');
      expect(find.byKey(const ValueKey('empty')), findsOneWidget);
    });
  });
}

/// 只记录 list/scan 的极简假引擎
class _FakeApi implements EngineApi {
  final List<String> calls = [];
  final Map<String, Map<String, dynamic>> _args = {};
  final _events = StreamController<EngineEvent>.broadcast();

  Map<String, dynamic>? lastArgs(String cmd) => _args[cmd];

  @override
  Stream<EngineEvent> get events => _events.stream;

  @override
  bool get busy => false;

  @override
  Future<void> start() async {}

  @override
  Future<Map<String, dynamic>> scan(List<String> paths) async {
    calls.add('scan');
    _args['scan'] = {'paths': paths};
    return {
      'items': [
        for (final p in paths)
          {
            'path': p,
            'name': p.split('/').last,
            'size': 1024,
            'kind': 'ZIP',
            'volumes': 1,
            'archive': true,
          },
      ],
    };
  }

  @override
  Future<Preview> list(String path, {String? password}) async {
    calls.add('list');
    _args['list'] = {'path': path};
    return Preview(
      format: 'zip',
      complete: true,
      totalSize: 10,
      encryptedCount: 0,
      encKind: null,
      entries: const [],
      hints: const [],
    );
  }

  @override
  void dispose() {}

  @override
  noSuchMethod(Invocation invocation) => throw UnimplementedError(
      '本文件只验挑文件，用到了 ${invocation.memberName} 说明测试写歪了');
}
