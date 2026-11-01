/// 预览页顶部的两块小信息 —— 从 `home_page.dart` 抽出来的第 5d-2 第 3、4 段。
///
/// 两块都是**纯展示、零回调**，各自只吃一个字段：
/// - `PreviewSummary`：格式 · 文件数 · 总大小 · 加密条目数（一行）
/// - `PreviewNotes`：引擎给的提示行（0 条时不占地方）
///
/// 放在一个文件里是因为它们总是相邻出现、共享同一套 import，分成两个文件反而要多记一个名字。
library;

import 'package:flutter/material.dart';

import '../engine.dart';
import '../format.dart';
import '../l10n.dart';

/// 摘要行：`[ZIP] 12 个文件 · 3.4 MB … [🔒 2 个加密]`
class PreviewSummary extends StatelessWidget {
  const PreviewSummary({super.key, required this.preview});

  final Preview preview;

  @override
  Widget build(BuildContext context) {
    final pv = preview;
    return Row(
      children: [
        Chip(
          label: Text(pv.format.toUpperCase()),
          visualDensity: VisualDensity.compact,
        ),
        const SizedBox(width: 8),
        Text(t('home.fileCountSize',
            {'files': '${pv.fileCount}', 'size': formatBytes(pv.totalSize)})),
        const Spacer(),
        if (pv.encryptedCount > 0)
          Chip(
            avatar: const Icon(Icons.lock_outline, size: 16),
            label: Text(t('home.encryptedCount', {'n': '${pv.encryptedCount}'})),
            visualDensity: VisualDensity.compact,
          ),
      ],
    );
  }
}

/// 引擎给的提示行（例如「这个包有 1/256 误判概率」这类）
class PreviewNotes extends StatelessWidget {
  const PreviewNotes({super.key, required this.notes});

  final List<String> notes;

  @override
  Widget build(BuildContext context) {
    if (notes.isEmpty) return const SizedBox.shrink();
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final n in notes)
            Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: Row(
                children: [
                  Icon(Icons.info_outline, size: 16, color: cs.primary),
                  const SizedBox(width: 6),
                  Expanded(
                      child: Text(n, style: const TextStyle(fontSize: 13))),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
