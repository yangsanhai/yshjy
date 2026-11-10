// 「主题」下拉的回归（2026-11-05 真机反馈：切深色/浅色/跟随系统，界面都不变）。
//
// 钉两件事：
//   ① 切下拉 → `themeMode` **立刻**变（不用点保存）。`theme_mode.dart` 的头注释写的就是
//      「设置页切换后立刻生效，不用重启」；旁边的「界面语言」下拉也是立刻生效
//      （`onChanged` 里直接 `switchLocale`）。主题当初只在 `_save()` 里改值 → 切了看着没反应。
//   ② 点保存 → `themeMode` 同样要对上（这条是旧的唯一通路，别在修 ① 时弄坏它）。
//
// 注意：本文件真写配置文件，必须隔离跑：
//   XDG_CONFIG_HOME=/tmp/yshjy-test-config flutter test test/settings_theme_test.dart

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/pages/settings_page.dart';
import 'package:yshjy_gui/common/platform_paths.dart';
import 'package:yshjy_gui/common/theme_mode.dart';

import 'config_guard.dart';
import 'fake_engine.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 3600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  bool loaded() {
    final f = find.byKey(const ValueKey('settings-save'));
    if (f.evaluate().isEmpty) return false;
    return tester.widget<FloatingActionButton>(f).onPressed != null;
  }

  // 页面内部 `_load()` 读真文件 → 必须放进 runAsync（假时钟里 File.* 永不完成）
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

/// 点开「主题」下拉并选中某一项（文案用 zh-CN 的原文）
///
/// ⚠️ 不能用 `find.text('跟随系统（推荐）')` 去点：语言下拉也有同一句话，
/// 页面上一共有两个「跟随系统（推荐）」，`first` 会命中语言那一项。
/// 主题下拉是页面上第一个 `DropdownButtonFormField<String>`（语言是第二个）。
Future<void> _pickTheme(WidgetTester tester, String label) async {
  final dropdown = find.byType(DropdownButtonFormField<String>).first;
  await tester.tap(dropdown);
  await tester.pumpAndSettle();
  // 展开后选项在 overlay 里；同名文案可能同时存在（已选中项也画一份），取最后一个
  await tester.tap(find.text(label).last);
  await tester.pumpAndSettle();
}

void main() {
  FakeEngine? fake;

  setUpAll(() => requireIsolatedConfigDir('主题切换测试'));

  setUp(() async {
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);

    // 全局单例，测试之间会互相污染 —— 每条都归位
    themeMode.value = ThemeMode.system;

    final f = File(_cfgPath);
    if (f.existsSync()) f.deleteSync();
  });

  tearDownAll(restoreRealTransport);

  testWidgets('选「深色」→ themeMode 立刻变深色（不用点保存）', (tester) async {
    _writeConfig('[general]\ntheme = "auto"\n');
    await _pump(tester);
    expect(themeMode.value, ThemeMode.system, reason: '起始是跟随系统');

    await _pickTheme(tester, '深色');

    expect(themeMode.value, ThemeMode.dark,
        reason: '下拉一选中就该立刻生效 —— 只在 _save() 里改值的话，'
            '用户切完看不到任何变化，看起来像「主题功能坏了」');
  });

  testWidgets('深色 → 浅色 也能切回来', (tester) async {
    _writeConfig('[general]\ntheme = "dark"\n');
    await _pump(tester);
    expect(themeMode.value, ThemeMode.system,
        reason: '页面本身不读配置里的主题（main.dart 启动时读），这里只验下拉的联动');

    await _pickTheme(tester, '浅色');
    expect(themeMode.value, ThemeMode.light, reason: '切浅色要立刻生效');
  });

  testWidgets('点保存 → themeMode 与下拉一致（旧的唯一通路别弄坏）', (tester) async {
    _writeConfig('[general]\ntheme = "auto"\n');
    await _pump(tester);

    await _pickTheme(tester, '浅色');
    await tester.runAsync(() async {
      await tester.tap(find.byKey(const ValueKey('settings-save')));
      await Future<void>.delayed(const Duration(milliseconds: 50));
    });
    await tester.pump();

    expect(themeMode.value, ThemeMode.light, reason: '保存后仍要对得上');
    final written = File(_cfgPath).readAsStringSync();
    expect(written.contains('theme = "light"') || written.contains("theme = 'light'"), isTrue,
        reason: '配置里要真的写出 light，否则重启又回跟随系统');
  });
}
