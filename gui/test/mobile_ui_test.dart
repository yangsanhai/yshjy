// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机界面的「看得见」验证：按红米 K70 冠军版的屏幕尺寸渲染成 PNG
//!
//! 为什么这么验：手上没有真机连着的时候，光靠 `flutter test` 只能验「不崩」，
//! 验不了「长什么样」。金标图能把真实排版渲染出来 —— **并且装真字体**，
//! 否则 flutter_test 用的是方块字（Ahem），看不出中文有没有被压、被截断。
//!
//! 生成/更新：`flutter test test/mobile_ui_test.dart --update-goldens`
//! 产物：`test/goldens/mobile_home_*.png`

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/mobile/mobile_shell.dart';

import 'fake_engine.dart';

/// K70 冠军版的逻辑分辨率（1080×2400 物理 / 3.0 dpr ≈ 360×800；取它实际报的 393×852 宽高比）
const Size _phoneSize = Size(393, 852);

/// 把系统中文字体装进测试引擎。
///
/// 关键一招：**同时注册成 `Roboto`** —— 测试环境的默认字体族就是 Roboto，
/// 只注册成别的名字的话界面还是用 Ahem（方块字），金标图看不清文案。
Future<void> _loadCjkFont() async {
  const candidates = [
    // Linux（本机 / CI 的基准图要用它，否则中文字是方块）
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc', // 思源黑体，TTC
    '/usr/share/fonts/truetype/wqy/wqy-microhei.ttc', // 文泉驿微米黑，TTC
    // Windows（出包那台）
    r'C:\Windows\Fonts\simhei.ttf', // 黑体，纯 TTF
    r'C:\Windows\Fonts\simsun.ttc', // 宋体，TTC
    r'C:\Windows\Fonts\msyh.ttc', // 微软雅黑，TTC
  ];
  for (final path in candidates) {
    if (!File(path).existsSync()) continue;
    try {
      final bytes = File(path).readAsBytesSync();
      for (final family in const ['Roboto', 'Noto Sans CJK SC']) {
        final loader = FontLoader(family)
          ..addFont(Future.value(ByteData.view(bytes.buffer)));
        await loader.load();
      }
      return;
    } catch (_) {
      // 字体加载失败不影响用例，只是图里会是方块
    }
  }
}

void main() {
  late FakeEngine fake;

  setUpAll(() async {
    await _loadCjkFont();
    await initLocale('zh-CN');
  });

  // 设置页一进去就会 `hello()` 问引擎版本。不注入假引擎就会真 spawn 子进程，
  // 它那个 6 小时超时 Timer 在假时钟里永不完成 → 用例结束报
  // 「A Timer is still pending」（2026-11-11 本机跑全量时撞到）。
  // 每个用例给一个新 fake（EngineClient 是单例，旧流已关）。
  setUp(() {
    fake = FakeEngine();
    installFakeEngine(fake);
  });

  tearDown(() => fake.dispose());

  tearDownAll(restoreRealTransport);

  testWidgets('手机外壳：三个页签都在，能切到历史与设置', (tester) async {
    await tester.binding.setSurfaceSize(_phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(const MobileApp());
    await tester.pump(const Duration(milliseconds: 200));

    expect(find.byType(NavigationBar), findsOneWidget);
    // 文案一律从语言包取，不写死中文字面量：写死「跟随系统」会漏掉包里那个「（推荐）」，
    // 而且界面语言一变就整条假红（本机第一次跑全量就红在这里）。
    expect(find.text(t('shell.nav.extract')), findsWidgets);
    expect(find.text(t('shell.nav.history')), findsOneWidget);
    expect(find.text(t('shell.nav.settings')), findsOneWidget);

    // 切到历史：页标题应该出现
    await tester.tap(find.descendant(
        of: find.byType(NavigationBar), matching: find.text(t('shell.nav.history'))));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text(t('history.title')), findsWidgets);

    // 切到设置：主题三项都在
    await tester.tap(find.descendant(
        of: find.byType(NavigationBar), matching: find.text(t('shell.nav.settings'))));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text(t('settings.theme.auto')), findsWidgets);

    await tester.binding.setSurfaceSize(null);
  });

  testWidgets('金标：手机首页空态（按 K70 尺寸渲染）', (tester) async {
    await tester.binding.setSurfaceSize(_phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(const MobileApp());
    await tester.pump(const Duration(milliseconds: 400));

    // 基准按平台分开存：Windows 与 Linux 的中文字体回退不同，同一张图跨平台必差。
    // 更新本平台的基准：flutter test test/mobile_ui_test.dart --update-goldens
    final golden = 'goldens/mobile_home_empty_${Platform.operatingSystem}.png';
    await expectLater(
      find.byType(MaterialApp),
      matchesGoldenFile(golden),
    );
    await tester.binding.setSurfaceSize(null);
    // CI 的 runner 字体/渲染栈与开发机不保证一致，跨环境的像素比对只会变成没人看的假红。
    // 金标由开发机负责（每个平台存各自的基准）；CI 里设 YSHJY_SKIP_GOLDEN=1 跳过这条。
  }, skip: Platform.environment['YSHJY_SKIP_GOLDEN'] == '1');
}
