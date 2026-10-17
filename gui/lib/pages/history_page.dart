import 'package:flutter/material.dart';

import '../history_store.dart';
import '../platform_paths.dart';

/// 历史页：本机处理过的记录（纯本地 JSON，不联网）
class HistoryPage extends StatefulWidget {
  const HistoryPage({super.key});

  @override
  State<HistoryPage> createState() => _HistoryPageState();
}

class _HistoryPageState extends State<HistoryPage> {
  List<HistoryEntry> _items = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    final list = await HistoryStore.load();
    if (!mounted) return;
    setState(() {
      _items = list;
      _loading = false;
    });
  }

  String _time(String iso) {
    try {
      final t = DateTime.parse(iso).toLocal();
      String two(int n) => n.toString().padLeft(2, '0');
      return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}';
    } catch (_) {
      return iso;
    }
  }

  String _size(int bytes) {
    const k = 1024;
    if (bytes >= k * k * k) return '${(bytes / (k * k * k)).toStringAsFixed(2)} GB';
    if (bytes >= k * k) return '${(bytes / (k * k)).toStringAsFixed(1)} MB';
    if (bytes >= k) return '${(bytes / k).toStringAsFixed(0)} KB';
    return '$bytes B';
  }

  /// 打开某个解压目录（历史条目点一下就走这里）
  Future<void> _reveal(String path) async {
    await openDirectory(path);
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text('历史记录', style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(width: 8),
              Text('${_items.length} 条', style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant)),
              const Spacer(),
              TextButton.icon(
                onPressed: _items.isEmpty
                    ? null
                    : () async {
                        await HistoryStore.clear();
                        _reload();
                      },
                icon: const Icon(Icons.delete_sweep_outlined, size: 16),
                label: const Text('清空', style: TextStyle(fontSize: 12)),
              ),
            ],
          ),
          Text(
            '只存在本机（${prettyPath(stateDir)}/gui-history.json），不会上传',
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          const SizedBox(height: 12),
          Expanded(
            child: _loading
                ? const Center(child: CircularProgressIndicator())
                : _items.isEmpty
                    ? Center(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(Icons.history, size: 48, color: cs.onSurfaceVariant),
                            const SizedBox(height: 10),
                            Text('还没处理过文件', style: TextStyle(color: cs.onSurfaceVariant)),
                          ],
                        ),
                      )
                    : Card(
                        clipBehavior: Clip.antiAlias,
                        child: ListView.separated(
                          itemCount: _items.length,
                          separatorBuilder: (_, __) => const Divider(height: 1),
                          itemBuilder: (context, i) {
                            final e = _items[i];
                            return ListTile(
                              dense: true,
                              leading: Icon(
                                e.ok ? Icons.check_circle_outline : Icons.error_outline,
                                size: 20,
                                color: e.ok ? cs.primary : cs.error,
                              ),
                              title: Text(
                                e.fileName,
                                style: const TextStyle(fontSize: 13),
                                overflow: TextOverflow.ellipsis,
                              ),
                              subtitle: Text(
                                '${_time(e.time)} · ${_size(e.bytes)}'
                                '${e.dest == null ? '' : ' · ${e.dest}'}',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(fontSize: 11),
                              ),
                              // 点一下打开解压目录（调研 07:135：找不到解到哪是反面教材）
                              onTap: e.dest == null ? null : () => _reveal(e.dest!),
                            );
                          },
                        ),
                      ),
          ),
        ],
      ),
    );
  }
}
