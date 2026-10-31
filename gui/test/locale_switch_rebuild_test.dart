import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/l10n.dart';
import 'package:yshjy_gui/pages/history_page.dart';

/// 这一组测的是「切了界面语言，页面到底有没有跟着变」。
///
/// 起因是真机 bug（用户 2026-10-31 截图）：英文界面下导航栏是 Extract/Pack/History，
/// 但**历史页内容还是中文**（「历史记录 0条」「还没处理过文件」），底部状态栏也还写着「就绪」。
/// 同一个窗口一半英文一半中文。根因两处：
///   ① `const HistoryPage()` / `const SettingsPage()`：父级 rebuild 时 Flutter 判定
///      「同一个 const 实例 = 没变化」，直接跳过子树重建，页面内容停在首帧语言。
///   ② `String _status = t('shell.status.ready')`：字段初始化只在 State 创建时求值一次。
Map<String, String> readPack(String code) {
  final raw = File('assets/l10n/$code.json').readAsStringSync();
  final m = jsonDecode(raw) as Map<String, dynamic>;
  return {
    for (final e in m.entries)
      if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
  };
}

void main() {
  testWidgets('切界面语言后，历史页内容跟着变（不能停在首帧的语言）', (tester) async {
    final zh = readPack('zh-CN');
    final en = readPack('en-US');

    installTables(table: zh, base: zh, locale: 'zh-CN');
    // 这里**必须**非 const：const 实例在两次 pump 之间是同一个对象，
    // Flutter 会跳过重建，那这条测试就永远绿（假护栏）。
    // 外面包 Scaffold：T18-5 加的排序下拉是 Material 组件，要求有 Material 祖先
    // （真实应用里 AppShell 本来就提供 Scaffold，这里只是补上和真实宿主一样的壳）。
    // ignore: prefer_const_constructors
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: HistoryPage())));
    await tester.pump();
    expect(find.text('历史记录'), findsWidgets, reason: '中文表下标题应该是「历史记录」');

    // 模拟用户在下拉里切到 English：换表 + 父级重新 pump 一个新实例
    installTables(table: en, base: zh, locale: 'en-US');
    // ignore: prefer_const_constructors
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: HistoryPage())));
    await tester.pump();
    expect(find.text('History'), findsWidgets,
        reason: '切到英文后历史页标题必须是 History —— 还显示中文就是没重建');
    expect(find.text('还没处理过文件'), findsNothing,
        reason: '空状态文案也要跟着语言变');
  });

  test('外壳里不能把页面写成 const —— 否则切语言时 Flutter 跳过重建', () {
    final src = File('lib/app_shell.dart').readAsStringSync();
    for (final page in ['HistoryPage', 'SettingsPage']) {
      expect(src.contains('const $page('), isFalse,
          reason: 'app_shell 里写 `const $page(` 会让语言切换不生效：父级 rebuild 时同一个 '
              'const 实例被判定「没变化」，子页面不重建、内容停在旧语言（真机撞到过）。'
              '去掉 const 即可，代价可以忽略。');
    }
  });

  test('状态栏初始文字不能在字段里算 t()', () {
    final src = File('lib/app_shell.dart').readAsStringSync();
    // 行首锚定：文档注释里引用的「String _status = t(...)」不能算数
    expect(RegExp(r'^\s*String\??\s+_status\s*=\s*t\(', multiLine: true).hasMatch(src), isFalse,
        reason: '字段初始化只在 State 创建时求值一次，用户切语言后状态栏会停在旧语言'
            '（真机上是「英文界面 + 底部写着就绪」）。应该存 null，在 build 里 '
            '`_statusRaw ?? t(\'shell.status.ready\')` 现算。');
  });
}
