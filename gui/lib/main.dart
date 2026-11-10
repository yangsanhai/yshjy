// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/app_settings.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/transport/ffi_transport.dart';
import 'package:yshjy_gui/desktop/app_shell.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/platform_paths.dart';
import 'package:yshjy_gui/common/theme_mode.dart';

/// 启动前读一次 `[general] theme`（之后由设置页切换，不用重启）
Future<void> _loadThemeMode() async {
  try {
    // 配置目录走 platform_paths 的唯一真源（以前手写 XDG/HOME，Windows 上算成 /.config）
    final f = File('$configDir${Platform.pathSeparator}config.toml');
    if (!await f.exists()) return;
    final text = await f.readAsString();
    var inGeneral = false;
    for (final line in text.split('\n')) {
      final t = line.trim();
      if (t.startsWith('[') && t.endsWith(']')) {
        inGeneral = t == '[general]';
        continue;
      }
      if (!inGeneral || t.isEmpty || t.startsWith('#')) continue;
      final i = t.indexOf('=');
      if (i <= 0) continue;
      if (t.substring(0, i).trim() != 'theme') continue;
      final v = t.substring(i + 1).trim().replaceAll('"', '');
      themeMode.value = parseThemeMode(v);
      return;
    }
  } catch (_) {
    // 配置读不出来不影响启动
  }
}

/// 启动前读一次界面语言（`[general] ui_language`，默认跟随系统）
///
/// 语言包读不出来**不能拦着启动**：退回基准语言，界面照开
/// （`initLocale` 内部已经处理「目标语言包坏了」，这里只兜底最外层）。
Future<void> _loadUiLanguage() async {
  try {
    await initLocale(await uiLanguageSetting());
  } catch (_) {
    // 连基准语言包都读不出来 —— 那就保持空表，界面显示 key，至少还能开起来报障
  }
}

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  // 安卓不许起子进程：引擎换成进程内 FFI（libyshjy.so，见 docs/ANDROID-整体方案 §4.2）。
  // 桌面平台仍走 ProcessTransport —— 这条分支不碰已验收的三个桌面包。
  if (Platform.isAndroid) {
    EngineClient.transportFactory = (_, __) => FfiTransport();
    // 沙箱目录只有原生知道（filesDir）。注入之后历史/配置才有地方写；
    // 注入失败不拦启动 —— 界面对着默认路径也能开，只是存不住东西。
    try {
      final dir = await const MethodChannel('yshjy/paths').invokeMethod<String>('filesDir');
      if (dir != null && dir.isNotEmpty) setPlatformStateDir(dir);
    } catch (_) {}
  }
  await _loadThemeMode();
  await _loadUiLanguage();
  // 右键「用杨三海解压打开」时，桌面环境会把文件路径当参数传进来
  runApp(YshjyApp(initialFile: args.isNotEmpty ? args.first : null));
}

/// 应用入口：只负责主题与语言无关的壳
///
/// 配色按 UI-SPEC 的「青色系」（#00BCD4）—— 与主人喜欢的初音色系一致。
/// 主题默认跟随系统，设置页可以切成浅色/深色（立刻生效，不用重启）。
class YshjyApp extends StatelessWidget {
  /// 启动时要直接打开的文件（来自命令行 / 右键菜单），可能为空
  final String? initialFile;

  const YshjyApp({super.key, this.initialFile});

  @override
  Widget build(BuildContext context) {
    const seed = Color(0xFF00BCD4);
    // Windows 上 Flutter 默认字体是 Segoe UI，中文全靠系统回退；实测回退会挑到
    // 「Microsoft YaHei Light」，11~12px 的小字笔画发细，看着发灰像半透明。
    // 显式把中文回退钉成常规字重的「Microsoft YaHei UI」，西文仍用 Segoe UI。
    // （Linux 不动：那边回退到 Noto Sans CJK，本来就没这毛病。）
    final bool isWindows = Platform.isWindows;
    ThemeData theme(Brightness b) => ThemeData(
          useMaterial3: true,
          colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: b),
          visualDensity: VisualDensity.standard,
          fontFamily: isWindows ? 'Segoe UI' : null,
          fontFamilyFallback:
              isWindows ? const ['Microsoft YaHei UI', 'Microsoft YaHei'] : null,
        );

    return ValueListenableBuilder<ThemeMode>(
      valueListenable: themeMode,
      builder: (_, mode, __) => ValueListenableBuilder<String>(
        // 切语言要整棵树重建：各页面的文案都在 build 里用 t() 取，
        // 只换表不重建 = 界面还停在旧语言
        valueListenable: uiLocaleSetting,
        builder: (_, ___, ____) => MaterialApp(
          title: t('app.name'),
          debugShowCheckedModeBanner: false,
          theme: theme(Brightness.light),
          darkTheme: theme(Brightness.dark),
          themeMode: mode,
          home: AppShell(initialFile: initialFile),
        ),
      ),
    );
  }
}
