import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../engine.dart';
import '../notify.dart';

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
  /// 打包模式（GUI-PLAN §5.2②）：普通压缩是默认，伪装是特殊功能
  String _mode = 'plain';
  /// 用户挑的伪装文件（外壳）：任意类型，用户选什么就是什么
  String? _coverPath;
  int _coverSize = 0;
  /// 外壳扩展名 —— 决定输出文件的扩展名（必须一致，不然一认文件头就露馅）
  String _coverExt = '';
  /// 自解压（第 23 批）：生成双击就解压的包，不需要外壳文件
  bool _sfx = false;
  /// 自解压种类（P23-5）：windows = exe、linux = .sh
  String _sfxKind = 'windows';
  final TextEditingController _pwCtrl = TextEditingController();
  /// 解压码是否显示明文（眼睛图标）
  bool _showPw = false;
  /// 交付选项（DESIGN §17.8）：分卷 / 校验和 / 说明书
  /// 压缩级别（P23-4）：0 = 仅存储最快 / 6 = 标准 / 9 = 最小体积
  int _level = 6;
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

  /// 当前模式对应的输出扩展名
  /// （普通 = .zip；自解压 = .exe / .sh；伪装 = 外壳的扩展名）
  String get _ext => _mode == 'plain'
      ? 'zip'
      : (_sfx
          ? (_sfxKind == 'linux' ? 'sh' : 'exe')
          : (_coverExt.isEmpty ? 'mp4' : _coverExt));

  /// 从路径里取小写扩展名（没有就空串）
  String _extOf(String path) {
    final base = path.split('/').last;
    final dot = base.lastIndexOf('.');
    return dot > 0 ? base.substring(dot + 1).toLowerCase() : '';
  }

  /// 把输出名的扩展名换成 `ext`（保留目录和主名）
  String _withExt(String path, String ext) {
    final dot = path.lastIndexOf('.');
    final slash = path.lastIndexOf('/');
    final stem = dot > slash ? path.substring(0, dot) : path;
    return '$stem.$ext';
  }

  String _suggestOut(String first) {
    final base = first.split('/').last;
    final stem = base.contains('.') ? base.substring(0, base.lastIndexOf('.')) : base;
    final dir = first.substring(0, first.lastIndexOf('/') + 1);
    return '$dir$stem.$_ext';
  }

  /// 挑伪装文件：任意类型都行（不设白名单）。
  ///
  /// 用户选什么，产物就是「这个文件的完整内容 + 尾部压缩数据」。
  /// 输出扩展名自动跟着外壳走 —— 名字和内容不一致会一认文件头就露馅。
  Future<void> _pickCover() async {
    // 不传 extensions / mimeTypes = 允许所有文件（任意类型都能当外壳）
    const group = XTypeGroup(label: '所有文件');
    final f = await openFile(acceptedTypeGroups: const [group]);
    if (f == null) return;
    final size = File(f.path).lengthSync();
    setState(() {
      _coverPath = f.path;
      _coverSize = size;
      final ext = _extOf(f.path);
      _coverExt = ext.isEmpty ? 'bin' : ext;
      if (_out != null) {
        _out = _withExt(_out!, _coverExt);
      } else if (_inputs.isNotEmpty) {
        _out = _suggestOut(_inputs.first);
      }
    });
  }

  Future<void> _pickOut() async {
    final loc = await getSaveLocation(
        suggestedName: _out?.split('/').last ?? 'packed.$_ext');
    if (loc == null) return;
    setState(() {
      if (_mode == 'plain') {
        // 普通压缩产物就是 ZIP，名字必须对得上（用户手打 .7z 也不让）
        _out = _withExt(loc.path, 'zip');
      } else {
        _out = _withExt(loc.path, _ext);
      }
    });
  }

  /// 分卷大小的实时校验（看不懂就红字，绝不静默变成「不分卷」）
  String? get _splitSizeError {
    if (!_split) return null;
    final t = _splitCtrl.text.trim();
    if (t.isEmpty) return '填个大小，例如 2G';
    if (_parseSize(t) == null) return '看不懂这个大小，用 2G / 500M / 1048576';
    return null;
  }

  /// 错误码 → 人话（界面别直接糊 code）
  String _errorText(String code) => switch (code) {
        'NO_COVER' => '伪装模式要先选一个伪装文件（在下面「伪装文件」那里选）',
        'SPLIT_SIZE_INVALID' => '分卷大小填得不对，改一下再开始',
        'BAD_PARAM' => '参数不对：输出文件不能和伪装文件/源文件是同一个',
        'PATH_NOT_FOUND' => '找不到伪装文件（是不是被移走了？）',
        _ => '打包失败（$code）',
      };

  Future<void> _pack() async {
    if (_inputs.isEmpty || _out == null) return;
    final isSfx = _mode == 'disguise' && _sfx;
    if (!isSfx && _mode == 'disguise' && _coverPath == null) {
      setState(() => _error = 'NO_COVER');
      return;
    }
    final splitErr = _splitSizeError;
    if (splitErr != null) {
      setState(() => _error = 'SPLIT_SIZE_INVALID');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
      _result = null;
    });
    widget.onStatus?.call('正在打包…');
    try {
      // 普通压缩产物永远是 ZIP；伪装产物扩展名必须和外壳一致
      final out = _mode == 'plain'
          ? _withExt(_out!, 'zip')
          : _withExt(_out!, _ext);
      final r = await _client.pack(
        inputs: _inputs,
        out: out,
        password: _pwCtrl.text.trim().isEmpty ? null : _pwCtrl.text.trim(),
        cover: _mode == 'disguise' && !isSfx ? _coverPath : null,
        sfxKind: isSfx ? _sfxKind : null,
        splitBytes: !isSfx && _split ? _parseSize(_splitCtrl.text) : null,
        level: _level,
        checksum: _checksum,
        // 普通压缩不写「这不是视频」的说明书（用户验收 2026-10-17）；自解压也不需要
        readme: _mode == 'disguise' && !isSfx ? _readme : false,
        mode: _mode,
      );
      if (!mounted) return;
      setState(() => _result = r);
      widget.onStatus?.call('打包完成');
      Notifier.send('打包完成', '输出目录里看结果，或点历史页');
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
          Text(
            _mode == 'plain' ? '压缩文件' : '打包成伪装文件',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 4),
          Text(
            _mode == 'plain'
                ? '把文件夹/文件压成一个 .zip，任何解压工具都能打开'
                : '选一个真实文件当外壳，压缩包藏在它后面 —— 外壳是什么，产物就是什么',
            style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
          ),
          const SizedBox(height: 12),
          // 模式二选一（GUI-PLAN §5.2②）：普通压缩是默认，伪装是特殊功能
          SegmentedButton<String>(
            segments: const [
              ButtonSegment(
                value: 'plain',
                icon: Icon(Icons.folder_zip_outlined, size: 16),
                label: Text('普通压缩（.zip）'),
              ),
              ButtonSegment(
                value: 'disguise',
                icon: Icon(Icons.movie_outlined, size: 16),
                label: Text('伪装成文件'),
              ),
            ],
            selected: {_mode},
            onSelectionChanged: _busy
                ? null
                : (s) {
                    setState(() {
                      _mode = s.first;
                      // 输出名跟着换扩展名（普通 = .zip；伪装 = 外壳的扩展名）
                      if (_out != null) _out = _withExt(_out!, _ext);
                    });
                  },
          ),
          if (_mode == 'disguise') ...[
            const SizedBox(height: 4),
            SwitchListTile(
              key: const ValueKey('sfx-toggle'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _sfx,
              onChanged: _busy
                  ? null
                  : (v) => setState(() {
                        _sfx = v;
                        if (_out != null) _out = _withExt(_out!, _ext);
                      }),
              secondary: const Icon(Icons.window_outlined, size: 20),
              title: const Text('生成自解压包（双击就解压）', style: TextStyle(fontSize: 13)),
              subtitle: const Text('别人不用装任何软件；支持解压码，不支持分卷',
                  style: TextStyle(fontSize: 11)),
            ),
            if (_sfx)
              Padding(
                padding: const EdgeInsets.only(left: 40, right: 4, bottom: 6),
                child: Row(
                  children: [
                    const Text('目标系统：', style: TextStyle(fontSize: 12)),
                    DropdownButton<String>(
                      key: const ValueKey('sfx-kind'),
                      value: _sfxKind,
                      isDense: true,
                      style: const TextStyle(fontSize: 12),
                      items: const [
                        DropdownMenuItem(
                            value: 'windows', child: Text('Windows（.exe）')),
                        DropdownMenuItem(
                            value: 'linux', child: Text('Linux（.sh）')),
                      ],
                      onChanged: _busy
                          ? null
                          : (v) => setState(() {
                                _sfxKind = v ?? 'windows';
                                if (_out != null) _out = _withExt(_out!, _ext);
                              }),
                    ),
                  ],
                ),
              ),
          ],
          const SizedBox(height: 8),
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
          // 伪装文件（外壳）：用户想选什么就选什么，没有任何类型限制
          if (_mode == 'disguise' && !_sfx) ...[
            Row(
              children: [
                const Expanded(
                  child: Text('伪装文件',
                      style: TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
                ),
                if (_coverPath != null)
                  TextButton(
                    onPressed: _busy ? null : _pickCover,
                    child: const Text('换一个', style: TextStyle(fontSize: 12)),
                  ),
              ],
            ),
            const SizedBox(height: 4),
            if (_coverPath == null)
              OutlinedButton.icon(
                key: const ValueKey('pick-cover'),
                onPressed: _busy ? null : _pickCover,
                icon: const Icon(Icons.folder_open_outlined, size: 18),
                label: const Text('选择伪装文件（任意类型）'),
                style: OutlinedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                ),
              )
            else
              Card(
                margin: EdgeInsets.zero,
                clipBehavior: Clip.antiAlias,
                child: ListTile(
                  dense: true,
                  leading: const Icon(Icons.insert_drive_file_outlined, size: 20),
                  title: Text(
                    _coverPath!.split('/').last,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 13),
                  ),
                  subtitle: Text(
                    '${_size(_coverSize)} · ${_coverExt.isEmpty ? '无扩展名' : '.$_coverExt'}',
                    style: const TextStyle(fontSize: 11),
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.close, size: 16),
                    tooltip: '清掉，重新选',
                    onPressed: _busy
                        ? null
                        : () => setState(() {
                              _coverPath = null;
                              _coverSize = 0;
                              _coverExt = '';
                            }),
                  ),
                ),
              ),
            const SizedBox(height: 6),
            Text(
              '这个文件会原样放在最前面，压缩数据追加在它后面。'
              '视频、图片、PDF、音乐、exe、安装包……选什么都行。\n'
              '选 exe / 安装包时，别人双击运行的是这个程序本身；'
              '建议挑一个体积和内容相称的真实文件，太小的外壳看起来会不自然。',
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant, height: 1.5),
            ),
          ],
          const SizedBox(height: 10),
          // 只读展示（不用 TextField：每帧 new controller 会丢状态）
          InputDecorator(
            decoration: InputDecoration(
              labelText: '输出文件',
              isDense: true,
              border: const OutlineInputBorder(),
              suffixIcon: IconButton(
                icon: const Icon(Icons.folder_open, size: 18),
                onPressed: _busy ? null : _pickOut,
              ),
            ),
            child: Text(
              _out ?? '（还没选输出位置）',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12),
            ),
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _pwCtrl,
            obscureText: !_showPw,
            decoration: InputDecoration(
              // 第 23 批 P23-5：自解压也支持解压码了（存根会提示输入）
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
          // 压缩级别（P23-4）：以前只在引擎里有，CLI/GUI 都没暴露
          ListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.speed, size: 18),
            title: const Text('压缩级别', style: TextStyle(fontSize: 13)),
            subtitle: const Text('越小越快、越大越省空间（默认「标准」）',
                style: TextStyle(fontSize: 11)),
            trailing: DropdownButton<int>(
              value: _level,
              isDense: true,
              style: const TextStyle(fontSize: 12),
              items: const [
                DropdownMenuItem(value: 0, child: Text('仅存储（最快）')),
                DropdownMenuItem(value: 1, child: Text('很快')),
                DropdownMenuItem(value: 6, child: Text('标准')),
                DropdownMenuItem(value: 9, child: Text('最小体积（最慢）')),
              ],
              onChanged: _busy ? null : (v) => setState(() => _level = v ?? 6),
            ),
          ),
          // 交付选项：默认「校验和 + 说明书」开（DESIGN §14.2），分卷按需
          SwitchListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _split,
            onChanged: _busy || _sfx ? null : (v) => setState(() => _split = v),
            title: Text(
                _sfx ? '分成多个卷（自解压不支持）' : '分成多个卷（大于 4 GB 建议开）',
                style: const TextStyle(fontSize: 13)),
            subtitle: _split
                ? Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: TextField(
                      controller: _splitCtrl,
                      onChanged: (_) => setState(() {}),
                      decoration: InputDecoration(
                        labelText: '每卷大小',
                        hintText: '2G / 500M',
                        errorText: _splitSizeError,
                        isDense: true,
                        border: const OutlineInputBorder(),
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
          // 说明书是给「伪装包」用的（告诉对方这不是视频）；普通压缩和自解压不需要
          if (_mode == 'disguise' && !_sfx)
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
              _errorText(_error!),
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
            label: Text(
              _busy
                  ? (_mode == 'plain' ? '压缩中…' : (_sfx ? '生成中…' : '打包中…'))
                  : (_mode == 'plain'
                      ? '开始压缩'
                      : (_sfx ? '开始生成自解压 exe' : '开始打包（伪装）')),
            ),
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
    final mode = r['mode']?.toString() ?? 'disguise';
    final plain = mode == 'plain';
    final cover = r['cover']?.toString();
    final coverName = cover?.split('/').last;
    final sfx = r['sfx'] == true;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: cs.primaryContainer.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Text(
        sfx
            ? '✅ 自解压包完成：${_size(size)}，$entries 个条目\n'
                '把这个 exe 发给别人（Windows），双击就会解压到它旁边的新文件夹，不用装任何软件。'
            : plain
                ? '✅ 压缩完成：${_size(size)}，$entries 个条目\n'
                    '这是一个标准 ZIP，任何解压工具都能直接打开。'
                : '✅ 打包完成：${_size(size)}，$entries 个条目，'
                    '${layout == 'FreeAtom' ? 'free 原子布局（推荐）' : '尾部追加布局'}\n'
                    '${coverName == null ? '' : '外壳：$coverName\n'}'
                    '外壳文件原样保留，双击打开的还是它；压缩数据藏在尾部，拖进杨三海解压就能还原。',
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
