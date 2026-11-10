// 处理中等待页的回归（按块拆分的第 4 块）。
//
// 这块只有一行字 + 一个转圈，但它有两个容易踩的点，值得钉住：
//   ① 转圈是**无限动画** —— 用 `pumpAndSettle` 会等到超时，这里只能用定次 `pump`；
//   ② 那句话必须原样显示出来（用户要看到「正在扫描…」，否则就是个转圈黑箱）。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/pages/home_busy_view.dart';

void main() {
  testWidgets('显示传进来的那句话 + 一个转圈', (tester) async {
    await tester.pumpWidget(const MaterialApp(
      home: Scaffold(body: BusyView(text: '正在扫描…')),
    ));
    // 不能用 pumpAndSettle：CircularProgressIndicator 是永不结束的 ticker，会一直等到超时
    await tester.pump();

    expect(find.text('正在扫描…'), findsOneWidget,
        reason: '只有转圈没有字，用户不知道在等什么，会以为卡死');
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
  });
}
