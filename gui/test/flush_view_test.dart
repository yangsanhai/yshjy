// 「等落盘」那一屏的回归 —— 按块拆分的第 6 块 A 段。
//
// 这一屏最要紧的是**进度条不能说谎**：
// 算得出比例时必须给具体数值（用户在等一件有尽头的事），
// 算不出（peak <= 0）时必须退回不确定态，绝不能画一根永远不动的 0% 条 ——
// 那看起来就是「卡死了」，用户会直接拔盘。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_flush_view.dart';

class Rec {
  int cancels = 0;
}

Future<Rec> pumpFlush(
  WidgetTester tester, {
  int peak = 1000,
  int pending = 250,
  int secs = 12,
}) async {
  final rec = Rec();
  tester.view.physicalSize = const Size(1000, 900);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: FlushView(
        peak: peak,
        pending: pending,
        secs: secs,
        onCancel: () => rec.cancels++,
      ),
    ),
  ));
  await tester.pump();
  return rec;
}

double? barValue(WidgetTester tester) =>
    tester.widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator)).value;

void main() {
  testWidgets('算出比例时进度条给具体数值（1000 待写、还剩 250 → 0.75）', (tester) async {
    await pumpFlush(tester, peak: 1000, pending: 250);
    expect(barValue(tester), closeTo(0.75, 1e-9),
        reason: '给不出比例就画转圈，用户在等有尽头的事却被暗示「不知道要多久」');
  });

  testWidgets('写完了（pending=0）就是满格 1.0', (tester) async {
    await pumpFlush(tester, peak: 1000, pending: 0);
    expect(barValue(tester), 1.0);
  });

  testWidgets('峰值未知（peak<=0）退回不确定态，不画一根 0% 的死条', (tester) async {
    await pumpFlush(tester, peak: 0, pending: 500);
    expect(barValue(tester), isNull,
        reason: '算不出比例还给 0.0，看起来就是卡死了，用户会拔盘');
  });

  testWidgets('剩余量、已等秒数都写出来', (tester) async {
    await pumpFlush(tester, peak: 1000, pending: 250, secs: 42);
    expect(find.textContaining('42'), findsOneWidget,
        reason: '「已等 N 秒」必须显示 —— 用户靠它判断要不要继续等');
    expect(find.textContaining('还剩约'), findsOneWidget);
  });

  testWidgets('点「停止等待」报给父层', (tester) async {
    final rec = await pumpFlush(tester);
    await tester.tap(find.text('停止等待'));
    await tester.pump();
    expect(rec.cancels, 1, reason: '停不下来是最难受的');
  });

  testWidgets('必须提醒「请勿拔出设备」', (tester) async {
    await pumpFlush(tester);
    expect(find.textContaining('请勿拔出'), findsOneWidget,
        reason: '这一屏存在的理由就是这句话，丢了就可能丢数据');
  });

  // ── T11 无障碍：键盘 ─────────────────────────────────────────────
  // UI-SPEC §4b.6「Esc 取消当前操作」。这一屏只有一个动作「停止等待」，
  // 等得不耐烦的用户第一反应就是按 Esc。
  testWidgets('Esc = 停止等待', (tester) async {
    final rec = await pumpFlush(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(rec.cancels, 1,
        reason: 'Esc 没接上，键盘用户只能眼睁睁看着进度条不许走');
  });

  testWidgets('Enter 在这屏不做任何事（误触不能打断落盘）', (tester) async {
    final rec = await pumpFlush(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(rec.cancels, 0,
        reason: 'Enter 意外取消了落盘，用户会以为「解压坏了」');
  });
}
