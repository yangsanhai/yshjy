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

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/settings_page.dart';
import 'package:yshjy_gui/platform_paths.dart';

import 'config_guard.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 3600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  // 页面内部 `_load()` 要读真文件 → 整段放进 runAsync（假时钟里 File.* 永不完成）
  await tester.runAsync(() async {
    await tester.pumpWidget(const MaterialApp(home: Scaffold(body: SettingsPage())));
    await Future<void>.delayed(const Duration(milliseconds: 150));
  });
  await tester.pump();
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

    // 「跟随系统（推荐）」= 认不出的值被规整成了 system（主题那一项也是这句，所以 findsWidgets）
    expect(find.text('跟随系统（推荐）'), findsWidgets,
        reason: '认不出的语言值必须回落到「跟随系统」，不能让界面停在未知状态');
  });

  testWidgets('点保存 → 写出 ui_language，且同段其它键与注释原样保留', (tester) async {
    _writeConfig('[general]\n'
        '# 用户手写的注释也要留着\n'
        'ui_language = "ja-JP"\n'
        'theme = "dark"\n'
        'close_action = "tray"\n');
    await _pump(tester);

    await tester.runAsync(() async {
      await tester.tap(find.widgetWithText(FilledButton, '保存'));
      await Future<void>.delayed(const Duration(milliseconds: 150));
    });
    await tester.pump();

    final text = File(_cfgPath).readAsStringSync();
    expect(text.contains('ui_language = "ja-JP"'), isTrue, reason: '保存后要写回语言：\n$text');
    expect(text.contains('# 用户手写的注释也要留着'), isTrue, reason: '注释不能被冲掉：\n$text');
    expect(text.contains('theme = "dark"'), isTrue, reason: '同段其它键不能被冲掉：\n$text');
    expect(text.contains('close_action = "tray"'), isTrue, reason: '同段其它键不能被冲掉：\n$text');
  });
}
