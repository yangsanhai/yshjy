// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
// 包内目录树：**纯 Dart，不 import Flutter**，所以 `flutter test` 能直接测。
//
// 为什么不放引擎：引擎只给「带完整路径的平铺清单」，树是纯展示派生数据；
// 放前端算可以单测、改样式不用动协议（`docs/THIRD-PHASE-PLAN.md` §A 决策 1）。
//
// 约定：
//   · 目录节点的 `path` 不带尾斜杠（引擎给的目录条目带，这里统一剥掉）；
//   · 只有**文件**能进 `select`（引擎 `src/select.rs:8` 的序号锚点），目录只做聚合；
//   · 归档里的「空目录条目」显示出来但不可勾（没有内容可解，方案 §A 边界）；
//   · 目录的 `size` / `packed` / `fileCount` / `encryptedCount` 都是**子树合计**，
//     `packed` 只要有一个子项未知就整体记 null（别拿 0 冒充「压得很小」）。

import 'engine.dart';
import 'l10n.dart';

/// 勾选三态：文件只有 `all` / `none`，目录多一个 `some`（半选，画横线）
enum PickState { none, some, all }

/// 树节点（不可变；构建期用下面的 `_Node` 累加）
class EntryNode {
  const EntryNode({
    required this.name,
    required this.path,
    required this.level,
    required this.isDir,
    required this.size,
    required this.packed,
    required this.fileCount,
    required this.encryptedCount,
    required this.children,
    this.index,
    this.modified,
    this.risky = false,
    this.encrypted = false,
    this.sizeSuspect = false,
  });

  /// 本层名字（显示用）
  final String name;

  /// 归档内完整路径（面包屑跳转 / 展开状态都用它当键）
  final String path;

  /// 层级：根目录下的项 = 0
  final int level;

  final bool isDir;

  /// 文件：引擎条目序号（`select` 的锚点）；目录：只有归档里真有目录条目时才有值
  final int? index;

  /// 文件 = 自身解压后大小；目录 = 子树合计
  final int size;

  /// 压缩后大小；目录 = 子树合计（任一子项未知 ⇒ null）
  final int? packed;

  /// 子树里的文件数（目录自己不算）
  final int fileCount;

  /// 子树里的加密文件数
  final int encryptedCount;

  final int? modified;
  final bool risky;
  final bool encrypted;

  /// 自己（或子树里）有声明的尺寸不可信的条目（审计 A-6）→ 显示「尺寸异常」而不是假 GB
  final bool sizeSuspect;

  final List<EntryNode> children;

  /// 只有文件能进 `select`
  bool get selectable => !isDir && index != null;

  /// 归档里真有这个目录条目、但底下没东西（空目录：显示但不可勾）
  bool get isEmptyDir => isDir && children.isEmpty;

  @override
  String toString() =>
      'EntryNode($path, ${isDir ? "dir" : "file"}, '
      'files=$fileCount, size=$size)';
}

/// 把引擎给的平铺清单聚合成树。返回**根下的一级节点**（顺序 = 首次出现的顺序）。
///
/// 目录由路径里的中间段自动创建；同一个名字的中间目录被多条路径重复用到时**合并**成一个节点。
List<EntryNode> buildEntryTree(List<EntryItem> entries) {
  final root = _Node(name: '', path: '', level: -1, isDir: true);
  for (final e in entries) {
    final segs = e.path.split('/').where((s) => s.isNotEmpty).toList();
    if (segs.isEmpty) continue;
    var cur = root;
    for (var i = 0; i < segs.length; i++) {
      final isLast = i == segs.length - 1;
      final name = segs[i];
      final path = cur.path.isEmpty ? name : '${cur.path}/$name';
      cur = cur.child(name, path, isDir: isLast ? e.isDir : true);
      if (isLast) {
        if (e.isDir) {
          cur.markDir(e);
        } else {
          cur.markFile(e);
        }
      }
    }
  }
  return [for (final c in root.children.values) c.freeze()];
}

/// 按路径查节点（`''` 返回 null；找不到也返回 null）
EntryNode? findNode(List<EntryNode> roots, String path) {
  if (path.isEmpty) return null;
  for (final n in roots) {
    if (n.path == path) return n;
    if (n.isDir && _isPrefix(n.path, path)) {
      final hit = findNode(n.children, path);
      if (hit != null) return hit;
    }
  }
  return null;
}

/// `parent` 是不是 `path` 的上级（按段比，别用 `startsWith`：`a/b` 会误判 `a/bc`）
bool _isPrefix(String parent, String path) =>
    path.length > parent.length &&
    path.startsWith(parent) &&
    path[parent.length] == '/';

/// 面包屑的一节
class CrumbItem {
  const CrumbItem(this.name, this.path);
  final String name;
  final String path;
}

/// 面包屑：`[根, 一级, 二级…]`；`crumb` 为空时只有根
List<CrumbItem> breadcrumbs(String crumb) {
  final out = <CrumbItem>[CrumbItem(t('common.all'), '')];
  final segs = crumb.split('/').where((s) => s.isNotEmpty).toList();
  var acc = '';
  for (final s in segs) {
    acc = acc.isEmpty ? s : '$acc/$s';
    out.add(CrumbItem(s, acc));
  }
  return out;
}

/// 深度（根下的一级 = 0）；`''` 返回 -1
int depthOfPath(String path) =>
    path.isEmpty ? -1 : path.split('/').where((s) => s.isNotEmpty).length - 1;

/// 摊平成 ListView 能直接渲染的一行行。
///
/// - `crumb` 非空时，从这个目录的**下一级**开始列（面包屑「只显示该层」）；
/// - `expanded` 里的目录才展开子项（默认由界面决定要不要全展开）。
List<EntryNode> flattenTree(
  List<EntryNode> roots, {
  Set<String> expanded = const {},
  String crumb = '',
}) {
  final start = crumb.isEmpty
      ? roots
      : (findNode(roots, crumb)?.children ?? const <EntryNode>[]);
  final out = <EntryNode>[];
  void walk(List<EntryNode> nodes) {
    for (final n in nodes) {
      out.add(n);
      if (n.isDir && expanded.contains(n.path)) walk(n.children);
    }
  }

  walk(start);
  return out;
}

/// 收集一个节点子树里的**可勾选文件序号**（目录自身不参与）
Iterable<int> fileIndexesOf(EntryNode n) sync* {
  if (n.isDir) {
    for (final c in n.children) {
      yield* fileIndexesOf(c);
    }
  } else if (n.index != null) {
    yield n.index!;
  }
}

/// 三态：全选 / 半选 / 未选。空目录恒为 `none`（不可勾）。
PickState pickStateOf(EntryNode n, Set<int> picked) {
  if (!n.isDir) {
    return (n.index != null && picked.contains(n.index))
        ? PickState.all
        : PickState.none;
  }
  final idx = fileIndexesOf(n).toList();
  if (idx.isEmpty) return PickState.none;
  final hit = idx.where(picked.contains).length;
  if (hit == 0) return PickState.none;
  if (hit == idx.length) return PickState.all;
  return PickState.some;
}

/// 勾 / 取消一个节点（目录 = 作用于整棵子树）。
/// 返回真正**改动**的条数（0 表示点了没变化，界面就不用 setState 重绘整棵树）。
int applyPick(EntryNode n, Set<int> picked, bool select) {
  var changed = 0;
  for (final i in fileIndexesOf(n)) {
    final had = picked.contains(i);
    if (select && !had) {
      picked.add(i);
      changed++;
    } else if (!select && had) {
      picked.remove(i);
      changed++;
    }
  }
  return changed;
}

/// 一棵树（或一组子树）里的文件序号总数 —— 给「全选」和状态栏对账用
int countSelectable(List<EntryNode> nodes) {
  var n = 0;
  for (final e in nodes) {
    n += e.isDir ? countSelectable(e.children) : (e.index == null ? 0 : 1);
  }
  return n;
}

/// 收集所有目录路径（界面「默认全展开」用）
List<String> allDirPaths(List<EntryNode> roots) {
  final out = <String>[];
  void walk(List<EntryNode> nodes) {
    for (final n in nodes) {
      if (n.isDir) {
        out.add(n.path);
        walk(n.children);
      }
    }
  }

  walk(roots);
  return out;
}

// ───────────────────────── 构建期的可变节点 ─────────────────────────

class _Node {
  _Node({
    required this.name,
    required this.path,
    required this.level,
    required this.isDir,
  });

  final String name;
  final String path;
  final int level;
  final bool isDir;
  final Map<String, _Node> children = {};

  int? index;
  int ownSize = 0;
  int? ownPacked;
  bool ownSizeSuspect = false;
  int? modified;
  bool risky = false;
  bool encrypted = false;

  /// 同级同名节点复用（「同名中间目录」用例就靠它）
  _Node child(String name, String path, {required bool isDir}) =>
      children.putIfAbsent(
        name,
        () => _Node(name: name, path: path, level: level + 1, isDir: isDir),
      );

  void markFile(EntryItem e) {
    index = e.index;
    ownSize = e.size;
    ownPacked = e.packed;
    ownSizeSuspect = e.sizeSuspect;
    modified = e.modified;
    risky = e.risky;
    encrypted = e.encrypted;
  }

  /// 归档里真有这个目录条目（带序号）—— 空目录就靠它显示出来
  void markDir(EntryItem e) {
    index = e.index;
    modified = e.modified;
  }

  /// 转成不可变节点，顺手把聚合量算出来
  EntryNode freeze() {
    final kids = [for (final c in children.values) c.freeze()];
    if (!isDir) {
      return EntryNode(
        name: name,
        path: path,
        level: level,
        isDir: false,
        index: index,
        size: ownSize,
        packed: ownPacked,
        sizeSuspect: ownSizeSuspect,
        fileCount: 1,
        encryptedCount: encrypted ? 1 : 0,
        modified: modified,
        risky: risky,
        encrypted: encrypted,
        children: const [],
      );
    }
    var size = 0;
    var files = 0;
    var encs = 0;
    var packed = 0;
    var packedKnown = true;
    for (final k in kids) {
      size += k.size;
      files += k.fileCount;
      encs += k.encryptedCount;
      final p = k.packed;
      if (p == null) {
        packedKnown = false;
      } else {
        packed += p;
      }
    }
    return EntryNode(
      name: name,
      path: path,
      level: level,
      isDir: true,
      index: index,
      size: size,
      packed: packedKnown ? packed : null,
      // 目录：子树里只要有一个条目的声明尺寸不可信，整行也标出来（宁可提示，不摆假数字）
      sizeSuspect: kids.any((k) => k.sizeSuspect),
      fileCount: files,
      encryptedCount: encs,
      modified: modified,
      children: kids,
    );
  }
}
