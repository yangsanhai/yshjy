// S10 多候选的 widget 回归（T5）。
//
// 为什么非要 widget 测试：这段界面的判断是「什么时候**不**显示」——
// 绝大多数包只有一个候选，要是条件写反了，每个包都会多出一句提示 + 一个按钮，
// 用户以为自己的包有问题。而真机手点验不出「不该出现的没出现」，
// 只有测试能钉住这条反向断言。
//
// 注入的是**引擎原样输出的 scan items**（不是界面自己造的模型），
// 所以「解析 → 渲染」整条链都在这几条测试里。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';

/// 一份「一个文件里两处像归档」的 scan item（数字取自 2026-10-30 探针实测）
Map<String, dynamic> multiCandidateItem() => {
      'path': '/tmp/藏两个.mp4',
      'name': '藏两个.mp4',
      'size': 27262976,
      'kind': 'Zip',
      'volumes': 1,
      'looks_like': 'Text',
      'mime': 'application/zip',
      'archive': true,
      'candidates': [
        {
          'format': 'zip',
          'offset': 3145728,
          'confidence': 'high',
          'note': 'LFH+CD ok',
          'chosen': false,
        },
        {
          'format': 'zip',
          'offset': 6291581,
          'confidence': 'high',
          'note': 'located',
          'chosen': true,
        },
      ],
      'seek': 'full',
      'seek_label': '可以直接读',
    };

/// 普通包：只有一条候选
Map<String, dynamic> singleCandidateItem() => {
      'path': '/tmp/普通.zip',
      'name': '普通.zip',
      'size': 3072,
      'kind': 'Zip',
      'volumes': 1,
      'looks_like': 'Zip',
      'mime': 'application/zip',
      'archive': true,
      'candidates': [
        {
          'format': 'zip',
          'offset': 0,
          'confidence': 'high',
          'note': 'located',
          'chosen': true,
        },
      ],
      'seek': 'full',
      'seek_label': '可以直接读',
    };

/// 老引擎（没这个字段）：界面必须装作不知道，不许崩
Map<String, dynamic> legacyItem() => {
      'path': '/tmp/老引擎.zip',
      'name': '老引擎.zip',
      'size': 1024,
      'kind': 'Zip',
      'volumes': 1,
      'looks_like': 'Zip',
      'mime': 'application/zip',
      'archive': true,
      'seek': 'full',
      'seek_label': '可以直接读',
    };

Future<void> pumpQueue(WidgetTester tester, List<Map<String, dynamic>> items) async {
  tester.view.physicalSize = const Size(1200, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(autoStart: false, debugScanItems: items),
    ),
  ));
  await tester.pump();
}

void main() {
  group('S10 多候选：什么时候才该打扰用户', () {
    testWidgets('一个文件里两处 → 出提示 + 「换个地方试」', (tester) async {
      await pumpQueue(tester, [multiCandidateItem()]);
      expect(find.textContaining('像是有 2 处藏着东西'), findsOneWidget,
          reason: '两处候选却不告诉用户，等于这个功能没做');
      expect(find.widgetWithText(TextButton, '换个地方试'), findsOneWidget,
          reason: '没有这个按钮，用户就只能放弃');
    });

    testWidgets('普通包（只有一条候选）→ 一个字都不许多说', (tester) async {
      await pumpQueue(tester, [singleCandidateItem()]);
      expect(find.textContaining('处藏着东西'), findsNothing,
          reason: '正常包弹这句，用户会以为自己的包有问题');
      expect(find.text('换个地方试'), findsNothing);
    });

    testWidgets('老引擎没 candidates 字段 → 不崩也不显示', (tester) async {
      await pumpQueue(tester, [legacyItem()]);
      expect(find.textContaining('处藏着东西'), findsNothing);
      expect(find.text('老引擎.zip'), findsOneWidget);
    });

    testWidgets('一批里只有多候选那行才显示，别的行不受影响', (tester) async {
      await pumpQueue(tester, [singleCandidateItem(), multiCandidateItem()]);
      // 只有一处提示：不能因为队列里有一个多候选，就给所有行都加上
      expect(find.textContaining('处藏着东西'), findsOneWidget);
      expect(find.text('换个地方试'), findsOneWidget);
    });
  });

  group('候选的人话文案', () {
    test('偏移 → 人话（字节数对新手没意义）', () {
      const c = ArchiveCandidate(
          format: 'zip', offset: 3145728, confidence: 'high', chosen: false);
      expect(c.offsetText, contains('3.0 MB'));
      const none = ArchiveCandidate(
          format: 'zip', offset: 0, confidence: 'high', chosen: true);
      expect(none.offsetText, '文件开头');
    });

    test('三档置信度都有对应的中文', () {
      String t(String c) => ArchiveCandidate(
              format: 'zip', offset: 1, confidence: c, chosen: false)
          .confidenceText;
      expect(t('high'), '可信度高');
      expect(t('medium'), '还算靠得住');
      expect(t('low'), '可能是误报');
    });

    test('引擎给的 chosen 解析成 bool（不是字符串判断出错）', () {
      final l = EngineClient.candidatesOf(multiCandidateItem());
      expect(l.length, 2);
      expect(l.where((c) => c.chosen).length, 1);
      expect(l.first.chosen, isFalse);
      expect(l.last.chosen, isTrue);
    });
  });
}
