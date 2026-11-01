// 预览页三个小零件的回归 —— 按块拆分的第 5d-1 步。
//
// 这三个原来是 `_miniBtn` / `_pickedBytes` / `_smartDirHint` 三个私有方法，
// 埋在 2976 行的 State 里：**纯函数反而最难测**（要 pump 整个预览页、造引擎状态、还得
// 精确点到某个按钮才间接验证）。抽成顶层函数后，几行就能把边界全钉一遍。
//
// `smartDirHint` 尤其值得测：它说的是「点开始解压之后磁盘上会多出什么」，
// 说错了用户会照着他以为的去找文件 —— 比不写更糟。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/pages/preview_parts.dart';

EntryItem f(String path, int index, int size) => EntryItem(
      index: index,
      path: path,
      size: size,
      packed: size,
      isDir: false,
      encrypted: false,
      risky: false,
    );

Preview pv(List<EntryItem> entries) => Preview(
      format: 'zip',
      complete: true,
      totalSize: entries.fold<int>(0, (a, e) => a + e.size),
      encryptedCount: 0,
      encKind: null,
      entries: entries,
      hints: const [],
    );

void main() {
  group('pickedBytes', () {
    test('只算勾上的文件', () {
      final p = pv([f('a.txt', 1, 100), f('b.txt', 2, 200), f('c.txt', 3, 400)]);
      expect(pickedBytes(p, {1, 3}), 500);
    });

    test('什么都没勾是 0', () {
      expect(pickedBytes(pv([f('a.txt', 1, 100)]), const {}), 0);
    });

    test('勾了不存在的序号也不会崩', () {
      final p = pv([f('a.txt', 1, 100)]);
      expect(pickedBytes(p, {1, 99}), 100);
    });
  });

  group('smartDirHint（对应引擎智能解压三条规则）', () {
    test('单文件 → 直接解出来', () {
      final hint = smartDirHint(pv([f('说明.txt', 1, 10)]));
      expect(hint, isNotEmpty);
      // 文案随语言包变，这里只断言「说清了是单个文件」这件事有没有做到
      expect(hint.contains('1') || hint.contains('一个'), isTrue,
          reason: '单文件时说「会新建文件夹」是错的 —— 引擎不会多套一层');
    });

    test('全在一个顶层目录下 → 解到那个目录里，不多套一层', () {
      final hint = smartDirHint(pv([
        f('第一季/一.txt', 1, 10),
        f('第一季/二.txt', 2, 20),
      ]));
      expect(hint.contains('第一季'), isTrue,
          reason: '得说出具体目录名，否则用户还是不知道会多出什么');
    });

    test('散文件 → 新建同名文件夹', () {
      final hint = smartDirHint(pv([
        f('一.txt', 1, 10),
        f('二.txt', 2, 20),
      ]));
      expect(hint, isNotEmpty);
      expect(hint.contains('2'), isTrue,
          reason: '要说清会装进文件夹的文件数');
    });

    test('空包（只有目录、没有文件）也有话说，不许空白', () {
      final hint = smartDirHint(pv(const []));
      expect(hint, isNotEmpty, reason: '空包给一句空话，用户以为预览坏了');
    });

    test('两个顶层目录 → 不能谎称「解到那个目录里」', () {
      final hint = smartDirHint(pv([
        f('甲/一.txt', 1, 10),
        f('乙/二.txt', 2, 20),
      ]));
      // 两个顶层目录时应该走「新建文件夹」那条，而不是挑一个说成目标
      expect(hint.contains('甲'), isFalse,
          reason: '两个顶层目录还说「解到甲里」，乙的文件去哪了？');
      expect(hint, isNotEmpty);
    });
  });

  group('MiniButton', () {
    testWidgets('点一下触发回调', (tester) async {
      var hits = 0;
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(body: MiniButton('全选', () => hits++)),
      ));
      await tester.tap(find.text('全选'));
      await tester.pump();
      expect(hits, 1);
    });

    testWidgets('高度 28（默认 TextButton 是 48，一行四个会把列表挤没）', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(body: Center(child: MiniButton('全选', () {}))),
      ));
      final size = tester.getSize(find.byType(MiniButton));
      expect(size.height, lessThanOrEqualTo(32),
          reason: '没压高度就退化成普通 TextButton，一行四个吃掉近 200 像素');
    });
  });
}
