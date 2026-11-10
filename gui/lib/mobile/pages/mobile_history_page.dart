// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机端历史页：最近处理过的包（纯本地，不联网）

import 'package:flutter/material.dart';

import 'package:yshjy_gui/common/format.dart';
import 'package:yshjy_gui/common/history_store.dart';
import 'package:yshjy_gui/common/l10n.dart';

class MobileHistoryPage extends StatefulWidget {
  const MobileHistoryPage({super.key});

  @override
  State<MobileHistoryPage> createState() => _MobileHistoryPageState();
}

class _MobileHistoryPageState extends State<MobileHistoryPage> {
  List<HistoryEntry>? _items;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final list = await HistoryStore.load();
    if (mounted) setState(() => _items = list);
  }

  Future<void> _clear() async {
    await HistoryStore.clear();
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final items = _items;
    return Scaffold(
      appBar: AppBar(
        title: Text(t('history.title')),
        actions: [
          if ((items ?? const []).isNotEmpty)
            IconButton(
              tooltip: t('history.clear'),
              icon: const Icon(Icons.delete_outline),
              onPressed: _clear,
            ),
        ],
      ),
      body: items == null
          ? const Center(child: CircularProgressIndicator())
          : items.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.history, size: 56, color: Theme.of(context).colorScheme.outline),
                      const SizedBox(height: 12),
                      Text(t('history.empty')),
                    ],
                  ),
                )
              : RefreshIndicator(
                  onRefresh: _load,
                  child: ListView.separated(
                    itemCount: items.length,
                    separatorBuilder: (_, __) => const Divider(height: 1),
                    itemBuilder: (ctx, i) {
                      final e = items[i];
                      final when = DateTime.tryParse(e.time);
                      return ListTile(
                        leading: Icon(
                          e.ok ? Icons.task_alt : Icons.error_outline,
                          color: e.ok ? null : Theme.of(ctx).colorScheme.error,
                        ),
                        title: Text(e.fileName, maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: Text(
                          '${when == null ? e.time : when.toLocal().toString().substring(0, 16)}'
                          '${e.ok ? ' · ${formatBytes(e.bytes)}' : ''}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        trailing: e.dest == null
                            ? null
                            : IconButton(
                                tooltip: t('home.openDir'),
                                icon: const Icon(Icons.info_outline),
                                onPressed: () => showDialog<void>(
                                  context: ctx,
                                  builder: (_) => AlertDialog(
                                    title: Text(e.fileName),
                                    content: SelectableText(e.dest!),
                                    actions: [
                                      TextButton(
                                        onPressed: () => Navigator.of(ctx).pop(),
                                        child: Text(t('home.gotit')),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                      );
                    },
                  ),
                ),
    );
  }
}
