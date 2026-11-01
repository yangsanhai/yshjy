// 解压码输入区的独立回归 —— 按块拆分的第 5d-2 第 1 段。
//
// 这块原来是 `_previewView` build 里的一段 spread（51 行），要靠整页 pump + 造加密包
// 才能验。抽成组件后，一次只验一件事：什么时候显示、三种输入方式、trim 规则。
//
// 原页面级回归仍在 `password_box_widget_test.dart`（那条腿保证「真页面上还在原位」），
// 这条保证「组件自己的边界都对」—— 两条互补，别互相替代。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/preview_password_box.dart';

class Rec {
  final changed = <String?>[];
  final picked = <String>[];
  int toggles = 0;
  int submitted = 0;
}

Future<Rec> pumpBox(
  WidgetTester tester, {
  int encryptedCount = 3,
  bool showPlain = false,
  List<String> recent = const [],
  bool withSubmit = false,
}) async {
  final rec = Rec();
  final ctrl = TextEditingController();
  addTearDown(ctrl.dispose);
  tester.view.physicalSize = const Size(900, 1200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: PreviewPasswordBox(
        encryptedCount: encryptedCount,
        controller: ctrl,
        showPlain: showPlain,
        recent: recent,
        onToggleShowPlain: () => rec.toggles++,
        onChanged: rec.changed.add,
        onPickRecent: rec.picked.add,
        onSubmitted: withSubmit ? () => rec.submitted++ : null,
      ),
    ),
  ));
  await tester.pump();
  return rec;
}

void main() {
  testWidgets('没有加密条目时整块不显示（父层不用再写 if）', (tester) async {
    await pumpBox(tester, encryptedCount: 0);
    expect(find.byType(TextField), findsNothing,
        reason: '没加密却摆一个密码框，用户会以为这个包坏了');
    expect(find.byType(ActionChip), findsNothing);
  });

  testWidgets('有加密条目才显示，标题写明几个文件加密', (tester) async {
    await pumpBox(tester, encryptedCount: 3);
    expect(find.byType(TextField), findsOneWidget);
    expect(find.textContaining('3'), findsWidgets,
        reason: '标题必须带具体数量 —— 「这个包有 N 个文件加密」');
  });

  testWidgets('眼睛按钮切换明文，只是报名不自己改状态', (tester) async {
    final rec = await pumpBox(tester);
    expect(find.byTooltip('显示解压码'), findsOneWidget,
        reason: '默认是密文，所以按钮应该叫「显示解压码」');
    await tester.tap(find.byTooltip('显示解压码'));
    await tester.pump();
    expect(rec.toggles, 1);

    // 已经是明文时，按钮该翻成「隐藏解压码」
    await pumpBox(tester, showPlain: true);
    expect(find.byTooltip('隐藏解压码'), findsOneWidget,
        reason: '明文了还写「显示解压码」，用户点了以为没反应');
  });

  testWidgets('输入会 trim，空白输入报 null 而不是空串', (tester) async {
    final rec = await pumpBox(tester);
    await tester.enterText(find.byType(TextField), '  abc123  ');
    await tester.pump();
    expect(rec.changed.last, 'abc123',
        reason: '不 trim 的话用户复制进来的尾随空格会让密码永远不对');

    await tester.enterText(find.byType(TextField), '   ');
    await tester.pump();
    expect(rec.changed.last, isNull,
        reason: '全是空格应该当成没填（引擎侧收 null 才走「没给密码」那条路）');
  });

  testWidgets('密码本有常用码时列成 chips，点一下报那个码', (tester) async {
    final rec = await pumpBox(tester, recent: ['123456', 'yshjy']);
    expect(find.byType(ActionChip), findsNWidgets(2));
    await tester.tap(find.text('123456'));
    await tester.pump();
    expect(rec.picked, ['123456'],
        reason: '点了 chip 要报给父层去改 controller —— 自己改就是两份真相');
  });

  testWidgets('密码本为空时不占地方', (tester) async {
    await pumpBox(tester);
    expect(find.byType(ActionChip), findsNothing);
  });

  // ── T11 无障碍：键盘 ─────────────────────────────────────────────
  testWidgets('输完密码按回车 = 提交（UI-SPEC §4b.6「Enter 确认」）', (tester) async {
    final rec = await pumpBox(tester, withSubmit: true);
    await tester.enterText(find.byType(TextField), 'abc123');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    expect(rec.submitted, 1,
        reason: '报错页那个重试框早就能回车提交，预览页这个框没反应 → 用户以为「这里坏了」');
  });

  testWidgets('父层没给提交回调时，回车什么都不做（不报错）', (tester) async {
    final rec = await pumpBox(tester); // withSubmit: false
    await tester.enterText(find.byType(TextField), 'abc123');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    expect(rec.submitted, 0);
    expect(rec.changed, contains('abc123'),
        reason: '没接提交也不该把输入丢掉 —— 输入变化照旧要报给父层');
  });
}
