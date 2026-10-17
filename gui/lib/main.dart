import 'package:flutter/material.dart';

import 'app_shell.dart';

void main(List<String> args) {
  // 右键「用杨三海解压打开」时，桌面环境会把文件路径当参数传进来
  runApp(YshjyApp(initialFile: args.isNotEmpty ? args.first : null));
}

/// 应用入口：只负责主题与语言无关的壳
///
/// 配色按 UI-SPEC 的「青色系」（#00BCD4）—— 与主人喜欢的初音色系一致。
/// 深色模式跟随系统（`ThemeMode.system`），不需要用户手动切。
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

    return MaterialApp(
      title: '杨三海解压',
      debugShowCheckedModeBanner: false,
      theme: theme(Brightness.light),
      darkTheme: theme(Brightness.dark),
      themeMode: ThemeMode.system,
      home: AppShell(initialFile: initialFile),
    );
  }
}
