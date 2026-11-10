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

/// K70 冠军版的逻辑分辨率（1080×2400 物理 / 3.0 dpr ≈ 360×800；取它实际报的 393×852 宽高比）
const Size _phoneSize = Size(393, 852);

/// 把系统中文字体装进测试引擎。
///
/// 关键一招：**同时注册成 `Roboto`** —— 测试环境的默认字体族就是 Roboto，
/// 只注册成别的名字的话界面还是用 Ahem（方块字），金标图看不清文案。
Future<void> _loadCjkFont() async {
  const candidates = [
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
  setUpAll(() async {
    await _loadCjkFont();
    await initLocale('zh-CN');
  });

  testWidgets('手机外壳：三个页签都在，能切到历史与设置', (tester) async {
    await tester.binding.setSurfaceSize(_phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(const MobileApp());
    await tester.pump(const Duration(milliseconds: 200));

    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.text('解压'), findsWidgets);
    expect(find.text('历史'), findsOneWidget);
    expect(find.text('设置'), findsOneWidget);

    // 切到历史：空态文案应该出现
    await tester.tap(find.descendant(of: find.byType(NavigationBar), matching: find.text('历史')));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('历史记录'), findsWidgets);

    // 切到设置：主题三项都在
    await tester.tap(find.descendant(of: find.byType(NavigationBar), matching: find.text('设置')));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('跟随系统'), findsWidgets);

    await tester.binding.setSurfaceSize(null);
  });

  testWidgets('金标：手机首页空态（按 K70 尺寸渲染）', (tester) async {
    await tester.binding.setSurfaceSize(_phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await tester.pumpWidget(const MobileApp());
    await tester.pump(const Duration(milliseconds: 400));

    await expectLater(
      find.byType(MaterialApp),
      matchesGoldenFile('goldens/mobile_home_empty.png'),
    );
    await tester.binding.setSurfaceSize(null);
  });
}
