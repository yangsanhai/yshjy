// 「键位写在界面上」（T11 的可见部分）
//
// 为什么单独验：键盘接好了但没人知道有这个键，等于没做。用户原话：
// 「记得这些按键在相关页面写上不然快捷按键别人也不知道」。
//
// 两条腿：
//   ① 一张**只读**快捷键表（行来自 `lib/keyboard.dart` 的 keyHelpRows，
//      和 keyActionFor 同一份真相 —— 改键时说明自动跟着变）
//      ⚠️ 2026-11-05 起这张表在**关于页**，不在设置页（用户要求：那些不是设置）。
//      表本身的显示断言在 test/about_tables_test.dart，这里只校验数据源与翻译。
//   ② 主按钮上就地写着键位（悬停可见），用户不用为了找一个键跑去关于页
//
// 参考项目：RustDesk 设置页有「Keyboard shortcuts」卡片（可重映射 → 进独立配置页）；
// LocalSend / Keka / FileRoller 设置里没有快捷键页。我们键位固定，做成只读表最省一步。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/keyboard.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/desktop/pages/home_done_view.dart';
import 'package:yshjy_gui/desktop/pages/home_empty_view.dart';
import 'package:yshjy_gui/desktop/pages/home_progress_view.dart';
import 'package:yshjy_gui/desktop/pages/settings_page.dart';

import 'fake_engine.dart';

void main() {
  tearDownAll(restoreRealTransport);

  test('keyHelpRows 每一行都能翻译成人话（不是把键名原样显示出来）', () {
    // 表里 6 行，和 keyActionFor 支持的动作一一对应
    expect(keyHelpRows.map((r) => r.keys).toList(),
        ['Enter', 'Esc', 'Ctrl+E', 'Ctrl+A', 'Ctrl+V', 'Del'],
        reason: '键位表被改动时这条会红 —— 提醒你同步 UI-SPEC 和 CHANGELOG');
    for (final r in keyHelpRows) {
      final text = t(r.whatKey);
      expect(text, isNot(r.whatKey),
          reason: '${r.whatKey} 没翻译（显示成了键名本身），用户看到的是 '
              'keys.primary 这种字样');
      expect(text.trim(), isNotEmpty);
    }
    // 说明文字也在
    expect(t('keys.note'), isNot('keys.note'));
    expect(t('settings.section.keys'), '快捷键');
  });

  // 快捷键表本身在关于页 —— 显示断言见 test/about_tables_test.dart。
  // 这条只留设置页自己的就地提示（T11 收尾：打包/设置两页接 Enter 时一起写的）。
  testWidgets('设置页保存按钮上写着 Enter', (tester) async {
    final fake = FakeEngine();
    installFakeEngine(fake);
    // 和 settings_perf_widget_test 同样的隔离方式：视口给足（ListView 只构建可视区）
    tester.view.physicalSize = const Size(1200, 5200);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    // 结束条件：保存按钮可用（页面配置读完）。格式表搬走后不能再用「zip 出现」当信号。
    bool ready() {
      final f = find.byKey(const ValueKey('settings-save'));
      if (f.evaluate().isEmpty) return false;
      return tester.widget<FloatingActionButton>(f).onPressed != null;
    }

    await tester.runAsync(() async {
      await tester.pumpWidget(
          const MaterialApp(home: Scaffold(body: SettingsPage())));
      for (var i = 0; i < 120 && !ready(); i++) {
        await Future<void>.delayed(const Duration(milliseconds: 25));
        await tester.pump();
      }
    });
    await tester.pump();

    expect(find.byTooltip('Enter'), findsWidgets,
        reason: '设置页保存按钮没标 Enter');
  });

  testWidgets('主按钮上有就地键位提示（悬停可见）', (tester) async {
    tester.view.physicalSize = const Size(1000, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    // 第一屏：选文件 = Enter
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: EmptyDropView(
          dragging: false,
          onPickFile: () {},
          onPickFolder: () {},
          onPastePath: () {},
        ),
      ),
    ));
    expect(find.byTooltip('Enter'), findsOneWidget,
        reason: '第一屏的主按钮没写键位，用户不会知道按 Enter 就能选文件');

    // 完成页：打开目录 = Enter
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: DoneView(
          result: ExtractResult(
            outcome: 'Success',
            entriesDone: 2,
            entriesTotal: 2,
            bytesWritten: 100,
            dest: '/tmp/out',
            millis: 10,
          ),
          verifyMode: false,
          nestedArchives: const [],
          notes: const [],
          fileName: 'a.zip',
          password: null,
          entryHint: null,
          onScanNested: (_) {},
          onOpenFolder: (_) {},
          onReset: () {},
        ),
      ),
    ));
    await tester.pump();
    expect(find.byTooltip('Enter'), findsOneWidget);
  });

  testWidgets('解压中的「取消」写着 Esc，暂停/跳过没有乱标键位', (tester) async {
    tester.view.physicalSize = const Size(1000, 1200);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: ExtractActionRow(
          batchSize: 3,
          paused: false,
          pauseRequested: false,
          onPauseOrResume: () {},
          onSkipCurrent: () {},
          onCancel: () {},
        ),
      ),
    ));
    expect(find.byTooltip('Esc'), findsOneWidget,
        reason: '取消没写 Esc，用户只能去点按钮');
    // 暂停/跳过在 UI-SPEC 里没有键位 —— 不许自己拍，也就不许标
    expect(find.byTooltip('Space'), findsNothing);
    expect(find.byTooltip('Ctrl+S'), findsNothing);
  });
}
