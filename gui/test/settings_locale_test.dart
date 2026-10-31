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
import 'fake_engine.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 3600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  // 页面内部 `_load()` 要读真文件 → 整段放进 runAsync（假时钟里 File.* 永不完成）。
  // 等待方式：轮询到「保存按钮可用」= 配置真读完了。以前睡固定 300ms，
  // 全量并发跑时机器忙就偶发红（那条 flaky 的根因就是它，不是业务问题）。
  bool loaded() {
    final f = find.widgetWithText(FilledButton, '保存');
    if (f.evaluate().isEmpty) return false;
    return tester.widget<FilledButton>(f).onPressed != null;
  }

  await tester.runAsync(() async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: SettingsPage())));
    for (var i = 0; i < 120 && !loaded(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
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
  FakeEngine? fake;

  setUpAll(() => requireIsolatedConfigDir('界面语言测试'));

  setUp(() async {
    // 设置页 `initState` 会问引擎（格式表 + 磁盘类型）—— 换掉真子进程
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);

    final f = File(_cfgPath);
    if (f.existsSync()) f.deleteSync();
  });

  tearDownAll(restoreRealTransport);

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

  testWidgets('配置写了根本不存在的语言（xx-XX）→ 当跟随系统', (tester) async {
    // 之前这里是个真 bug：`switchLocale` 读不到语言包就「保持现状」，
    // 用户在英文界面点「日本語」→ 界面一直是英文 → 看着像切换失灵。
    // 现在没有语言包的语言不进下拉（[availableLocales]），配置里残留也当成「跟随系统」。
    // 这条原本拿 ja-JP 当例子，后来七种语言全翻完了、ja-JP 已经真生效，
    // 所以换成不存在的代码 —— 测的是「认不出就回落」这个机制本身。
    _writeConfig('[general]\nui_language = "xx-XX"\n');
    await _pump(tester);

    expect(find.text('跟随系统（推荐）'), findsWidgets,
        reason: '配置里的未知语言要当成「跟随系统」');
  });

  testWidgets('七种语言都翻完了 → 下拉里七种名字都在', (tester) async {
    // 和 l10n_test 的双向校验互补：那边看「有包 ⇔ 在 availableLocales」，
    // 这边看用户真正能看到的下拉里有没有这七项。
    _writeConfig('[general]\nui_language = "system"\n');
    await _pump(tester);

    // 下拉未展开时，选项在 IndexedStack 里是 offstage 的 ——
    // find.text 默认 skipOffstage: true 会一个都找不到（那样这条测试就是假绿），
    // 必须显式 skipOffstage: false 才是在验证「选项真的存在」。
    for (final name in ['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Español', 'Русский']) {
      expect(find.text(name, skipOffstage: false), findsWidgets,
          reason: '「$name」应该出现在界面语言下拉里');
    }
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
