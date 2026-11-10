// 冒烟测试：外壳能构建出来、五个导航项都在
//
// 不真的启动引擎（测试环境没有 yshjy 可执行文件），只看界面骨架。
// 2026-11-01 补：设置页 T17-1 改成"进页面就向引擎要格式表"，而 AppShell 的
// IndexedStack 会把五个页面都构建一遍 → 不给假引擎的话，这里会去真的 spawn
// 子进程（然后 dispose 时撞「ProcessTransport 还没 start()」）。
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/app_shell.dart';

import 'fake_engine.dart';

void main() {
  FakeEngine? fake;

  setUp(() async {
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);
  });

  tearDownAll(restoreRealTransport);

  testWidgets('导航栏有五个入口', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: AppShell(bootstrapEngine: false)));
    await tester.pump();
    for (final label in ['解压', '打包', '历史', '设置', '关于']) {
      expect(find.text(label), findsWidgets, reason: '缺少导航项 $label');
    }
  });

  // 梯队四·首屏单一入口收口
  //
  // 新手打开最怕「一堆按钮不知道该点哪个」。规矩是：首屏只能有一个最强入口
  // （FilledButton = 选择文件），其余入口必须降级成文字按钮。
  // 这条以前只在注释里写着（调研 07:921-923），现在用测试钉住 ——
  // 免得以后又顺手加个 FilledButton 把首屏搞乱。
  testWidgets('首屏只有一个最强入口', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: AppShell(bootstrapEngine: false)));
    await tester.pump();

    expect(
      find.widgetWithText(FilledButton, '选择文件'),
      findsOneWidget,
      reason: '首屏该有「选择文件」这个主按钮',
    );
    expect(
      find.byType(FilledButton),
      findsOneWidget,
      reason: '首屏 FilledButton 超过一个，新手就不知道该点哪个了',
    );
    expect(
      find.widgetWithText(TextButton, '选整个文件夹'),
      findsOneWidget,
      reason: '次入口（选文件夹）要弱化成文字按钮，不许跟主入口抢',
    );
  });
}
