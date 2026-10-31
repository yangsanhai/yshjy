// 设置页两个「自动找解压码」开关的回归（T8 界面半，2026-10-30）。
//
// 为什么非要有：这两个开关以前在设置页只是**一行硬编码字符串**
// （`auto_from_notes = true（从旁边的说明文件里找）`），用户没法改。
// 现在接成真勾选后，最容易出的错是「假开关」——
//   界面勾了 `auto_from_notes = "off"`，引擎却用 `as_bool()` 读 →
//   字符串返回 None → 该字段被静默忽略 → 保持默认 true →
//   用户明明关了，程序还在翻包旁边的说明文件，而且永远不报错。
//
// 所以这里钉三件事：
//   ① 没配置文件时两个开关默认是「开」（与引擎默认一致，别把默认值抄反）
//   ② 文件里写 `"off"` / `false` 两种写法，界面都显示成「关」
//   ③ 点保存后：文件里写出 on/off，且同段里用户手写的其它键（file / max_candidates）
//      一个字节都不许被冲掉
//
// 注意：这个测试会真写配置文件，必须用临时目录跑：
//   XDG_CONFIG_HOME=/tmp/yshjy-test-config flutter test

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/settings_page.dart';
import 'package:yshjy_gui/platform_paths.dart';

import 'config_guard.dart';
import 'fake_engine.dart';

String get _cfgPath => '$configDir/config.toml';

Future<void> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 4200);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  // ★ 必须整段放进 runAsync：`testWidgets` 默认是假时钟（FakeAsync），
  //   `await File.readAsString()` 这类**真 I/O** 的 Future 在里面永远不完成 →
  //   `_load()` 卡在第一行，界面停在初始默认值。教训：一开始我用 pumpAndSettle()
  //   + 后面补 runAsync 等待，全都不行（Future 是在假 zone 里发起的，跟真实事件循环
  //   断开了），只有「在 runAsync 里 pumpWidget」才连得上。
  //
  // 2026-11-01：等待方式从「睡固定 120ms」改成**轮询到保存按钮可用**。
  // 全量跑时机器忙，固定 sleep 会让这条偶发变红（单跑又绿）。
  bool loaded() {
    final f = find.widgetWithText(FilledButton, '保存');
    if (f.evaluate().isEmpty) return false;
    return tester.widget<FilledButton>(f).onPressed != null;
  }

  await tester.runAsync(() async {
    await tester.pumpWidget(
      const MaterialApp(home: Scaffold(body: SettingsPage())),
    );
    for (var i = 0; i < 120 && !loaded(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
      await tester.pump();
    }
  });
  await tester.pump();
}

bool _switchValue(WidgetTester tester, String title) {
  final tile = tester.widget<SwitchListTile>(
    find.ancestor(
      of: find.text(title),
      matching: find.byType(SwitchListTile),
    ),
  );
  // `SwitchListTile.value` 是非空 bool —— 原来的 `?? false` 是死代码（analyze 的
  // dead_null_aware_expression）。去掉后行为完全一样，只是不再留一条警告。
  return tile.value;
}

void main() {
  FakeEngine? fake;

  setUpAll(() {
    // 保险丝：跑错目录就会改到用户真配置，宁可整组报错也别偷偷写。
    // 判定收在 `config_guard.dart`（与 app_settings_test 同一套，别再各写一份）
    requireIsolatedConfigDir('设置页测试');
  });

  setUp(() async {
    // 设置页 `initState` 会问引擎要格式表和磁盘类型 —— 必须换掉真子进程
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = FakeEngine();
    installFakeEngine(fake!);

    final f = File(_cfgPath);
    if (f.existsSync()) f.deleteSync();
  });

  tearDownAll(restoreRealTransport);

  testWidgets('没有配置文件：两个开关默认都是关（与引擎默认一致）', (tester) async {
    await _pump(tester);
    // 2026-10-31 用户口径：自动找解压码属于「程序主动翻你的东西/替你试码」，
    // 必须用户自己在设置里打开，不许默认替他做。
    expect(_switchValue(tester, '自动从「说明.txt」里找解压码'), isFalse);
    expect(_switchValue(tester, '自动试「密码本」里的解压码'), isFalse);
  });

  testWidgets('文件里写字符串 off 和布尔 false，界面都显示关', (tester) async {
    File(_cfgPath)
      ..createSync(recursive: true)
      ..writeAsStringSync('[password]\n'
          'auto_from_notes = "off"\n'
          'auto_from_book = false\n');
    await _pump(tester);
    expect(_switchValue(tester, '自动从「说明.txt」里找解压码'), isFalse);
    expect(_switchValue(tester, '自动试「密码本」里的解压码'), isFalse);
  });

  testWidgets('点保存：写回 on/off，且同段的其它键原样保留', (tester) async {
    File(_cfgPath)
      ..createSync(recursive: true)
      ..writeAsStringSync('# 我自己写的说明，别冲掉\n'
          '[password]\n'
          'file = "my-passwords.txt"\n'
          'max_candidates = 5\n'
          'auto_from_notes = "on"\n'
          'auto_from_book = "on"\n');
    await _pump(tester);

    // 两个都关掉再保存
    await tester.tap(find.text('自动从「说明.txt」里找解压码'));
    await tester.pump();
    await tester.tap(find.text('自动试「密码本」里的解压码'));
    await tester.pump();
    // 点保存（同样要进 runAsync：它走真实文件写入）
    await tester.runAsync(() async {
      await tester.tap(find.widgetWithText(FilledButton, '保存'));
      await Future<void>.delayed(const Duration(milliseconds: 120));
    });
    await tester.pump();

    final text = File(_cfgPath).readAsStringSync();
    expect(text, contains('auto_from_notes = "off"'));
    expect(text, contains('auto_from_book = "off"'));
    // 用户手写的键与注释必须还在
    expect(text, contains('file = "my-passwords.txt"'));
    expect(text, contains('max_candidates = 5'));
    expect(text, contains('# 我自己写的说明，别冲掉'));

    // 再进来一次：界面必须显示成关（写进去的要能读回来，别自己打自己）
    await _pump(tester);
    expect(_switchValue(tester, '自动从「说明.txt」里找解压码'), isFalse);
    expect(_switchValue(tester, '自动试「密码本」里的解压码'), isFalse);
  });
}
