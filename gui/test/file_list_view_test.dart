// 文件清单（列表 + 每一行）的独立回归 —— 按块拆分的第 5b/5c 块。
//
// 这块原来是 `_fileList` + `_entryRow` 两个方法埋在 2976 行的 State 里，
// 交互只能靠整页 pump + 精确点到某个复选框来验（实测真机手点会差几像素点到隔壁行）。
// 抽出来之后一次只验一个动作：展开、下钻、勾选、三态。
//
// 数据夹具照 preview_tree_widget_test.dart 的形状抄（同一套 EntryItem / Preview），
// 不另编一套 —— 两套夹具早晚对不上。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/entry_tree.dart';
import 'package:yshjy_gui/desktop/pages/preview_file_list.dart';

EntryItem file(String path, int index, int size) => EntryItem(
      index: index,
      path: path,
      size: size,
      packed: size,
      isDir: false,
      encrypted: false,
      risky: false,
    );

/// 3 个文件 / 2 个目录：合集A(第一季(一,二)) + 另一个(三)
Preview sample() => Preview(
      format: 'zip',
      complete: true,
      totalSize: 60,
      encryptedCount: 0,
      encKind: null,
      entries: [
        file('合集A/第一季/一.txt', 1, 10),
        file('合集A/第一季/二.txt', 2, 20),
        file('另一个/三.txt', 3, 30),
      ],
      hints: const [],
    );

class Rec {
  final expand = <String>[];
  final drill = <String>[];
  final pick = <String>[];
}

Future<Rec> pumpList(
  WidgetTester tester, {
  bool treeView = true,
  Set<String> expanded = const {},
  Set<int> picked = const {},
  String crumb = '',
}) async {
  final rec = Rec();
  final pv = sample();
  final ctrl = ScrollController();
  addTearDown(ctrl.dispose);
  // 视口要大：ListView 看不见的行不 build，find 会「找不到」被误判成功能坏了
  tester.view.physicalSize = const Size(1200, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: FileListView(
        preview: pv,
        treeView: treeView,
        tree: buildEntryTree(pv.entries),
        expanded: expanded,
        crumb: crumb,
        picked: picked,
        scrollController: ctrl,
        onToggleExpand: rec.expand.add,
        onTogglePick: (n) => rec.pick.add(n.path),
        onDrill: rec.drill.add,
      ),
    ),
  ));
  await tester.pump();
  return rec;
}

void main() {
  testWidgets('平铺模式显示完整路径（不是光秃秃的文件名）', (tester) async {
    await pumpList(tester, treeView: false);
    expect(find.text('合集A/第一季/一.txt'), findsOneWidget,
        reason: '平铺时只给文件名，两个同名文件就分不清是哪个');
    expect(find.text('一.txt'), findsNothing);
  });

  testWidgets('树模式显示名字，目录带「N 个文件」', (tester) async {
    await pumpList(tester);
    expect(find.text('合集A'), findsOneWidget);
    expect(find.text('3 个文件'), findsNothing, reason: '两个目录各 2 个和 1 个文件');
    expect(find.text('2 个文件'), findsOneWidget, reason: '合集A 下有两个文件');
    expect(find.text('1 个文件'), findsOneWidget, reason: '另一个 下有一个文件');
  });

  testWidgets('点目录行 = 展开/收起（报给父层，不自己改状态）', (tester) async {
    final rec = await pumpList(tester);
    await tester.tap(find.text('合集A'));
    await tester.pump();
    expect(rec.expand, ['合集A'],
        reason: '展开状态要留在父层（面包屑/缩进基准都要用），这里只报名');
    // 收起也要能报，不能只有展开没收起
    await tester.tap(find.text('合集A'));
    await tester.pump();
    expect(rec.expand, ['合集A', '合集A']);
  });

  testWidgets('「只看这一层」按钮 = 面包屑下钻到那个目录', (tester) async {
    final rec = await pumpList(tester);
    // tooltip 是「只看这一层」；点目录名是展开，两者必须分开
    await tester.tap(find.byTooltip('只看这一层').first);
    await tester.pump();
    expect(rec.drill, ['合集A'],
        reason: '没有这个入口，面包屑只能回不能进，用户进不去子目录');
  });

  testWidgets('点文件行 = 勾上；勾了全部的行复选框是 true', (tester) async {
    final rec = await pumpList(tester, treeView: false);
    await tester.tap(find.text('合集A/第一季/一.txt'));
    await tester.pump();
    expect(rec.pick, ['合集A/第一季/一.txt']);

    // 已勾选 → 复选框应为 true（三态里 all）
    await pumpList(tester, treeView: false, picked: {1});
    final boxes = tester.widgetList<Checkbox>(find.byType(Checkbox)).toList();
    expect(boxes.length, 3);
    expect(boxes[0].value, isTrue, reason: '第 1 个文件已勾，复选框没钩上就是假的');
    expect(boxes[1].value, isFalse);
  });

  testWidgets('目录半选时复选框是三态 null（不是全选也不是未选）', (tester) async {
    // 合集A 下两个文件（序号 1、2），只勾 1 → 父目录只能是半选
    await pumpList(tester, picked: {1});
    final boxes = tester.widgetList<Checkbox>(find.byType(Checkbox)).toList();
    final dirBox = boxes.firstWhere((b) => b.tristate);
    expect(dirBox.value, isNull,
        reason: '半选画成 false，用户以为一个都没勾；画成 true，又以为全勾了');
  });

  testWidgets('窄窗口省掉「压缩后」那一列，宽窗口才显示', (tester) async {
    // 宽：1200 - 卡片边距，肯定 >= 720
    await pumpList(tester);
    expect(find.text('压缩后'), findsOneWidget);

    // 窄：600 < 720
    tester.view.physicalSize = const Size(600, 1600);
    await tester.pump();
    expect(find.text('压缩后'), findsNothing,
        reason: '窄窗口硬挤两列数字，文件名就没地方显示了');
    expect(find.text('大小'), findsOneWidget, reason: '「大小」列窄窗口也要留');
  });
}
