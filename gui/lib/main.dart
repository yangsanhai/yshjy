import 'dart:io';

import 'package:flutter/material.dart';

import 'app_shell.dart';
import 'platform_paths.dart';
import 'theme_mode.dart';

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

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  await _loadThemeMode();
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
    ThemeData theme(Brightness b) => ThemeData(
          useMaterial3: true,
          colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: b),
          visualDensity: VisualDensity.standard,
        );

    return ValueListenableBuilder<ThemeMode>(
      valueListenable: themeMode,
      builder: (_, mode, __) => MaterialApp(
        title: '杨三海解压',
        debugShowCheckedModeBanner: false,
        theme: theme(Brightness.light),
        darkTheme: theme(Brightness.dark),
        themeMode: mode,
        home: AppShell(initialFile: initialFile),
      ),
    );
  }
}
