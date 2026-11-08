// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 批量解压完成卡（Sunpack 三段式：结果标题 / 量化副标题 / 直达按钮）
/// —— 从 `home_page.dart` 抽出来的第 2 块。
///
/// 依赖极少：一份 `batch` 结果 + 三个回调（打开输出目录 / 在文件管理器里定位失败项 /
/// 再来一次）。原来这些东西都直接挂在 `_HomePageState` 上，所以只能留在那个 2976 行的类里。
///
/// 逐字搬移，只把 `_batch` / `_openFolder` / `_revealInFolder` / `_resetQueue` / `_size` /
/// `_batchOutcomeText` 换成构造参数与顶层函数。
library;

import 'package:flutter/material.dart';

import '../format.dart';
import '../humanize.dart';
import '../key_action_scope.dart';
import '../keyboard.dart';
import '../l10n.dart';

class BatchDoneView extends StatelessWidget {
  const BatchDoneView({
    super.key,
    required this.batch,
    required this.onOpenFolder,
    required this.onRevealInFolder,
    required this.onReset,
  });

  /// 引擎 `batch` 命令的结果原样（`ok` / `failed` / `partial` / `elapsed_ms` / `bytes` / `jobs`）
  final Map<String, dynamic> batch;

  final void Function(String dir) onOpenFolder;
  final void Function(String path) onRevealInFolder;
  final VoidCallback onReset;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final r = batch;
    final ok = (r['ok'] as num?)?.toInt() ?? 0;
    final failed = (r['failed'] as num?)?.toInt() ?? 0;
    final partial = (r['partial'] as num?)?.toInt() ?? 0;
    final ms = (r['elapsed_ms'] as num?)?.toInt() ?? 0;
    final bytes = (r['bytes'] as num?)?.toInt() ?? 0;
    final jobs = (r['jobs'] as List?) ?? const [];
    final failedJobs = jobs
        .cast<Map>()
        .where((j) => (j['outcome'] as String? ?? '') != 'ok')
        .toList();
    final String? firstDest = jobs
        .cast<Map>()
        .map((j) => j['dest'] as String?)
        .firstWhere((d) => d != null, orElse: () => null);

    final content = Column(
      key: const ValueKey('batch-done'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 6),
        Icon(failed == 0 ? Icons.check_circle : Icons.info_outline,
            size: 44, color: failed == 0 ? cs.primary : Colors.orange),
        const SizedBox(height: 8),
        Text(t('home.done'),
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 6),
        Text(
          t('home.batchSummary', {
            'ok': '$ok',
            'partial': partial > 0 ? t('home.batchPartial', {'n': '$partial'}) : '',
            'failed': failed > 0 ? t('home.batchFailed', {'n': '$failed'}) : '',
            'secs': (ms / 1000).toStringAsFixed(1),
            'size': formatBytes(bytes),
          }),
          textAlign: TextAlign.center,
          style: TextStyle(color: cs.onSurfaceVariant, fontSize: 13),
        ),
        const SizedBox(height: 14),
        if (failedJobs.isNotEmpty)
          Expanded(
            child: Card(
              clipBehavior: Clip.antiAlias,
              child: ListView.separated(
                itemCount: failedJobs.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (context, i) {
                  final j = failedJobs[i];
                  final name = (j['path'] as String? ?? '').split('/').last;
                  return ListTile(
                    dense: true,
                    leading: Icon(Icons.error_outline, size: 18, color: cs.error),
                    title: Text(name, style: const TextStyle(fontSize: 12)),
                    subtitle: Text(batchOutcomeText(j['outcome'] as String? ?? ''),
                        style: const TextStyle(fontSize: 11)),
                    // 失败项点一下在文件管理器里定位（调研 07:135：静默隔离是反面教材）
                    onTap: () => onRevealInFolder(j['path'] as String? ?? ''),
                  );
                },
              ),
            ),
          )
        else
          const Spacer(),
        const SizedBox(height: 10),
        Row(
          children: [
            if (firstDest != null)
              Expanded(
                // 键位就地提示（T11）：Enter = 打开输出目录；Esc = 再来一批
                child: Tooltip(
                  message: 'Enter',
                  child: FilledButton.icon(
                    onPressed: () => onOpenFolder(firstDest),
                    icon: const Icon(Icons.folder_open, size: 18),
                    label: Text(t('home.openOutDir')),
                  ),
                ),
              ),
            if (firstDest != null) const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: onReset,
                icon: const Icon(Icons.refresh, size: 18),
                label: Text(t('home.again')),
              ),
            ),
          ],
        ),
      ],
    );
    // Enter = 本屏主操作（UI-SPEC §4b.6）。这一屏的主操作是「打开输出目录」；
    // 一个成功的都没有（firstDest == null）时就不接 —— 按钮也没出现，
    // 键盘不能凭空造出一个界面上不存在的入口。
    return KeyActionScope(
      handlers: {
        KeyAction.primary: firstDest == null ? null : () => onOpenFolder(firstDest),
        KeyAction.cancel: onReset,
      },
      child: content,
    );
  }
}
