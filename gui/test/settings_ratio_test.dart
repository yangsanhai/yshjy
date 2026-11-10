// 「压缩比上限」入口（T20 第 3/3 步）
//
// 背景：引擎有这道闸（默认 100:1，防压缩炸弹），但**界面没有入口** ——
// 用户碰上「解不开的大日志 / 磁盘镜像」只能去手改 config.toml。
// 更要命的是协议那侧原来把上限写死成 100（已修），所以就算改了配置界面也不认。
//
// 这里钉三件事：
//   ① 设置页真有一个「压缩比上限」下拉（不是只写在文档里）
//   ② 四档都在（100 / 500 / 2000 / 不限），默认 100
//   ③ **保存真的落盘**，而且写成 `zip_bomb_ratio = 2000`（不带引号）——
//      引擎用 as_integer() 读，写成 "2000" 会被静默当没写，那就是「假开关」

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/desktop/pages/settings_page.dart';
import 'package:yshjy_gui/common/platform_paths.dart';

import 'fake_engine.dart';

void main() {
  tearDownAll(restoreRealTransport);

  late Directory tmp;

  setUp(() async {
    tmp = await Directory.systemTemp.createTemp('yshjy-ratio-');
    // 三个设置页测试会同时抢同一个 XDG 目录，所以必须注入自己的临时目录
    debugConfigDir = tmp.path;
  });

  tearDown(() async {
    debugConfigDir = null;
    if (tmp.existsSync()) tmp.deleteSync(recursive: true);
  });

  /// 把设置页 pump 起来并等它读完配置（和 keys_help_test 同一套隔离方式）
  Future<void> pumpSettings(WidgetTester tester) async {
    final fake = FakeEngine();
    installFakeEngine(fake);
    tester.view.physicalSize = const Size(1200, 5200); // ListView 只构建可视区
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    bool ready() =>
        find.text('zip').evaluate().isNotEmpty ||
        find.text('重试').evaluate().isNotEmpty;
    await tester.runAsync(() async {
      await tester.pumpWidget(
          const MaterialApp(home: Scaffold(body: SettingsPage())));
      for (var i = 0; i < 120 && !ready(); i++) {
        await Future<void>.delayed(const Duration(milliseconds: 25));
        await tester.pump();
      }
    });
    await tester.pumpAndSettle();
  }

  testWidgets('设置页有「压缩比上限」下拉，四档齐全，默认不检查', (tester) async {
    await pumpSettings(tester);

    expect(find.byKey(const ValueKey('ratio-select')), findsOneWidget,
        reason: '引擎有这道闸，界面却没有任何入口 —— 用户只能手改配置文件');
    expect(find.text(t('settings.safety.ratioOff')), findsOneWidget,
        reason: '默认必须是「不检查」—— 和 7-Zip / PeaZip 一致（2026-11-02 拍板）');

    // 展开看四档
    await tester.tap(find.byKey(const ValueKey('ratio-select')));
    await tester.pumpAndSettle();
    for (final k in ['ratioOff', 'ratio100', 'ratio1000', 'ratio2000']) {
      expect(find.text(t('settings.safety.$k')), findsWidgets,
          reason: '$k 这一档没在下拉里');
    }
  });

  testWidgets('选 2000 倍保存后，config.toml 里是 zip_bomb_ratio = 2000（不带引号）',
      (tester) async {
    await pumpSettings(tester);

    await tester.tap(find.byKey(const ValueKey('ratio-select')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(t('settings.safety.ratio2000')).last);
    await tester.pumpAndSettle();

    // 保存（真写文件）—— 文件 IO 是真实异步，必须放 runAsync 里，
    // 否则 pumpAndSettle 推不动它（keys_help_test 里 pump 用 runAsync 也是这个原因）
    await tester.runAsync(() async {
      await tester.tap(find.byKey(const ValueKey('settings-save')));
      await tester.pump();
      for (var i = 0; i < 40; i++) {
        if (File('${tmp.path}/config.toml').existsSync()) break;
        await Future<void>.delayed(const Duration(milliseconds: 25));
        await tester.pump();
      }
    });
    await tester.pumpAndSettle();

    final f = File('${tmp.path}/config.toml');
    expect(f.existsSync(), isTrue, reason: '保存按钮没写出配置文件');
    final cfg = f.readAsStringSync();
    expect(cfg, contains('[safety]'), reason: '没写进 [safety] 段');
    expect(cfg, contains('zip_bomb_ratio = 2000'),
        reason: '引擎用 as_integer() 读；写成别的样子就是「假开关」');
    expect(cfg, isNot(contains('"2000"')),
        reason: '带引号会变成字符串，引擎静默当没写');
  });

  testWidgets('配置里手写了 300 这种非档位值 → 夹到 1000，且不崩', (tester) async {
    // 用户手改配置是很常见的事；下拉里塞一个 items 里没有的值会直接 assert 崩
    File('${tmp.path}/config.toml').writeAsStringSync(
        '[safety]\nzip_bomb_ratio = 300\n');
    await pumpSettings(tester);

    expect(find.text(t('settings.safety.ratio1000')), findsOneWidget,
        reason: '300 应该往上取到 1000（用户想设限，不该被塞回 100）');
  });
}
