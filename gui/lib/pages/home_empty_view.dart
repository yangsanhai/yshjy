/// 空态拖放区 —— 从 `home_page.dart` 抽出来的第 3 块。
///
/// 依赖只有三个：拖拽中标记（决定标题换不换那句话）+ 两个「选文件 / 选文件夹」回调。
///
/// **这块的界面约定改不得**（有测试钉着）：
/// - 第一屏只允许**一个** `FilledButton`（主操作 = 选文件），另一入口弱化成 `TextButton`
///   （调研 07:921-923：一屏两个同样重的按钮，新手不知道点哪个）。
///   实测在 `test/widget_test.dart` 里有一条专门断言 FilledButton 只有 1 个 —— 别改成 FilledButton。
/// - 拖放是核心入口（拖拽时标题换成「松手就行」），所以这块必须吃 `dragging` 状态。
library;

import 'package:flutter/material.dart';

import '../l10n.dart';

class EmptyDropView extends StatelessWidget {
  const EmptyDropView({
    super.key,
    required this.dragging,
    required this.onPickFile,
    required this.onPickFolder,
  });

  /// 有文件正拖在窗口上方（拖放高亮）
  final bool dragging;

  /// 主入口：选文件（第一屏唯一的主操作按钮）
  final VoidCallback onPickFile;

  /// 次入口：选文件夹（批量）
  final VoidCallback onPickFolder;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Center(
      key: const ValueKey('empty'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 120,
            height: 120,
            decoration: BoxDecoration(
              color: cs.primaryContainer.withValues(alpha: 0.5),
              shape: BoxShape.circle,
            ),
            child: Icon(Icons.inbox_outlined, size: 56, color: cs.primary),
          ),
          const SizedBox(height: 24),
          Text(
            dragging ? t('home.dropHint') : t('home.dropTitle'),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            t('home.dropDesc'),
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: cs.onSurfaceVariant,
                  height: 1.6,
                ),
          ),
          const SizedBox(height: 28),
          Wrap(
            alignment: WrapAlignment.center,
            spacing: 10,
            children: [
              FilledButton.icon(
                onPressed: onPickFile,
                icon: const Icon(Icons.add),
                label: Text(t('home.pickFile')),
                style: FilledButton.styleFrom(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 26, vertical: 18),
                ),
              ),
              const SizedBox(width: 8),
              // 次入口弱化成文字按钮（调研 07:921-923：第一屏只该有一个最强入口）
              // ⚠️ 别改成 FilledButton —— widget_test 里有条测试专门钉着这件事
              TextButton.icon(
                onPressed: onPickFolder,
                icon: const Icon(Icons.folder_open_outlined, size: 16),
                label: Text(t('home.pickDir'),
                    style: const TextStyle(fontSize: 12)),
              ),
            ],
          ),
          const SizedBox(height: 24),
          Text(
            t('home.anyFormatHint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
        ],
      ),
    );
  }
}
