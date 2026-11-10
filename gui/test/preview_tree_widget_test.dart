// 预览树（第三期 A）的**交互**回归：勾父 / 取消子 / 半选 / 面包屑下钻。
//
// 为什么用 widget 测试而不是真机手点：这些是状态机，靠绝对指针点复选框差几像素
// 就点到隔壁行（实测把「已选 1001」点成「已选 1」），红绿说不清是功能错还是手滑。
// 树的**数据**逻辑另有 `entry_tree_test.dart`；这里只钉「点了之后界面上的样子」。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';

EntryItem file(String path, int index, int size) => EntryItem(
      index: index,
      path: path,
      size: size,
      packed: size,
      isDir: false,
      encrypted: false,
      risky: false,
    );

/// 3 个文件 / 3 个目录：合集A(第一季(一,二)) + 另一个(三)
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

Future<void> pumpPreview(WidgetTester tester) async {
  // 视口要够大：列表是 ListView，看不见的行不会被 build —— 默认 800x600 下
  // 预览页头部（摘要/勾选卡/状态栏/面包屑/列标题）一挤，列表只剩一两行，
  // find.text('另一个') 会「找不到」而被误判成功能坏了（实测踩到）。
  tester.view.physicalSize = const Size(1200, 2000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(
      body: HomePage(autoStart: false, debugPreview: sample()),
    ),
  ));
  await tester.pumpAndSettle();
}

/// 某个名字所在的那一行
Finder rowOf(String name) => find
    .ancestor(of: find.text(name), matching: find.byType(ListTile))
    .first;

/// 那一行里的复选框
Finder checkOf(String name) =>
    find.descendant(of: rowOf(name), matching: find.byType(Checkbox));

/// 那一行里「只看这一层」的下钻按钮
Finder drillOf(String name) => find.descendant(
      of: rowOf(name),
      matching: find.byIcon(Icons.subdirectory_arrow_right),
    );

bool? checkValue(WidgetTester tester, String name) =>
    tester.widget<Checkbox>(checkOf(name)).value;

void main() {
  testWidgets('进预览页默认全选，状态栏条数与合计都对', (tester) async {
    await pumpPreview(tester);
    expect(find.textContaining('已选 3 / 3 个文件'), findsOneWidget);
    expect(find.text('合集A'), findsOneWidget);
    expect(find.text('第一季'), findsOneWidget);
    expect(find.text('一.txt'), findsOneWidget);
    expect(checkValue(tester, '合集A'), isTrue);
  });

  testWidgets('点目录的复选框 → 整棵子树取消，兄弟目录不受影响', (tester) async {
    await pumpPreview(tester);
    await tester.tap(checkOf('合集A'));
    await tester.pumpAndSettle();

    expect(find.textContaining('已选 1 / 3 个文件'), findsOneWidget,
        reason: '合集A 下 2 个文件该被取消，只剩「另一个」里那 1 个');
    expect(checkValue(tester, '合集A'), isFalse);
    expect(checkValue(tester, '另一个'), isTrue);
    // 子项也跟着变
    expect(checkValue(tester, '一.txt'), isFalse);
  });

  testWidgets('半选状态：只勾子树里一个 → 父目录显示横线（value == null）', (tester) async {
    await pumpPreview(tester);
    // 先把合集A整棵取消，再单独勾回一个
    await tester.tap(checkOf('合集A'));
    await tester.pumpAndSettle();
    await tester.tap(checkOf('一.txt'));
    await tester.pumpAndSettle();

    expect(find.textContaining('已选 2 / 3 个文件'), findsOneWidget);
    expect(checkValue(tester, '合集A'), isNull,
        reason: '子项部分勾选时父目录要是三态中间值（界面画横线）');
    expect(checkValue(tester, '第一季'), isNull,
        reason: '中间层目录同样要半选');
    expect(checkValue(tester, '一.txt'), isTrue);
    expect(checkValue(tester, '二.txt'), isFalse);
  });

  testWidgets('半选时点父目录 → 变全选（不是变全不选）', (tester) async {
    await pumpPreview(tester);
    await tester.tap(checkOf('合集A'));
    await tester.pumpAndSettle();
    await tester.tap(checkOf('一.txt')); // 半选
    await tester.pumpAndSettle();
    expect(checkValue(tester, '合集A'), isNull);

    await tester.tap(checkOf('合集A'));
    await tester.pumpAndSettle();
    expect(checkValue(tester, '合集A'), isTrue);
    expect(find.textContaining('已选 3 / 3 个文件'), findsOneWidget);
  });

  testWidgets('点文件行本身也能勾/取消（不只点小方框）', (tester) async {
    await pumpPreview(tester);
    await tester.tap(find.text('三.txt'));
    await tester.pumpAndSettle();
    expect(find.textContaining('已选 2 / 3 个文件'), findsOneWidget);
    // 「另一个」下只有这 1 个文件，取消后它是**全不选**（不是半选 —— 半选要有
    // 「勾了一部分」才有意义；这条以前写成 isNull，是我想当然了）
    expect(checkValue(tester, '另一个'), isFalse);
  });

  testWidgets('面包屑下钻：只看这一层，兄弟目录从列表消失；点包名回根', (tester) async {
    await pumpPreview(tester);
    await tester.tap(drillOf('合集A'));
    await tester.pumpAndSettle();

    // 列表只剩「合集A」的子树；「另一个」被过滤掉
    expect(find.text('第一季'), findsOneWidget);
    expect(find.text('另一个'), findsNothing);
    expect(find.text('三.txt'), findsNothing);
    // 面包屑出现第二级（合集A 现在出现两次：面包屑 + 列表行由过滤后的第一季取代，
    // 所以「合集A」只剩面包屑那一处）
    expect(find.text('合集A'), findsOneWidget);

    // 点面包屑第一级（包名）回根
    await tester.tap(find.text('测试包.zip'));
    await tester.pumpAndSettle();
    expect(find.text('另一个'), findsOneWidget);
    expect(find.text('三.txt'), findsOneWidget);
  });

  testWidgets('空目录：显示「空目录」但复选框不可点', (tester) async {
    tester.view.physicalSize = const Size(1200, 2000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: HomePage(
          autoStart: false,
          debugPreview: Preview(
            format: 'zip',
            complete: true,
            totalSize: 5,
            encryptedCount: 0,
            encKind: null,
            entries: [
              EntryItem(
                index: 1,
                path: '空目录/',
                size: 0,
                isDir: true,
                encrypted: false,
                risky: false,
              ),
              file('有文件.txt', 2, 5),
            ],
            hints: const [],
          ),
        ),
      ),
    ));
    await tester.pumpAndSettle();

    expect(find.text('空目录'), findsOneWidget);
    expect(find.textContaining('空目录（没有东西可解）'), findsOneWidget);
    // 空目录那一行没有复选框
    expect(checkOf('空目录'), findsNothing);
    expect(find.textContaining('已选 1 / 1 个文件'), findsOneWidget);
  });

  testWidgets('声明的尺寸不可信 → 写「尺寸异常」，绝不摆包自己写的假 GB（A-6）',
      (tester) async {
    tester.view.physicalSize = const Size(1200, 2000);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: HomePage(
          autoStart: false,
          debugPreview: Preview(
            format: 'zip',
            complete: true,
            // CD 里写的就是这个假数字（724 B 的样本声称 1.17 GiB）
            totalSize: 1258686016,
            encryptedCount: 0,
            encKind: null,
            entries: [
              EntryItem(
                index: 1,
                path: '坏包里的.txt',
                size: 1258686016,
                isDir: false,
                encrypted: false,
                risky: false,
                sizeSuspect: true,
              ),
            ],
            hints: const [],
          ),
        ),
      ),
    ));
    await tester.pumpAndSettle();

    expect(find.text('尺寸异常'), findsWidgets,
        reason: '行里和状态栏都该写「尺寸异常」');
    expect(find.textContaining('1.17 GB'), findsNothing,
        reason: '包自己写的假数字不许出现在界面任何地方');
    expect(find.textContaining('1.2 GB'), findsNothing, reason: '换个单位的同一个假数字也不行');
  });
}
