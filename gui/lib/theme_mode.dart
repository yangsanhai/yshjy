import 'package:flutter/material.dart';

/// 全局主题模式（设置页切换后立刻生效，不用重启）
///
/// 单独放一个文件：`main.dart` 和 `settings_page.dart` 都要用它，
/// 放在 main 里会形成循环 import。
final ValueNotifier<ThemeMode> themeMode = ValueNotifier(ThemeMode.system);

/// 配置里的字符串 → ThemeMode（认不出就跟随系统）
ThemeMode parseThemeMode(String? v) => switch (v) {
      'light' => ThemeMode.light,
      'dark' => ThemeMode.dark,
      _ => ThemeMode.system,
    };

/// ThemeMode → 写回配置用的字符串
String themeModeName(ThemeMode m) => switch (m) {
      ThemeMode.light => 'light',
      ThemeMode.dark => 'dark',
      ThemeMode.system => 'auto',
    };
