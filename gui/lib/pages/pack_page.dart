import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../engine.dart';

/// 打包页：把文件/文件夹做成"能播放的伪装包"
///
/// 走引擎的 `pack` 命令（协议 v1）。图片/文本封面会自动用「尾部追加」布局，
/// 原因见 PROTOCOL.md / pack/cover.rs：free 原子是 MP4 的 ISO 结构，塞进图片会写坏封面。
class PackPage extends StatefulWidget {
  final ValueChanged<String>? onStatus;

  const PackPage({super.key, this.onStatus});

  @override
  State<PackPage> createState() => _PackPageState();
}

class _PackPageState extends State<PackPage> {
  final EngineClient _client = EngineClient.instance;

  final List<String> _inputs = [];
  String? _out;
  String _coverExt = 'mp4';
  final TextEditingController _pwCtrl = TextEditingController();
  /// 解压码是否显示明文（眼睛图标）
  bool _showPw = false;
  /// 交付选项（DESIGN §17.8）：分卷 / 校验和 / 说明书
  bool _split = false;
  bool _checksum = true;
  bool _readme = true;
  final _splitCtrl = TextEditingController(text: '2G');
  bool _busy = false;
  String? _error;
  Map<String, dynamic>? _result;

  @override
  void dispose() {
    _pwCtrl.dispose();
    super.dispose();
  }

  Future<void> _addFiles() async {
    const group = XTypeGroup(label: '任意文件');
    final files = await openFiles(acceptedTypeGroups: const [group]);
    if (files.isEmpty) return;
    setState(() {
      for (final f in files) {
        if (!_inputs.contains(f.path)) _inputs.add(f.path);
      }
      _out ??= _suggestOut(_inputs.first);
    });
  }

  Future<void> _addFolder() async {
    final dir = await getDirectoryPath();
    if (dir == null) return;
    setState(() {
      if (!_inputs.contains(dir)) _inputs.add(dir);
      _out ??= _suggestOut(dir);
    });
  }

  String _suggestOut(String first) {
    final base = first.split('/').last;
    final stem = base.contains('.') ? base.substring(0, base.lastIndexOf('.')) : base;
    final dir = first.substring(0, first.lastIndexOf('/') + 1);
    return '$dir$stem.$_coverExt';
  }

  Future<void> _pickOut() async {
    final loc = await getSaveLocation(suggestedName: _out?.split('/').last ?? 'packed.$_coverExt');
    if (loc == null) return;
    setState(() {
      _out = loc.path;
      final dot = loc.path.lastIndexOf('.');
      if (dot > 0) _coverExt = loc.path.substring(dot + 1).toLowerCase();
    });
  }

  Future<void> _pack() async {
    if (_inputs.isEmpty || _out == null) return;
    setState(() {
      _busy = true;
      _error = null;
      _result = null;
    });
    widget.onStatus?.call('正在打包…');
    try {
      final r = await _client.pack(
        inputs: _inputs,
        out: _out!,
        password: _pwCtrl.text.trim().isEmpty ? null : _pwCtrl.text.trim(),
        splitBytes: _split ? _parseSize(_splitCtrl.text) : null,
        checksum: _checksum,
        readme: _readme,
      );
      if (!mounted) return;
      setState(() => _result = r);
      widget.onStatus?.call('打包完成');
    } on EngineError catch (e) {
      if (!mounted) return;
      setState(() => _error = e.code);
      widget.onStatus?.call('打包失败：${e.code}');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// 解析「2G / 500M / 1048576」为字节数（看不懂返回 null = 不分卷）
  int? _parseSize(String s) {
    final m = RegExp(r'^([0-9.]+)\s*(GB|G|MB|M|KB|K|B)?$')
        .firstMatch(s.trim().toUpperCase());
    if (m == null) return null;
    final v = double.tryParse(m.group(1)!);
    if (v == null || v <= 0) return null;
    final mult = switch (m.group(2)) {
      'GB' || 'G' => 1024 * 1024 * 1024,
      'MB' || 'M' => 1024 * 1024,
      'KB' || 'K' => 1024,
      _ => 1,
    };
    return (v * mult).toInt();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text('打包成伪装文件', style: Theme.of(context).textTheme.titleLarge),
          const SizedBox(height: 4),
          Text(
            '把文件夹/文件做成一个「能播放/能看」的文件，再传给别人',
            style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              OutlinedButton.icon(
                onPressed: _busy ? null : _addFolder,
                icon: const Icon(Icons.create_new_folder_outlined, size: 18),
                label: const Text('加文件夹'),
              ),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                onPressed: _busy ? null : _addFiles,
                icon: const Icon(Icons.insert_drive_file_outlined, size: 18),
                label: const Text('加文件'),
              ),
              const Spacer(),
              if (_inputs.isNotEmpty)
                TextButton(
                  onPressed: _busy ? null : () => setState(_inputs.clear),
                  child: const Text('清空'),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Expanded(
            child: _inputs.isEmpty
                ? Center(
                    child: Text(
                      '先加一个文件夹或几个文件',
                      style: TextStyle(color: cs.onSurfaceVariant),
                    ),
                  )
                : Card(
                    clipBehavior: Clip.antiAlias,
                    child: ListView.separated(
                      itemCount: _inputs.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, i) {
                        final p = _inputs[i];
                        final isDir = Directory(p).existsSync();
                        return ListTile(
                          dense: true,
                          leading: Icon(
                            isDir ? Icons.folder_outlined : Icons.insert_drive_file_outlined,
                            size: 20,
                          ),
                          title: Text(p.split('/').last, style: const TextStyle(fontSize: 13)),
                          subtitle: Text(
                            isDir ? '文件夹（会整个打进去）' : p,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(fontSize: 11),
                          ),
                          trailing: IconButton(
                            icon: const Icon(Icons.close, size: 16),
                            onPressed: _busy ? null : () => setState(() => _inputs.removeAt(i)),
                          ),
                        );
                      },
                    ),
                  ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: TextField(
                  readOnly: true,
                  controller: TextEditingController(text: _out ?? '（还没选输出位置）'),
                  style: const TextStyle(fontSize: 12),
                  decoration: InputDecoration(
                    labelText: '输出文件',
                    isDense: true,
                    border: const OutlineInputBorder(),
                    suffixIcon: IconButton(
                      icon: const Icon(Icons.folder_open, size: 18),
                      onPressed: _busy ? null : _pickOut,
                    ),
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          Row(
            children: [
              const Text('伪装成：', style: TextStyle(fontSize: 13)),
              const SizedBox(width: 8),
              SegmentedButton<String>(
                segments: const [
                  ButtonSegment(value: 'mp4', label: Text('视频')),
                  ButtonSegment(value: 'png', label: Text('图片')),
                  ButtonSegment(value: 'txt', label: Text('文本')),
                ],
                selected: {_coverExt == 'jpg' ? 'png' : _coverExt},
                onSelectionChanged: _busy
                    ? null
                    : (s) {
                        setState(() {
                          _coverExt = s.first;
                          if (_out != null) {
                            final dot = _out!.lastIndexOf('.');
                            if (dot > 0) _out = '${_out!.substring(0, dot)}.$_coverExt';
                          }
                        });
                      },
              ),
            ],
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _pwCtrl,
            obscureText: !_showPw,
            decoration: InputDecoration(
              labelText: '解压码（可选，留空就不加密）',
              isDense: true,
              border: const OutlineInputBorder(),
              prefixIcon: const Icon(Icons.lock_outline, size: 18),
              suffixIcon: IconButton(
                icon: Icon(
                  _showPw ? Icons.visibility_off : Icons.visibility,
                  size: 18,
                ),
                tooltip: _showPw ? '隐藏解压码' : '显示解压码',
                onPressed: () => setState(() => _showPw = !_showPw),
              ),
            ),
          ),
          const SizedBox(height: 12),
          // 交付选项：默认「校验和 + 说明书」开（DESIGN §14.2），分卷按需
          SwitchListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _split,
            onChanged: _busy ? null : (v) => setState(() => _split = v),
            title: const Text('分成多个卷（大于 4 GB 建议开）', style: TextStyle(fontSize: 13)),
            subtitle: _split
                ? Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: TextField(
                      controller: _splitCtrl,
                      decoration: const InputDecoration(
                        labelText: '每卷大小',
                        hintText: '2G / 500M',
                        isDense: true,
                        border: OutlineInputBorder(),
                      ),
                    ),
                  )
                : null,
          ),
          SwitchListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _checksum,
            onChanged: _busy ? null : (v) => setState(() => _checksum = v),
            title: const Text('生成 SHA-256 校验和', style: TextStyle(fontSize: 13)),
            subtitle: const Text('对方可以 sha256sum -c 验证没下坏',
                style: TextStyle(fontSize: 11)),
          ),
          SwitchListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _readme,
            onChanged: _busy ? null : (v) => setState(() => _readme = v),
            title: const Text('生成说明.txt', style: TextStyle(fontSize: 13)),
            subtitle: const Text('告诉对方这不是视频、怎么打开（有密码会写进去）',
                style: TextStyle(fontSize: 11)),
          ),
          const SizedBox(height: 8),
          if (_error != null)
            Text(
              '打包失败（${_error}）',
              style: TextStyle(color: cs.error, fontSize: 12),
            ),
          if (_result != null) _resultCard(cs),
          const SizedBox(height: 8),
          FilledButton.icon(
            onPressed: _busy || _inputs.isEmpty ? null : _pack,
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.archive),
            label: Text(_busy ? '打包中…' : '开始打包'),
            style: FilledButton.styleFrom(padding: const EdgeInsets.symmetric(vertical: 14)),
          ),
        ],
      ),
    );
  }

  Widget _resultCard(ColorScheme cs) {
    final r = _result!;
    final size = (r['total_size'] as num?)?.toInt() ?? 0;
    final entries = (r['entries'] as num?)?.toInt() ?? 0;
    final layout = r['layout']?.toString() ?? '';
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: cs.primaryContainer.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Text(
        '✅ 打包完成：${_size(size)}，$entries 个条目，'
        '${layout == 'FreeAtom' ? 'free 原子布局（推荐）' : '尾部追加布局'}\n'
        '这个文件能正常播放/打开，对方拖进解压工具也能还原。',
        style: const TextStyle(fontSize: 12, height: 1.5),
      ),
    );
  }

  String _size(int bytes) {
    const k = 1024;
    if (bytes >= k * k * k) return '${(bytes / (k * k * k)).toStringAsFixed(2)} GB';
    if (bytes >= k * k) return '${(bytes / (k * k)).toStringAsFixed(1)} MB';
    if (bytes >= k) return '${(bytes / k).toStringAsFixed(0)} KB';
    return '$bytes B';
  }
}
