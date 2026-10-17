// 冒烟测试：外壳能构建出来、五个导航项都在
//
// 不真的启动引擎（测试环境没有 yshjy 可执行文件），只看界面骨架。
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/app_shell.dart';

void main() {
  testWidgets('导航栏有五个入口', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: AppShell(bootstrapEngine: false)));
    await tester.pump();
    for (final label in ['解压', '打包', '历史', '设置', '关于']) {
      expect(find.text(label), findsWidgets, reason: '缺少导航项 $label');
    }
  });
}
