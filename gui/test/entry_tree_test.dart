// 预览树（第三期 A）纯函数单测。
//
// 覆盖方案 `docs/THIRD-PHASE-PLAN.md` §A「验收」点名的 6 组：空包 / 单层平铺 /
// 深嵌套 / 同名中间目录 / 加密计数 / 空目录不参与勾选 —— 外加三态勾选与面包屑。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/entry_tree.dart';

/// 造一条条目（默认是文件）
EntryItem it(
  String path, {
  int index = 1,
  int size = 0,
  int? packed,
  bool isDir = false,
  bool encrypted = false,
  bool risky = false,
}) =>
    EntryItem(
      index: index,
      path: path,
      size: size,
      packed: packed,
      isDir: isDir,
      encrypted: encrypted,
      risky: risky,
    );

void main() {
  group('buildEntryTree', () {
    test('空包：返回空树，不崩', () {
      final t = buildEntryTree(const []);
      expect(t, isEmpty);
      expect(countSelectable(t), 0);
      expect(allDirPaths(t), isEmpty);
    });

    test('单层平铺：全是文件，层级都是 0', () {
      final t = buildEntryTree([
        it('a.txt', index: 1, size: 10),
        it('b.txt', index: 2, size: 20),
      ]);
      expect(t.map((n) => n.name), ['a.txt', 'b.txt']);
      expect(t.every((n) => n.level == 0 && !n.isDir), isTrue);
      expect(t.map((n) => n.index), [1, 2]);
      expect(countSelectable(t), 2);
    });

    test('深嵌套：中间目录自动创建，层级递增，子树合计对得上', () {
      final t = buildEntryTree([
        it('a/', index: 1, isDir: true),
        it('a/b/', index: 2, isDir: true),
        it('a/b/c.txt', index: 3, size: 100, packed: 40),
        it('a/d.txt', index: 4, size: 200, packed: 60),
      ]);
      expect(t.length, 1);
      final a = t.first;
      expect(a.name, 'a');
      expect(a.isDir, isTrue);
      expect(a.level, 0);
      expect(a.fileCount, 2);
      expect(a.size, 300);
      expect(a.packed, 100);

      final b = a.children.firstWhere((n) => n.name == 'b');
      expect(b.level, 1);
      expect(b.fileCount, 1);
      expect(b.size, 100);

      final c = b.children.single;
      expect(c.path, 'a/b/c.txt');
      expect(c.level, 2);
      expect(c.isDir, isFalse);
      expect(c.index, 3);

      // 摊平 + 全展开 = 原样四行（目录先于子项）
      final rows = flattenTree(t, expanded: allDirPaths(t).toSet());
      expect(rows.map((n) => n.path), ['a', 'a/b', 'a/b/c.txt', 'a/d.txt']);
    });

    test('同名中间目录：多条路径复用同一个节点，不重复造', () {
      final t = buildEntryTree([
        it('x/y/1.txt', index: 1, size: 1),
        it('x/y/2.txt', index: 2, size: 2),
        it('x/z/3.txt', index: 3, size: 4),
      ]);
      final x = t.single;
      expect(x.children.map((n) => n.name), ['y', 'z']);
      final y = x.children.first;
      expect(y.fileCount, 2);
      expect(y.size, 3);
      // 合计：x = 1+2+4
      expect(x.size, 7);
      expect(x.fileCount, 3);
    });

    test('加密计数：目录汇总子树里的加密文件', () {
      final t = buildEntryTree([
        it('d/1.txt', index: 1, encrypted: true),
        it('d/2.txt', index: 2),
        it('d/sub/3.txt', index: 3, encrypted: true),
        it('plain.txt', index: 4),
      ]);
      final d = t.firstWhere((n) => n.name == 'd');
      expect(d.encryptedCount, 2);
      final plain = t.firstWhere((n) => n.name == 'plain.txt');
      expect(plain.encryptedCount, 0);
    });

    test('空目录条目：显示出来但不可勾（fileCount 0、三态恒为 none）', () {
      final t = buildEntryTree([
        it('empty/', index: 1, isDir: true),
        it('full/', index: 2, isDir: true),
        it('full/a.txt', index: 3, size: 5),
      ]);
      expect(t.length, 2);
      final empty = t.firstWhere((n) => n.name == 'empty');
      expect(empty.isDir, isTrue);
      expect(empty.isEmptyDir, isTrue);
      expect(empty.fileCount, 0);
      expect(empty.selectable, isFalse);

      final picked = <int>{};
      expect(pickStateOf(empty, picked), PickState.none);
      expect(applyPick(empty, picked, true), 0); // 点它什么也不会变
      expect(picked, isEmpty);

      final full = t.firstWhere((n) => n.name == 'full');
      expect(full.isEmptyDir, isFalse);
      expect(pickStateOf(full, picked), PickState.none);
    });

    test('packed 未知：目录合计整体记 null，不拿 0 冒充', () {
      final t = buildEntryTree([
        it('d/a.txt', index: 1, size: 1, packed: 1),
        it('d/b.txt', index: 2, size: 2), // packed = null
      ]);
      expect(t.single.packed, isNull);
    });
  });

  group('三态勾选', () {
    late List<EntryNode> tree;
    late Set<int> picked;

    setUp(() {
      tree = buildEntryTree([
        it('d/', index: 1, isDir: true),
        it('d/a.txt', index: 2, size: 10),
        it('d/b.txt', index: 3, size: 20),
        it('e.txt', index: 4, size: 30),
      ]);
      picked = <int>{};
    });

    test('勾父 → 子全选', () {
      final d = tree.firstWhere((n) => n.name == 'd');
      expect(applyPick(d, picked, true), 2);
      expect(picked, {2, 3});
      expect(pickStateOf(d, picked), PickState.all);
    });

    test('取消一个子 → 父变半选', () {
      final d = tree.firstWhere((n) => n.name == 'd');
      applyPick(d, picked, true);
      final a = d.children.firstWhere((n) => n.name == 'a.txt');
      applyPick(a, picked, false);
      expect(picked, {3});
      expect(pickStateOf(d, picked), PickState.some);
    });

    test('半选时点父一下 → 全选；再点 → 全取消', () {
      final d = tree.firstWhere((n) => n.name == 'd');
      applyPick(d, picked, true);
      applyPick(d.children.first, picked, false); // 半选
      expect(pickStateOf(d, picked), PickState.some);
      applyPick(d, picked, pickStateOf(d, picked) != PickState.all); // 界面点法
      expect(picked, {2, 3});
      applyPick(d, picked, pickStateOf(d, picked) != PickState.all);
      expect(picked, isEmpty);
    });

    test('全选 → _picked 条数 == 文件数（目录不计数）', () {
      final all = fileIndexesOf(tree[0]).toList(); // 只有 d 子树
      expect(all, [2, 3]);
      // 界面「全选」按钮走的是 pv.entries 过滤，这里等价校验：
      expect(countSelectable(tree), 3);
    });

    test('文件单点：勾上再取消，父目录三态跟着走', () {
      final d = tree.firstWhere((n) => n.name == 'd');
      final a = d.children.firstWhere((n) => n.name == 'a.txt');
      final b = d.children.firstWhere((n) => n.name == 'b.txt');
      applyPick(a, picked, true);
      applyPick(b, picked, true);
      expect(pickStateOf(d, picked), PickState.all);
      applyPick(a, picked, false);
      applyPick(b, picked, false);
      expect(pickStateOf(d, picked), PickState.none);
    });
  });

  group('面包屑与过滤', () {
    final tree = buildEntryTree([
      it('a/', index: 1, isDir: true),
      it('a/b/', index: 2, isDir: true),
      it('a/b/c.txt', index: 3),
      it('a/d.txt', index: 4),
      it('top.txt', index: 5),
    ]);

    test('breadcrumbs：根 + 每级一节，路径逐级拼', () {
      expect(breadcrumbs('').map((c) => c.path), ['']);
      expect(breadcrumbs('a/b').map((c) => c.name), ['全部', 'a', 'b']);
      expect(breadcrumbs('a/b').map((c) => c.path), ['', 'a', 'a/b']);
    });

    test('depthOfPath', () {
      expect(depthOfPath(''), -1);
      expect(depthOfPath('a'), 0);
      expect(depthOfPath('a/b'), 1);
    });

    test('面包屑停在 a：只列 a 的下一级（目录 + 文件）', () {
      final rows = flattenTree(tree, expanded: allDirPaths(tree).toSet(), crumb: 'a');
      expect(rows.map((n) => n.path), ['a/b', 'a/b/c.txt', 'a/d.txt']);
    });

    test('面包屑停在 a/b：只列 c.txt', () {
      final rows = flattenTree(tree, expanded: allDirPaths(tree).toSet(), crumb: 'a/b');
      expect(rows.map((n) => n.path), ['a/b/c.txt']);
    });

    test('不展开：只显示根层（目录收起，子项不出现）', () {
      final rows = flattenTree(tree);
      expect(rows.map((n) => n.path), ['a', 'top.txt']);
    });

    test('findNode：按路径找得到，找不到返回 null；前缀误判要挡住', () {
      expect(findNode(tree, 'a/b')?.name, 'b');
      expect(findNode(tree, 'a/bb'), isNull);
      expect(findNode(tree, ''), isNull);
    });
  });

  group('真实包形状（引擎 --list 的原始输出形态）', () {
    test('目录条目带尾斜杠也能正确聚合', () {
      // 真二进制实测：`{"path":"子目录/","is_dir":true}` + `{"path":"子目录/b.txt"}`
      final t = buildEntryTree([
        it('a.txt', index: 1, size: 6, packed: 6),
        it('子目录/', index: 2, isDir: true),
        it('子目录/b.txt', index: 3, size: 6, packed: 6),
      ]);
      expect(t.map((n) => n.path), ['a.txt', '子目录']);
      final d = t.last;
      expect(d.children.single.name, 'b.txt');
      expect(d.fileCount, 1);
      expect(d.size, 6);
      expect(d.packed, 6);
      // 中文名按显示宽度排也不会崩（这里只验数据层）
      expect(d.name, '子目录');
    });
  });
}
