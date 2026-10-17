import 'package:flutter/material.dart';

import 'home_page.dart';

void main() {
  runApp(const YshjyApp());
}

/// 应用入口：只负责主题与语言无关的壳
///
/// 配色按 UI-SPEC 的「青色系」（#00BCD4）—— 与主人喜欢的初音色系一致。
/// 深色模式跟随系统（`ThemeMode.system`），不需要用户手动切。
class YshjyApp extends StatelessWidget {
  const YshjyApp({super.key});

  @override
  Widget build(BuildContext context) {
    const seed = Color(0xFF00BCD4);
    ThemeData theme(Brightness b) => ThemeData(
          useMaterial3: true,
          colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: b),
          visualDensity: VisualDensity.standard,
        );

    return MaterialApp(
      title: '杨三海解压',
      debugShowCheckedModeBanner: false,
      theme: theme(Brightness.light),
      darkTheme: theme(Brightness.dark),
      themeMode: ThemeMode.system,
      home: const HomePage(),
    );
  }
}
