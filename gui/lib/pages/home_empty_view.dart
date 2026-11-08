// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
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

import '../key_action_scope.dart';
import '../keyboard.dart';
import '../l10n.dart';

class EmptyDropView extends StatelessWidget {
  const EmptyDropView({
    super.key,
    required this.dragging,
    required this.onPickFile,
    required this.onPickFolder,
    required this.onPastePath,
  });

  /// 有文件正拖在窗口上方（拖放高亮）
  final bool dragging;

  /// 主入口：选文件（第一屏唯一的主操作按钮）
  final VoidCallback onPickFile;

  /// 次入口：选文件夹（批量）
  final VoidCallback onPickFolder;

  /// `Ctrl+V`：把剪贴板里的路径粘进来并自动识别（UI-SPEC §4b.6）
  final VoidCallback onPastePath;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final content = Center(
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
              // 键位就地提示（T11）：Enter = 本屏主操作。设置页有完整表格，
              // 但用户不该为了找一个键先跑去设置页。
              Tooltip(
                message: 'Enter',
                child: FilledButton.icon(
                  onPressed: onPickFile,
                  icon: const Icon(Icons.add),
                  label: Text(t('home.pickFile')),
                  style: FilledButton.styleFrom(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 26, vertical: 18),
                  ),
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
          const SizedBox(height: 4),
          // 键位就地提示（T11）：设置页有完整表格，但没人会为了找一个键跑去设置页。
          // Ctrl+V 尤其藏 —— 用户从文件管理器复制完，第一反应就是按 Ctrl+V。
          Text(
            t('home.pasteHint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
        ],
      ),
    );
    // Enter = 本屏主操作（UI-SPEC §4b.6「Enter 确认」）。第一屏的主操作就是「选文件」，
    // 也就是上面那个唯一的主按钮 —— 键盘和鼠标走同一个回调，不另开一条路。
    // Ctrl+V = 粘贴路径（同一节）：剪贴板里有路径时不该逼用户去点对话框。
    return KeyActionScope(
      handlers: {
        KeyAction.primary: onPickFile,
        KeyAction.pastePath: onPastePath,
      },
      child: content,
    );
  }
}
