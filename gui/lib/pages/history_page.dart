import 'package:flutter/material.dart';

import '../history_store.dart';
import '../platform_paths.dart';
import '../l10n.dart';

/// 历史页：本机处理过的记录（纯本地 JSON，不联网）
class HistoryPage extends StatefulWidget {
  const HistoryPage({super.key});

  @override
  State<HistoryPage> createState() => _HistoryPageState();
}

class _HistoryPageState extends State<HistoryPage> {
  List<HistoryEntry> _items = [];
  bool _loading = true;

  /// 排序字段（T18-5）：`time` / `name` / `size`。默认按时间 —— 存储顺序本来就是
  /// 「最新在前」（见 `HistoryStore.add`），所以默认视图和以前一模一样。
  String _sortBy = 'time';

  /// 是否倒序。默认 true：
  /// · 时间 → 最新的在最上面（和存储顺序一致）
  /// · 名称 → 倒着排（Z → A）
  /// · 大小 → 大的在前面
  bool _desc = true;

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

  /// 列表按当前排序显示（**不改 `_items` 本身**：存储顺序是原始顺序，切回来还要用）
  List<HistoryEntry> get _view {
    final list = [..._items];
    int cmp(HistoryEntry a, HistoryEntry b) {
      switch (_sortBy) {
        case 'name':
          // 只做不区分大小写的字典序。**不承诺拼音序** —— 中文按码点排本来就
          // 看起来有点乱，这里不假装会拼音排序（要做就得上词库，不值当）。
          return a.fileName.toLowerCase().compareTo(b.fileName.toLowerCase());
        case 'size':
          return a.bytes.compareTo(b.bytes);
        default:
          // ISO-8601 字符串同格式可直接比；解析失败也不至于崩（最坏是排得不准）
          return a.time.compareTo(b.time);
      }
    }

    list.sort((a, b) => _desc ? cmp(b, a) : cmp(a, b));
    return list;
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
    // 排序只算一次：itemBuilder 里每格再算一遍等于 O(n² log n)（200 条也能扛，
    // 但没必要 —— 而且中途排序会让 index 和内容对不上，滚动时会看到条目跳）
    final view = _view;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text(t('history.title'), style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(width: 8),
              Text(t('history.count', {'n': '${_items.length}'}), style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant)),
              const Spacer(),
              TextButton.icon(
                onPressed: _items.isEmpty
                    ? null
                    : () async {
                        await HistoryStore.clear();
                        _reload();
                      },
                icon: const Icon(Icons.delete_sweep_outlined, size: 16),
                label: Text(t('history.clear'), style: const TextStyle(fontSize: 12)),
              ),
            ],
          ),
          Text(
            t('history.subtitle', {'path': '${prettyPath(stateDir)}/gui-history.json'}),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          // 排序（T18-5）：UI-SPEC 的「排序：时间 ▾」在这里落地。
          // 单独一行右对齐 —— 和标题挤一行在 576px 的真机窗口里会溢出。
          Row(
            children: [
              const Spacer(),
              Text(
                t('history.sort'),
                style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
              ),
              const SizedBox(width: 4),
              DropdownButton<String>(
                key: const ValueKey('history-sort'),
                value: _sortBy,
                isDense: true,
                underline: const SizedBox.shrink(),
                style: TextStyle(fontSize: 12, color: cs.onSurface),
                items: [
                  DropdownMenuItem(value: 'time', child: Text(t('history.sort.time'))),
                  DropdownMenuItem(value: 'name', child: Text(t('history.sort.name'))),
                  DropdownMenuItem(value: 'size', child: Text(t('history.sort.size'))),
                ],
                // 换字段时把方向复位成「这个字段最常用的方向」：
                // 时间 = 新的在前，名称 = A→Z，大小 = 大的在前。
                onChanged: (v) => setState(() {
                  _sortBy = v ?? 'time';
                  _desc = _sortBy != 'name';
                }),
              ),
              IconButton(
                key: const ValueKey('history-sort-dir'),
                onPressed: () => setState(() => _desc = !_desc),
                icon: Icon(_desc ? Icons.arrow_downward : Icons.arrow_upward, size: 16),
                iconSize: 16,
                visualDensity: VisualDensity.compact,
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(minWidth: 28, minHeight: 28),
                tooltip: t('history.sort.toggleDir'),
              ),
            ],
          ),
          const SizedBox(height: 8),
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
                            Text(t('history.empty'), style: TextStyle(color: cs.onSurfaceVariant)),
                          ],
                        ),
                      )
                    : Card(
                        clipBehavior: Clip.antiAlias,
                        child: ListView.separated(
                          itemCount: view.length,
                          separatorBuilder: (_, __) => const Divider(height: 1),
                          itemBuilder: (context, i) {
                            final e = view[i];
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
