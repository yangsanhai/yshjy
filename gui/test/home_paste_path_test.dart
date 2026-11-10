// Ctrl+V 粘贴路径的 widget 回归（T11 无障碍的最后一笔）
//
// 为什么要有这一组：`keyboard.dart` 里早就把 Ctrl+V 映射成 `KeyAction.pastePath`，
// 但**全仓没有一处消费它** —— 键位认得、没人干活，用户按了完全没反应，
// 而且设置页的快捷键表还写着「Ctrl+V 粘贴路径」，等于文档在撒谎。
//
// 这里钉三件事：
//   ① 粘进来的是文件 → 走「选择文件」那条路（list + 进预览页）
//   ② 粘进来的是文件夹 → 走队列（scan）
//   ③ 粘进来的不是能打开的路径 → **必须有提示**（不能静默，静默 = 用户以为键坏了）

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';

import 'fake_engine.dart';

late FakeEngine fake;

/// 起一页首页，剪贴板内容由 `clipboard` 决定
Future<void> pumpHome(
  WidgetTester tester, {
  required Future<String?> Function() clipboard,
}) async {
  // 解压/识别完成会发桌面通知（notify-send 走 Process.run）—— 测试里必须关掉
  Notifier.enabled = false;
  fake = FakeEngine();
  installFakeEngine(fake);
  // 立刻回包：不回的话 EngineClient 的超时 Timer 会挂到测试结束
  fake.autoReply['list'] = {
    'ok': true,
    'result': {
      'format': 'zip',
      'complete': true,
      'total_size': 10,
      'encrypted': 0,
      'entries': [
        {
          'index': 1,
          'path': '里面的东西.txt',
          'size': 10,
          'packed': 10,
          'is_dir': false,
          'encrypted': false,
          'is_risky': false,
        },
      ],
      'hints': [],
    },
  };
  fake.autoReply['scan'] = {
    'ok': true,
    'result': {'items': []},
  };
  tester.view.physicalSize = const Size(1200, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(autoStart: false, readClipboardText: clipboard),
    ),
  ));
  await tester.pumpAndSettle();
}

/// 按 Ctrl+V 并把后面的事情推完
///
/// ★ 两个坑（都实测踩过）：
///   1. 「这条路径存不存在」「目录还是文件」是**真实磁盘 I/O**，
///      而 widget 测试跑在假时钟里 —— `await Directory(p).exists()` 永远不完成，
///      表现是「按了没反应、命令一条没发」。必须用 `runAsync` 给真实事件循环让出时间。
///   2. 之后**不能** `pumpAndSettle`：识别中那一屏是无限旋转的进度圈，
///      pumpAndSettle 会一直等到超时（`pumpAndSettle timed out`）。
Future<void> pressCtrlV(WidgetTester tester) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 120)));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
}

/// 造一个真存在的临时文件（内容无所谓，界面只查存在性）
String tempFile({String name = '包.zip'}) {
  final dir = Directory.systemTemp.createTempSync('yshjy_paste_');
  addTearDown(() {
    if (dir.existsSync()) dir.deleteSync(recursive: true);
  });
  final f = File('${dir.path}/$name')..writeAsBytesSync([1, 2, 3]);
  return f.path;
}

void main() {
  tearDownAll(restoreRealTransport);

  testWidgets('Ctrl+V 粘一个存在的文件 → 直接识别（发 list，进预览页）', (tester) async {
    final path = tempFile();
    await pumpHome(tester, clipboard: () async => path);

    await pressCtrlV(tester);

    final cmd = fake.lastCmd('list');
    expect(cmd, isNotNull,
        reason: 'Ctrl+V 没人消费 = 键位表写着能用、实际按了没反应');
    expect(cmd!['path'], path,
        reason: '命令里的路径不对，粘的和打开的不是同一个文件');
    expect(find.textContaining('里面的东西'), findsWidgets,
        reason: '识别出来了却没进预览页，用户还得再点一次');
  });

  testWidgets('Ctrl+V 粘一个文件夹 → 走队列（发 scan，不是 list）', (tester) async {
    final dir = Directory.systemTemp.createTempSync('yshjy_paste_dir_');
    addTearDown(() {
      if (dir.existsSync()) dir.deleteSync(recursive: true);
    });
    await pumpHome(tester, clipboard: () async => dir.path);

    await pressCtrlV(tester);

    expect(fake.lastCmd('scan'), isNotNull,
        reason: '粘目录却当成单个包去 list，会报「这不像压缩包」');
    expect(fake.lastCmd('scan')!['paths'], [dir.path]);
    expect(fake.lastCmd('list'), isNull, reason: '目录不该再走一次 list');
  });

  testWidgets('Ctrl+V 粘一段无关文本 → 有提示，不说「键坏了」', (tester) async {
    await pumpHome(tester, clipboard: () async => '今天天气不错，晚上吃火锅');

    await pressCtrlV(tester);

    // 一整句话在「路径」的眼里是一个不存在的路径 → 走「不是能打开的文件或文件夹」那句。
    // 关键是**必须有提示**：静默什么都不做，用户只会以为 Ctrl+V 没接上。
    expect(find.textContaining('不是一个能打开的文件或文件夹'), findsOneWidget);
    expect(fake.lastCmd('list'), isNull);
    expect(fake.lastCmd('scan'), isNull);
  });

  testWidgets('Ctrl+V 粘一个不存在的路径 → 有提示，且不去问引擎', (tester) async {
    await pumpHome(
      tester,
      clipboard: () async => '/绝对不存在/根本没有这个文件.zip',
    );

    await pressCtrlV(tester);

    expect(find.textContaining('不是一个能打开的文件或文件夹'), findsOneWidget);
    expect(fake.lastCmd('list'), isNull, reason: '不存在的路径不该送去引擎白跑一趟');
  });

  testWidgets('Enter 仍然只触发选文件，不受 Ctrl+V 影响', (tester) async {
    await pumpHome(tester, clipboard: () async => '/绝对不存在/根本.zip');
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    // 生产没注入 pickFiles → 会去弹系统对话框；测试环境里它返回空（等价用户取消），
    // 只要不等出「剪贴板」提示就说明 Enter 没走错分支
    expect(find.textContaining('剪贴板里没有路径'), findsNothing);
  });
}
