// 「界面语言」下拉的回归（T14 第三批）。
//
// 钉三件事：
//   ① 配置里写了 `ui_language = "en-US"` → 下拉显示 English（真读回来了）
//   ② 配置里写坏了（`klingon`）→ 当「跟随系统」，界面不能变成未知状态
//   ③ 点保存 → `[general]` 段里写出 `ui_language`，且**同段用户手写的键/注释一个字节都不许被冲掉**
//
// ⚠️ 两个坑（本文件第一版就是这么挂掉 30 分钟超时的）：
//   · 文件操作**一律用同步 API**（`createSync` / `writeAsStringSync` / `readAsStringSync`）。
//     `testWidgets` 的函数体跑在假时钟里，`await File(...).writeAsString()` 这类真 I/O
//     的 Future **永远不完成** → 挂到 10 分钟超时才报错，症状看起来像界面卡死。
//   · 点「保存」必须包在 `tester.runAsync` 里（它走真实文件写入，同理）。
//
// 注意：这个测试会真写配置文件，必须隔离：
//   XDG_CONFIG_HOME=/tmp/yshjy-test-config flutter test

import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/l10n.dart';
import 'package:yshjy_gui/pages/settings_page.dart';
import 'package:yshjy_gui/platform_paths.dart';

import 'config_guard.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 3600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  // 页面内部 `_load()` 要读真文件 → 整段放进 runAsync（假时钟里 File.* 永不完成）。
  // 等 300ms 而不是 150ms：机器忙时读配置 + 首帧构建可能超过 150ms，
  // 曾出现过「偶发第一条红、重跑就绿」的 flaky（那是等待不够，不是业务问题）。
  await tester.runAsync(() async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: SettingsPage())));
    await Future<void>.delayed(const Duration(milliseconds: 300));
  });
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 50));
}

void _writeConfig(String text) {
  File(_cfgPath)
    ..createSync(recursive: true)
    ..writeAsStringSync(text);
}

void main() {
  setUpAll(() => requireIsolatedConfigDir('界面语言测试'));

  setUp(() {
    final f = File(_cfgPath);
    if (f.existsSync()) f.deleteSync();
  });

  testWidgets('配置写了 en-US → 下拉显示 English', (tester) async {
    _writeConfig('[general]\nui_language = "en-US"\n');
    await _pump(tester);

    expect(find.text('English'), findsWidgets,
        reason: '配置里是 en-US，下拉应当显示 English（说明真的读回来了）');
  });

  testWidgets('配置写坏了（klingon）→ 当跟随系统', (tester) async {
    _writeConfig('[general]\nui_language = "klingon"\n');
    await _pump(tester);

    // 「跟随系统（推荐）」= 认不出的值被规整成了 system（主题那一项也是这句话，所以 findsWidgets）
    expect(find.text('跟随系统（推荐）'), findsWidgets,
        reason: '认不出的语言值必须回落到「跟随系统」，不能让界面停在未知状态');
  });

  testWidgets('配置写了还没翻完的语言（ja-JP）→ 当跟随系统，下拉不出现日本語', (tester) async {
    // 之前这里是个真 bug：`switchLocale` 读不到语言包就「保持现状」，
    // 用户在英文界面点「日本語」→ 界面一直是英文 → 看着像切换失灵。
    // 现在没翻完的语言根本不进下拉，配置里残留也会被当成「跟随系统」。
    _writeConfig('[general]\nui_language = "ja-JP"\n');
    await _pump(tester);

    expect(find.text('日本語'), findsNothing,
        reason: 'ja-JP 还没翻完，下拉里不该出现它 —— 选了只会回落中文');
    expect(find.text('跟随系统（推荐）'), findsWidgets,
        reason: '配置里的 ja-JP 要当成「跟随系统」');
  });

  testWidgets('点保存 → 写出 ui_language，且同段其它键与注释原样保留', (tester) async {
    _writeConfig('[general]\n'
        '# 用户手写的注释也要留着\n'
        'ui_language = "en-US"\n'
        'theme = "dark"\n'
        'close_action = "tray"\n');
    await _pump(tester);

    await tester.runAsync(() async {
      await tester.tap(find.widgetWithText(FilledButton, '保存'));
      await Future<void>.delayed(const Duration(milliseconds: 150));
    });
    await tester.pump();

    final text = File(_cfgPath).readAsStringSync();
    // 用 en-US 而不是 ja-JP：ja-JP 还没翻完，会被当成「跟随系统」写成 system，
    // 那测的就不是「保存」而是「回落」了。
    expect(text.contains('ui_language = "en-US"'), isTrue, reason: '保存后要写回语言：\n$text');
    expect(text.contains('# 用户手写的注释也要留着'), isTrue, reason: '注释不能被冲掉：\n$text');
    expect(text.contains('theme = "dark"'), isTrue, reason: '同段其它键不能被冲掉：\n$text');
    expect(text.contains('close_action = "tray"'), isTrue, reason: '同段其它键不能被冲掉：\n$text');
  });

  testWidgets('英文 + 真机窄窗口（576 宽）不溢出', (tester) async {
    // 真机 576×607 时用户在截图里撞到过 `RIGHT OVERFLOWED BY 40 PIXELS`：
    // 「遇到同名文件 / 解压完原包」两个下拉原本并排，英文 label 一长就把右边挤出去。
    // 这条测试按真机宽度渲染整页 —— 任何 RenderFlex overflow 都会作为异常抛出来。
    _writeConfig('[general]\nui_language = "en-US"\n');
    final raw = File('assets/l10n/en-US.json').readAsStringSync();
    final parsed = jsonDecode(raw) as Map<String, dynamic>;
    installTables(
      table: {
        for (final e in parsed.entries)
          if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
      },
      locale: 'en-US',
    );
    addTearDown(() {
      // 装回基准语言，别把英文表留给后面的测试
      final zhRaw = File('assets/l10n/zh-CN.json').readAsStringSync();
      final zhParsed = jsonDecode(zhRaw) as Map<String, dynamic>;
      installTables(table: {
        for (final e in zhParsed.entries)
          if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
      });
    });

    tester.view.physicalSize = const Size(576, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.runAsync(() async {
      await tester.pumpWidget(const MaterialApp(home: Scaffold(body: SettingsPage())));
      await Future<void>.delayed(const Duration(milliseconds: 300));
    });
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));

    // ⚠️ 必须滚到那两个下拉的位置再断言：`ListView` 是**懒加载**的，不滚到那一带
    // 那两行根本不 build，溢出也就不会发生 —— 第一版没滚动，320 宽都「通过」，
    // 是条假护栏。反向验证过：把两个下拉改回并排 Row，这条会红。
    await tester.scrollUntilVisible(
      find.text(t('settings.conflict.rename')),
      120,
      // 页面里不止一个 Scrollable（输入框内部也有），不指定外层就会
      // `Bad state: Too many elements`。取第一个 = 最外层 ListView。
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pump();

    expect(tester.takeException(), isNull,
        reason: '真机 576 宽 + 英文曾溢出 40px —— 别把并排改回来，也别去掉 isExpanded');
  });
}
