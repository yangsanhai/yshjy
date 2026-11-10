// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机外壳：底部三个页签（解压 / 历史 / 设置）
//!
//! 与桌面壳（`desktop/app_shell.dart`）是**两个独立实现**，只共用 `common/` 里的逻辑：
//! 桌面用左侧 `NavigationRail` + 托盘 + 快捷键 + 拖放，这些在手机上都不存在
//! （参考项目 rustdesk 也是这么分的：`desktop/` 与 `mobile/` 各写各的，
//! 见 docs/ANDROID-整体方案-2026-11-10.md §1.1）。

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/app_theme.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/theme_mode.dart';
import 'package:yshjy_gui/mobile/pages/mobile_history_page.dart';
import 'package:yshjy_gui/mobile/pages/mobile_home_page.dart';
import 'package:yshjy_gui/mobile/pages/mobile_settings_page.dart';

/// 手机端应用根
class MobileApp extends StatelessWidget {
  const MobileApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<ThemeMode>(
      valueListenable: themeMode,
      builder: (_, mode, __) => ValueListenableBuilder<String>(
        // 切语言要整棵树重建（各页面的文案都在 build 里用 t() 取）
        valueListenable: uiLocaleSetting,
        builder: (_, ___, ____) => MaterialApp(
          title: t('shell.appName'),
          debugShowCheckedModeBanner: false,
          theme: buildYshjyTheme(Brightness.light),
          darkTheme: buildYshjyTheme(Brightness.dark),
          themeMode: mode,
          home: const MobileShell(),
        ),
      ),
    );
  }
}

/// 三个页签的容器
class MobileShell extends StatefulWidget {
  const MobileShell({super.key});

  @override
  State<MobileShell> createState() => _MobileShellState();
}

class _MobileShellState extends State<MobileShell> {
  int _index = 0;

  @override
  Widget build(BuildContext context) {
    // 安卓 15+ 强制 edge-to-edge：不处理 SafeArea 的话，文字会钻到状态栏/手势条底下
    return Scaffold(
      body: SafeArea(
        // 底部让给 NavigationBar，顶部让给状态栏
        bottom: false,
        child: IndexedStack(
          index: _index,
          children: const [
            MobileHomePage(),
            MobileHistoryPage(),
            MobileSettingsPage(),
          ],
        ),
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (i) => setState(() => _index = i),
        destinations: [
          NavigationDestination(
            icon: const Icon(Icons.unarchive_outlined),
            selectedIcon: const Icon(Icons.unarchive),
            label: t('shell.nav.extract'),
          ),
          NavigationDestination(
            icon: const Icon(Icons.history_outlined),
            selectedIcon: const Icon(Icons.history),
            label: t('shell.nav.history'),
          ),
          NavigationDestination(
            icon: const Icon(Icons.settings_outlined),
            selectedIcon: const Icon(Icons.settings),
            label: t('shell.nav.settings'),
          ),
        ],
      ),
    );
  }
}

/// 手机上的「打开目录」：安卓没有文件管理器 API 可用，先给用户看路径 + 复制
Future<void> showPathSheet(BuildContext context, String title, String path) async {
  await showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (ctx) => Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 28),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title, style: Theme.of(ctx).textTheme.titleMedium),
          const SizedBox(height: 10),
          SelectableText(path, style: const TextStyle(fontSize: 13)),
          const SizedBox(height: 18),
          SizedBox(
            width: double.infinity,
            child: FilledButton.tonal(
              onPressed: () {
                // 复制到剪贴板：手机上这是把路径交给别的 App 的最省事办法
                Clipboard.setData(ClipboardData(text: path));
                Navigator.of(ctx).pop();
              },
              child: Text(t('home.copyHowtoBtn')),
            ),
          ),
        ],
      ),
    ),
  );
}

/// 这个平台是不是「手机形态」（决定用哪套壳；测试里也能用）
bool get isMobileForm => Platform.isAndroid || Platform.isIOS;
