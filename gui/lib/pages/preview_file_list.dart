/// 预览页的文件清单（表头 + 卡片 + 滚动条 + 每一行）—— 从 `home_page.dart` 抽出来的
/// 第 5b/5c 块（`_fileList` + `_entryRow` 合并）。
///
/// **为什么合并成一个文件**：`_fileList` 的 itemBuilder 直接调 `_entryRow`，两者共享
/// `indent` / `tree` / `wide` 三个派生值。分开拆就得传一个 `rowBuilder` 回调，绕一层还不清楚。
/// 一个「列表 + 行」本来就是一个视图块。
///
/// **状态为什么全部留在外面**（4 处 setState 改成 3 个回调）：
/// `_picked`（勾选集合）解压时要传给引擎，`_crumb`（当前层）还要算缩进基准和面包屑，
/// `_expanded`（展开了哪些目录）跟树视图共用 —— 这三个都是**预览页的业务状态**，不是这块的私事。
/// 所以这块保持 `StatelessWidget`，只负责画 + 把用户动作报上去。
///
/// 三个回调各管一件事：
/// - `onToggleExpand(path)`：点目录行 = 展开/收起
/// - `onTogglePick(node)`：点文件行 / 点复选框 = 勾上或取消（目录走整棵）
/// - `onDrill(path)`：点「只看这一层」= 面包屑下钻进这个目录
library;

import 'package:flutter/material.dart';

import '../engine.dart';
import '../entry_tree.dart';
import '../format.dart';
import '../l10n.dart';

class FileListView extends StatelessWidget {
  const FileListView({
    super.key,
    required this.preview,
    required this.treeView,
    required this.tree,
    required this.expanded,
    required this.crumb,
    required this.picked,
    required this.scrollController,
    required this.onToggleExpand,
    required this.onTogglePick,
    required this.onDrill,
  });

  /// 引擎给的清单（平铺渲染用它的 `entries`）
  final Preview preview;

  /// 树视图开着吗（关掉就是平铺）
  final bool treeView;

  /// 树（`treeView` 为真时用它 flatten）
  final List<EntryNode> tree;

  /// 已展开的目录路径
  final Set<String> expanded;

  /// 当前面包屑层（缩进基准：停在哪层，那层的子项就不缩进；`''` = 根）
  final String crumb;

  /// 已勾选的**文件序号**（`EntryNode.index`；目录的三态靠子树里的序号算出来）
  final Set<int> picked;

  /// 滚动控制器由父层持有 —— 切树/平铺、重算清单时滚动位置不能丢
  final ScrollController scrollController;

  final void Function(String path) onToggleExpand;
  final void Function(EntryNode node) onTogglePick;
  final void Function(String path) onDrill;

  @override
  Widget build(BuildContext context) {
    final List<EntryNode> rows;
    if (treeView && tree.isNotEmpty) {
      rows = flattenTree(tree, expanded: expanded, crumb: crumb);
    } else {
      // 平铺：把条目包成「没有子项的单层节点」，勾选/图标/大小走同一套渲染，
      // 免得两套渲染以后各自长歪（状态栏的「已选」也必须两边一致）
      rows = [
        for (final e in preview.entries)
          EntryNode(
            name: e.name,
            path: e.path,
            level: 0,
            isDir: e.isDir,
            index: e.index,
            size: e.size,
            packed: e.packed,
            sizeSuspect: e.sizeSuspect,
            fileCount: e.isDir ? 0 : 1,
            encryptedCount: e.encrypted ? 1 : 0,
            modified: e.modified,
            risky: e.risky,
            encrypted: e.encrypted,
            children: const [],
          ),
      ];
    }
    // 缩进基准：面包屑停在哪一层，那一层的子项就不缩进
    final base = treeView ? depthOfPath(crumb) + 1 : 0;
    // 窄窗口不挤「压缩后」那一列（76+72+图标 ≈ 200px，窗口窄了文件名就没地方了）
    final wide = MediaQuery.of(context).size.width >= 720;
    final hdr = TextStyle(
        fontSize: 11, color: Theme.of(context).colorScheme.onSurfaceVariant);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.only(right: 14, bottom: 2),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              SizedBox(
                width: 76,
                child:
                    Text(t('home.colSize'), textAlign: TextAlign.right, style: hdr),
              ),
              if (wide)
                SizedBox(
                  width: 72,
                  child: Text(t('home.colCompressed'),
                      textAlign: TextAlign.right, style: hdr),
                ),
            ],
          ),
        ),
        Expanded(
          child: Card(
            clipBehavior: Clip.antiAlias,
            // 常显滚动条：列表比视口长时必须看得出来能滚（用户报过「滚轮没反应」，
            // 至少要有能拖的条；Flutter 在 Linux 桌面默认不画滚动条）
            child: Scrollbar(
              controller: scrollController,
              thumbVisibility: true,
              child: ListView.separated(
                controller: scrollController,
                itemCount: rows.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (context, i) {
                  final n = rows[i];
                  final indent =
                      (treeView && n.level > base) ? n.level - base : 0;
                  return _row(context, n, indent: indent, wide: wide);
                },
              ),
            ),
          ),
        ),
      ],
    );
  }

  /// 一行：三态复选框 + 图标 + 名字（+目录的文件数）+ 加密/风险 + 大小 + 压缩后
  Widget _row(
    BuildContext context,
    EntryNode n, {
    required int indent,
    required bool wide,
  }) {
    final cs = Theme.of(context).colorScheme;
    final st = pickStateOf(n, picked);
    final hasFiles = n.fileCount > 0; // 空目录没有可勾的东西
    final isExpanded = expanded.contains(n.path);
    // 审计 A-6：声明尺寸不可信 → 显示「尺寸异常」，绝不把包自己写的 GB 摆出来
    final sizeText = n.sizeSuspect
        ? t('home.sizeSuspect')
        : ((!n.isDir || hasFiles) ? formatBytes(n.size) : '');
    return ListTile(
      dense: true,
      contentPadding: EdgeInsets.only(left: 6.0 + indent * 18.0, right: 12),
      // 目录：点一行 = 展开/收起；文件：点一行 = 勾上/取消
      onTap: n.isDir
          ? (treeView ? () => onToggleExpand(n.path) : null)
          : () => onTogglePick(n),
      leading: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          SizedBox(
            width: 30,
            child: hasFiles
                ? Checkbox(
                    tristate: n.isDir,
                    value: n.isDir
                        ? (st == PickState.all
                            ? true
                            : (st == PickState.some ? null : false))
                        : (st == PickState.all),
                    // 点一下：没全选就整棵选中，已经全选就整棵取消
                    // （目录半选也走这里 —— 半选时点一下的直觉是「全选」）
                    onChanged: (_) => onTogglePick(n),
                  )
                : null,
          ),
          Icon(
            n.isDir
                ? (treeView && isExpanded
                    ? Icons.folder_open
                    : Icons.folder_outlined)
                : Icons.insert_drive_file_outlined,
            size: 20,
            color: n.isDir ? cs.primary : cs.onSurfaceVariant,
          ),
        ],
      ),
      title: Text(
        treeView ? n.name : n.path,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 13,
          fontWeight: n.isDir ? FontWeight.w500 : FontWeight.normal,
        ),
      ),
      // 目录带一行「N 个文件」；窄窗口省掉（把高度让给列表）
      subtitle: (treeView && n.isDir && wide)
          ? Text(
              n.isEmptyDir
                  ? t('home.emptyDir')
                  : t('home.nFiles', {'n': '${n.fileCount}'}),
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
            )
          : null,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          // 目录行的「只看这一层」入口：点它 = 面包屑下钻到该目录
          // （点目录名是展开/收起；要「进去」得有个明确入口，否则面包屑只能回不能进）
          if (treeView && n.isDir && !n.isEmptyDir)
            SizedBox(
              width: 30,
              child: IconButton(
                padding: EdgeInsets.zero,
                visualDensity: VisualDensity.compact,
                tooltip: t('home.onlyThisLevel'),
                icon: const Icon(Icons.subdirectory_arrow_right, size: 16),
                onPressed: () => onDrill(n.path),
              ),
            ),
          if (n.encrypted) const Icon(Icons.lock, size: 15),
          if (n.risky)
            const Padding(
              padding: EdgeInsets.only(left: 4),
              child: Icon(Icons.warning_amber_rounded, size: 16),
            ),
          const SizedBox(width: 8),
          SizedBox(
            width: 76,
            child: Text(
              sizeText,
              textAlign: TextAlign.right,
              style: const TextStyle(fontSize: 12),
            ),
          ),
          // 压缩后大小（PeaZip 默认列，调研 07:632）：一眼看出哪个文件「压不动」
          if (wide)
            SizedBox(
              width: 72,
              child: Text(
                (!n.isDir && n.packed != null)
                    ? formatBytes(n.packed!)
                    : '',
                textAlign: TextAlign.right,
                style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
              ),
            ),
        ],
      ),
    );
  }
}
