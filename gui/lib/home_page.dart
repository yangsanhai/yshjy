import 'dart:async';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'engine.dart';

/// 界面状态机（对应 UI-SPEC 的 S1 ~ S9 / S11）
enum Phase { empty, listing, preview, extracting, done, error }

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final EngineClient _client = EngineClient();
  StreamSubscription<EngineEvent>? _sub;

  Phase _phase = Phase.empty;
  String? _file;
  Preview? _preview;
  Progress? _progress;
  String? _current;
  String _stage = '';
  ExtractResult? _result;
  String? _errorCode;
  String? _errorNote;
  EngineError? _error;
  final List<String> _notes = [];

  @override
  void initState() {
    super.initState();
    // 引擎没启动也别让界面崩：错误在点"选择文件"时才提示
    _client.start().catchError((_) {});
    _sub = _client.events.listen(_onEvent);
  }

  @override
  void dispose() {
    _sub?.cancel();
    _client.dispose();
    super.dispose();
  }

  // ───────────────────────── 事件 → 状态 ─────────────────────────

  void _onEvent(EngineEvent e) {
    if (!mounted) return;
    switch (e.kind) {
      case 'STAGE_STARTED':
        setState(() => _stage = (e.data['stage'] as String?) ?? '');
      case 'EXTRACT_PROGRESS':
        setState(() {
          _progress = e.progress;
          _current = e.current;
          _stage = 'EXTRACT';
        });
      case 'DIAGNOSTIC':
        final code = e.data['code']?.toString() ?? '';
        final text = _diagText(code);
        if (!_notes.contains(text)) setState(() => _notes.add(text));
      case 'PASSWORD_NEEDED':
        if (!_notes.contains('需要解压码')) setState(() => _notes.add('需要解压码'));
      case 'SPACE_SHORTAGE':
        final need = e.data['needed'];
        if (!_notes.contains('磁盘可能不够')) {
          setState(() => _notes.add('磁盘可能不够（需要 ${_size((need as num?)?.toInt() ?? 0)}）'));
        }
      // 不认识的 kind 一律忽略 —— 后端加新事件不会弄坏旧界面
      default:
        break;
    }
  }

  String _diagText(String code) => switch (code) {
        'RISKY_ENTRY' => '解出了可执行文件，运行前请确认来源',
        'REBUILT_INDEX' => '索引坏了，清单是按文件头重建的',
        'VOLUMES_MERGED' => '分卷已自动拼起来',
        'OFFSET_CORRECTED' => '偏移不对，已自动修正',
        'HIDDEN_AT_OFFSET' => '包藏在文件中间，已按真实位置定位',
        'PASSWORD_FROM_NOTES' => '解压码是从旁边的说明里找到的',
        'PARTIAL_DATA' => '数据不完整，能救多少算多少',
        'SLOW_PATH' => '走了慢路（全文件扫描），请耐心等',
        _ => '提示：$code',
      };

  /// 错误码 → 人话（**这就是"引擎只给码、壳查文案"的落地**）
  String _errText(String code) => switch (code) {
        'NOT_ARCHIVE' => '这里面没找到压缩包 —— 可能不是伪装包，或者文件下坏了',
        'PASSWORD_REQUIRED' => '这个包需要解压码',
        'PASSWORD_WRONG' => '解压码不对',
        'PERMISSION_DENIED' => '没有权限读写这个位置（换个目录试试）',
        'PATH_NOT_FOUND' => '文件不存在（是不是被移走或改名了？）',
        'IO' => '读写文件出错了（磁盘满了 / 磁盘有问题）',
        'CANCELLED' => '已取消（已经解出来的文件都保留着）',
        'ENGINE_EXITED' => '引擎进程退出了（重开一次界面就好）',
        'TIMEOUT' => '等太久了，没等到引擎回应',
        _ => '出错了（错误码：$code）',
      };

  // ───────────────────────── 动作 ─────────────────────────

  Future<void> _pick() async {
    const group = XTypeGroup(
      label: '压缩包或伪装文件',
      extensions: [
        'mp4', 'mkv', 'avi', 'mov', 'webm', // 伪装外壳
        'zip', '7z', 'rar', 'tar', 'gz', 'xz', 'zst', 'bz2', 'lz4', 'br', 'z01',
        'png', 'jpg', 'jpeg', 'gif', 'webp', // 图片外壳
        'txt', 'md', // 文本外壳
      ],
    );
    try {
      final f = await openFile(acceptedTypeGroups: const [group]);
      if (f != null) await _open(f.path);
    } on PlatformException catch (e) {
      _showError('FILE_PICKER', e.message ?? '选不了文件');
    }
  }

  Future<void> _open(String path) async {
    setState(() {
      _phase = Phase.listing;
      _file = path;
      _preview = null;
      _result = null;
      _progress = null;
      _notes.clear();
      _errorCode = null;
    });
    try {
      final pv = await _client.list(path);
      if (!mounted) return;
      setState(() {
        _preview = pv;
        _phase = Phase.preview;
      });
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  Future<void> _extract() async {
    final path = _file;
    if (path == null) return;
    setState(() {
      _phase = Phase.extracting;
      _progress = null;
      _current = null;
    });
    try {
      final r = await _client.extract(path);
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  void _showError(String code, String note, [EngineError? e]) {
    if (!mounted) return;
    setState(() {
      _phase = Phase.error;
      _errorCode = code;
      _errorNote = note;
      _error = e;
    });
  }

  /// 文件类型 → 人话（与 CLI 同一张表；将来抽成语言包）
  ///
  /// ⚠ Dart 不支持非 ASCII 标识符，下面变量名一律英文。
  String _kindName(String k) => switch (k) {
        'mp4' => 'MP4 视频',
        'mov' => 'MOV 视频',
        'mkv' => 'MKV 视频',
        'webm' => 'WebM 视频',
        'avi' => 'AVI 视频',
        'flv' => 'FLV 视频',
        'ts' => 'MPEG-TS 视频流',
        'png' => 'PNG 图片',
        'jpeg' => 'JPEG 图片',
        'gif' => 'GIF 图片',
        'webp' => 'WebP 图片',
        'bmp' => 'BMP 图片',
        'tiff' => 'TIFF 图片',
        'mp3' => 'MP3 音频',
        'flac' => 'FLAC 音频',
        'ogg' => 'OGG 音频',
        'wav' => 'WAV 音频',
        'pdf' => 'PDF 文档',
        'elf' => 'Linux 可执行程序',
        'exe' => 'Windows 可执行程序',
        'zip' => 'ZIP 压缩包',
        '7z' => '7z 压缩包',
        'rar' => 'RAR 压缩包',
        'gzip' => 'gzip 压缩流',
        'xz' => 'xz 压缩流',
        'zstd' => 'zstd 压缩流',
        'bzip2' => 'bzip2 压缩流',
        'lz4' => 'LZ4 压缩流',
        'text' => '文本文件',
        _ => '认不出的文件',
      };

  /// 失败时那句「这个文件其实是……」
  ({String text, bool warn})? _typeHint() {
    final e = _error;
    if (e == null || e.detail.isEmpty) return null;
    final size = e.size;
    if (size != null && size == 0) {
      return (
        text: '这是一个空文件（0 字节）—— 一般是下载失败留下的空壳，重新下一次吧',
        warn: false
      );
    }
    final kind = _kindName(e.looksLike ?? 'unknown');
    final sizeText = size == null ? '' : '，${_size(size)}';
    if (e.extMismatch) {
      final extName = '.${e.ext ?? ''}';
      if (e.looksKind == 'archive') {
        return (text: '名字写的是$extName，内容其实是$kind$sizeText —— 这很可能就是伪装包', warn: false);
      }
      if (e.looksKind == 'program') {
        return (text: '⚠ 名字写的是$extName，内容其实是$kind$sizeText —— 来路不明的程序别运行', warn: true);
      }
      return (text: '名字写的是$extName，内容其实是$kind$sizeText —— 可能被改过名', warn: false);
    }
    if (e.looksKind == 'program') {
      return (text: '⚠ 这其实是一个$kind$sizeText —— 来路不明的程序别运行', warn: true);
    }
    if ((e.looksLike ?? '') == 'unknown') {
      return (text: '认不出这是什么文件$sizeText —— 可能被加密过，或者文件头坏了', warn: false);
    }
    return (text: '这个文件看起来是：$kind$sizeText', warn: false);
  }

  Future<void> _openFolder(String? dir) async {
    if (dir == null) return;
    try {
      await Process.run('xdg-open', [dir]);
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('打不开文件管理器：$dir')));
    }
  }

  Future<void> _cancel() async {
    await _client.cancel();
  }

  // ───────────────────────── 界面 ─────────────────────────

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            const Icon(Icons.inventory_2_outlined, size: 22),
            const SizedBox(width: 8),
            const Text('杨三海解压'),
            const SizedBox(width: 10),
            if (_file != null)
              Flexible(
                child: Text(
                  _shortPath(_file!),
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
          ],
        ),
        actions: [
          IconButton(
            onPressed: _phase == Phase.extracting ? null : _pick,
            icon: const Icon(Icons.folder_open),
            tooltip: '选择文件',
          ),
        ],
      ),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 1000),
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: AnimatedSwitcher(
              duration: const Duration(milliseconds: 180),
              child: _body(),
            ),
          ),
        ),
      ),
    );
  }

  Widget _body() {
    return switch (_phase) {
      Phase.empty => _emptyView(),
      Phase.listing => _busyView('正在识别…'),
      Phase.preview => _previewView(),
      Phase.extracting => _progressView(),
      Phase.done => _doneView(),
      Phase.error => _errorView(),
    };
  }

  Widget _emptyView() {
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
          Text('把伪装包交给我', style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 8),
          Text(
            '网盘下载的「.mp4 / .png」，其实可能是压缩包\n我能自己找到它藏在哪，然后解开',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: cs.onSurfaceVariant,
                  height: 1.6,
                ),
          ),
          const SizedBox(height: 28),
          FilledButton.icon(
            onPressed: _pick,
            icon: const Icon(Icons.add),
            label: const Text('选择文件'),
            style: FilledButton.styleFrom(
              padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 16),
            ),
          ),
          const SizedBox(height: 24),
          Text(
            '支持 ZIP / 7z / RAR / TAR / gzip / xz / zstd / bzip2 / lz4 / brotli',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }

  Widget _busyView(String text) {
    return Center(
      key: const ValueKey('busy'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const CircularProgressIndicator(),
          const SizedBox(height: 20),
          Text(text),
        ],
      ),
    );
  }

  Widget _previewView() {
    final pv = _preview!;
    final cs = Theme.of(context).colorScheme;
    return Column(
      key: const ValueKey('preview'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Chip(
              label: Text(pv.format.toUpperCase()),
              visualDensity: VisualDensity.compact,
            ),
            const SizedBox(width: 8),
            Text('${pv.fileCount} 个文件 · ${_size(pv.totalSize)}'),
            const Spacer(),
            if (pv.encryptedCount > 0)
              Chip(
                avatar: const Icon(Icons.lock_outline, size: 16),
                label: Text('${pv.encryptedCount} 个加密'),
                visualDensity: VisualDensity.compact,
              ),
          ],
        ),
        if (!pv.complete)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              '⚠ 索引不完整，下面是能读到的部分',
              style: TextStyle(color: cs.error),
            ),
          ),
        if (_notes.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final n in _notes)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Row(
                      children: [
                        Icon(Icons.info_outline, size: 16, color: cs.primary),
                        const SizedBox(width: 6),
                        Expanded(child: Text(n, style: const TextStyle(fontSize: 13))),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        const SizedBox(height: 12),
        Expanded(
          child: Card(
            clipBehavior: Clip.antiAlias,
            child: ListView.separated(
              itemCount: pv.entries.length,
              separatorBuilder: (_, __) => const Divider(height: 1),
              itemBuilder: (context, i) {
                final e = pv.entries[i];
                return ListTile(
                  dense: true,
                  leading: Icon(
                    e.isDir ? Icons.folder_outlined : Icons.insert_drive_file_outlined,
                    size: 20,
                    color: e.isDir ? cs.primary : cs.onSurfaceVariant,
                  ),
                  title: Text(
                    e.path,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 13,
                      fontWeight: e.isDir ? FontWeight.w500 : FontWeight.normal,
                    ),
                  ),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (e.encrypted) const Icon(Icons.lock, size: 15),
                      if (e.risky)
                        const Padding(
                          padding: EdgeInsets.only(left: 4),
                          child: Icon(Icons.warning_amber_rounded, size: 16),
                        ),
                      const SizedBox(width: 8),
                      SizedBox(
                        width: 76,
                        child: Text(
                          e.isDir ? '' : _size(e.size),
                          textAlign: TextAlign.right,
                          style: const TextStyle(fontSize: 12),
                        ),
                      ),
                    ],
                  ),
                );
              },
            ),
          ),
        ),
        const SizedBox(height: 16),
        Row(
          children: [
            OutlinedButton.icon(
              onPressed: () => setState(() => _phase = Phase.empty),
              icon: const Icon(Icons.close),
              label: const Text('取消'),
            ),
            const Spacer(),
            FilledButton.icon(
              onPressed: _extract,
              icon: const Icon(Icons.download),
              label: Text('开始解压（${pv.fileCount} 个文件）'),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
              ),
            ),
          ],
        ),
      ],
    );
  }

  Widget _progressView() {
    final p = _progress;
    final cs = Theme.of(context).colorScheme;
    final frac = p?.fraction;
    return Center(
      key: const ValueKey('extract'),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('正在解压…', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 16),
            ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: LinearProgressIndicator(
                value: frac, // null = 不确定进度（未知总大小时）
                minHeight: 10,
              ),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: Text(
                    _current ?? '准备中…',
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 12),
                  ),
                ),
                Text(
                  p == null
                      ? ''
                      : '${_size(p.doneBytes)} / ${p.totalBytes == 0 ? "?" : _size(p.totalBytes)}',
                  style: const TextStyle(fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              _stage == 'LOCATE'
                  ? '第 1 步：在文件里找压缩包'
                  : _stage == 'LIST'
                      ? '第 2 步：读目录'
                      : '第 3 步：写文件（可随时取消，已解出的会保留）',
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 20),
            Align(
              alignment: Alignment.centerRight,
              child: OutlinedButton.icon(
                onPressed: _cancel,
                icon: const Icon(Icons.stop_circle_outlined),
                label: const Text('取消'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _doneView() {
    final r = _result!;
    final cs = Theme.of(context).colorScheme;
    final ok = r.ok;
    final seconds = (r.millis / 1000).toStringAsFixed(1);
    return Center(
      key: const ValueKey('done'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            ok ? Icons.check_circle_outline : Icons.info_outline,
            size: 72,
            color: ok ? cs.primary : cs.tertiary,
          ),
          const SizedBox(height: 16),
          Text(
            ok ? '解开了 ${r.entriesDone} 个文件' : '只解出了一部分（${r.entriesDone}/${r.entriesTotal}）',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            '用时 $seconds 秒 · ${_size(r.bytesWritten)}',
            style: TextStyle(color: cs.onSurfaceVariant),
          ),
          if (r.dest != null) ...[
            const SizedBox(height: 6),
            Text(
              r.dest!,
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 12),
            ),
          ],
          if (_notes.isNotEmpty) ...[
            const SizedBox(height: 14),
            for (final n in _notes)
              Padding(
                padding: const EdgeInsets.only(bottom: 4),
                child: Text('· $n', style: const TextStyle(fontSize: 12)),
              ),
          ],
          const SizedBox(height: 26),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (r.dest != null)
                FilledButton.icon(
                  onPressed: () => _openFolder(r.dest),
                  icon: const Icon(Icons.folder_open),
                  label: const Text('打开目录'),
                ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: () => setState(() {
                  _phase = Phase.empty;
                  _file = null;
                  _preview = null;
                  _result = null;
                  _notes.clear();
                }),
                icon: const Icon(Icons.add),
                label: const Text('再处理一个'),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _errorView() {
    final cs = Theme.of(context).colorScheme;
    return Center(
      key: const ValueKey('error'),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, size: 64, color: cs.error),
            const SizedBox(height: 16),
            Text(
              _errText(_errorCode ?? 'UNKNOWN'),
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.titleMedium,
            ),
            if (_typeHint() case final hint?) ...[
              const SizedBox(height: 12),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                decoration: BoxDecoration(
                  color: hint.warn
                      ? cs.errorContainer.withValues(alpha: 0.35)
                      : cs.surfaceContainerHighest,
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Text(
                  hint.text,
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    fontSize: 13,
                    color: hint.warn ? cs.error : null,
                    height: 1.5,
                  ),
                ),
              ),
            ],
            if ((_errorNote ?? '').isNotEmpty) ...[
              const SizedBox(height: 10),
              SelectableText(
                _errorNote!,
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
              ),
            ],
            const SizedBox(height: 24),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                FilledButton.icon(
                  onPressed: _pick,
                  icon: const Icon(Icons.folder_open),
                  label: const Text('换个文件'),
                ),
                const SizedBox(width: 12),
                OutlinedButton(
                  onPressed: () => setState(() => _phase = Phase.empty),
                  child: const Text('返回'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  // ───────────────────────── 小工具 ─────────────────────────

  String _size(int bytes) {
    const k = 1024;
    if (bytes >= k * k * k) return '${(bytes / (k * k * k)).toStringAsFixed(2)} GB';
    if (bytes >= k * k) return '${(bytes / (k * k)).toStringAsFixed(1)} MB';
    if (bytes >= k) return '${(bytes / k).toStringAsFixed(0)} KB';
    return '$bytes B';
  }

  String _shortPath(String p) {
    final parts = p.split('/').where((s) => s.isNotEmpty).toList();
    if (parts.length <= 2) return p;
    return '…/${parts.sublist(parts.length - 2).join('/')}';
  }
}
