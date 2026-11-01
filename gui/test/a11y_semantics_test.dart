// 无障碍语义（T11）
//
// T11 的验收标准原文是「`grep -c 'Semantics(' > 0` 且 flutter test 全绿」——
// 但「grep 到了」只能证明写了这个词，证明不了**读屏软件真能念出对的东西**。
// 所以这里测的是语义或 tooltip 的**实际值**：
//   · 自绘可点区域（面包屑）必须读成「按钮 + 名字」
//   · 进度条必须念出「在干什么」+「到哪了」
//   · 纯图标按钮必须有说明文字（否则读屏只念「按钮」）
//
// 已知的边界（写清楚，别以为测了就是全覆盖）：
//   这里验的是**语义值对不对**，没验「屏幕阅读器实际念出来的顺序」——
//   那要真机开 Orca 听，属于用户验收那一层。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pages/home_flush_view.dart';
import 'package:yshjy_gui/pages/home_progress_view.dart';
import 'package:yshjy_gui/pages/preview_crumb_bar.dart';

void main() {
  testWidgets('面包屑每个格子都是「按钮 + 名字」语义，不是纯文本', (tester) async {
    final handle = tester.ensureSemantics();
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: CrumbBar(
          crumb: '合集A/第一季',
          filePath: '/tmp/伪装.mp4',
          onCrumbTap: (_) {},
        ),
      ),
    ));
    await tester.pump();

    // 三格：包名 / 合集A / 第一季
    expect(
      tester.getSemantics(find.text('伪装.mp4')),
      matchesSemantics(label: '伪装.mp4', isButton: true, hasTapAction: true),
      reason: 'InkWell 包文字不包 Semantics，读屏只会念「伪装.mp4」而不会说它可以按',
    );
    expect(
      tester.getSemantics(find.text('第一季')),
      matchesSemantics(label: '第一季', isButton: true, hasTapAction: true),
    );
    handle.dispose();
  });

  testWidgets('解压进度条念得出「正在解压」+ 百分比', (tester) async {
    await tester.pumpWidget(const MaterialApp(
      home: Scaffold(
        body: ExtractProgressBar(
          progress: null, // 扫描阶段：用 scanDone/scanTotal 算比例
          scanDone: 5,
          scanTotal: 10,
          current: null,
          speed: 0,
          paused: false,
        ),
      ),
    ));
    final bar = tester.widget<LinearProgressIndicator>(
        find.byType(LinearProgressIndicator));
    expect(bar.semanticsLabel, '正在解压…',
        reason: '没有 label，读屏只会念「进度指示器 50%」，用户不知道在干什么');
    expect(bar.semanticsValue, '50%');
  });

  testWidgets('落盘进度条也没漏语义，算不出比例时不报假数字', (tester) async {
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: FlushView(peak: 0, pending: 100, secs: 3, onCancel: () {}),
      ),
    ));
    final bar = tester.widget<LinearProgressIndicator>(
        find.byType(LinearProgressIndicator));
    expect(bar.semanticsLabel, '正在检验落盘…');
    expect(bar.semanticsValue, isNull,
        reason: 'peak<=0 时比例算不出来，报一个数字就是在骗人（和进度条本体同一口径）');
  });
}
