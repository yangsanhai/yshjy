import 'dart:async';
import 'dart:io';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import '../history_store.dart';

/// 界面状态机（对应 UI-SPEC 的 S1 ~ S9 / S11）
enum Phase { empty, listing, preview, extracting, done, error, queue }

/// 队列里的一行（S13 批量队列）
class _QueueItem {
  final String path;
  final String name;
  final int size;
  final String kind;
  final int volumes;
  final bool archive;

  /// pending / running / ok / partial / failed
  String status = 'pending';

  _QueueItem({
    required this.path,
    required this.name,
    required this.size,
    required this.kind,
    required this.volumes,
    required this.archive,
  });
}

class HomePage extends StatefulWidget {
  /// 启动时要直接打开的文件（右键菜单传进来的），可能为空
  final String? initialFile;

  /// 把当前状态同步给外壳的底部状态栏（"正在解压…"/"就绪"）
  final ValueChanged<String>? onStatus;

  /// 是否自动拉起引擎（widget 测试里必须关掉）
  final bool autoStart;

  const HomePage({super.key, this.initialFile, this.onStatus, this.autoStart = true});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final EngineClient _client = EngineClient.instance;
  StreamSubscription<EngineEvent>? _sub;

  Phase _phase = Phase.empty;
  String? _file;
  Preview? _preview;
  Progress? _progress;
  String? _current;
  String _stage = '';
  ExtractResult? _result;
  String? _errorCode;

  /// 引擎给的结构化细节（如缺分卷的 missing/found）—— 失败卡据此说"缺哪一块"
  Map<String, dynamic>? _errorDetail;
  String? _errorNote;
  EngineError? _error;
  final TextEditingController _pwCtrl = TextEditingController();
  bool _dragging = false;

  /// 队列（S13）与批量结果
  final List<_QueueItem> _queue = [];
  List<String> _batchOrder = [];
  Map<String, dynamic>? _batch;
  String? _password;
  /// 解压码是否显示明文（眼睛图标，调研 07:383「是标配，必须做」）
  bool _showPw = false;
  /// 速度与 ETA（壳侧用两次 progress 的时间差自算，调研 07:248 要求超过 Bandizip）
  DateTime? _lastTick;
  int _lastBytes = 0;
  double _speed = 0; // 字节/秒（滑动平均）
  /// 自动试密码的进度（引擎 PASSWORD_TRY 事件）
  int _pwTry = 0;
  int _pwTotal = 0;
  /// 全文件扫描进度（引擎 SCAN_PROGRESS 事件）
  int _scanDone = 0;
  int _scanTotal = 0;
  /// 密码本里的常用解压码（点一下填入）
  List<String> _recentPw = [];
  final List<String> _notes = [];

  @override
  void initState() {
    super.initState();
    // 引擎没启动也别让界面崩：错误在点"选择文件"时才提示
    if (widget.autoStart) _client.start().catchError((_) {});
    _sub = _client.events.listen(_onEvent);
    _loadRecentPasswords();
    // 从右键菜单带文件启动：等首帧画完再开，避免 setState 早于 build
    final f = widget.initialFile;
    if (f != null && f.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _open(f));
    }
  }

  @override
  void dispose() {
    _pwCtrl.dispose();
    _sub?.cancel();
    _client.dispose();
    super.dispose();
  }

  // ───────────────────────── 事件 → 状态 ─────────────────────────

  void _onEvent(EngineEvent e) {
    if (!mounted) return;
    switch (e.kind) {
      case 'JOB_STARTED':
        final jobNo = ((e.data['job'] ?? e.data['id']) as num?)?.toInt();
        if (jobNo != null && jobNo >= 1 && jobNo <= _batchOrder.length) {
          final path = _batchOrder[jobNo - 1];
          final idx = _queue.indexWhere((q) => q.path == path);
          if (idx >= 0) {
            setState(() {
              _queue[idx].status = 'running';
              _current = _queue[idx].name;
            });
          }
        }
      case 'STAGE_STARTED':
        setState(() => _stage = (e.data['stage'] as String?) ?? '');
      case 'SCAN_PROGRESS':
        final b = e.data['bytes'];
        if (b is List && b.length == 2) {
          setState(() {
            _scanDone = (b[0] as num?)?.toInt() ?? 0;
            _scanTotal = (b[1] as num?)?.toInt() ?? 0;
            _stage = 'LOCATE';
          });
        }
      case 'EXTRACT_PROGRESS':
        final p = e.progress;
        setState(() {
          _progress = p;
          _current = e.current;
          _stage = 'EXTRACT';
          if (p != null) _tickSpeed(p.doneBytes);
        });
      case 'DIAGNOSTIC':
        final code = e.data['code']?.toString() ?? '';
        final text = _diagText(code);
        if (!_notes.contains(text)) setState(() => _notes.add(text));
      case 'PASSWORD_TRY':
        setState(() {
          _pwTry = (e.data['index'] as num?)?.toInt() ?? 0;
          _pwTotal = (e.data['total'] as num?)?.toInt() ?? 0;
        });
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
        'MISSING_VOLUME' => '分卷不齐 —— 缺了一块',
        'TRUNCATED' => '文件没下完（数据不完整）',
        'DATA_LOST' => '文件坏了，只能救回一部分',
        'ENCRYPTED_UNBREAKABLE' => '这个包加密了，而且没法试出解压码',
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
      // 多选：一次挑一堆包是常态（网盘一次下 8 个）
      final files = await openFiles(acceptedTypeGroups: const [group]);
      if (files.isEmpty) return;
      if (files.length == 1) {
        await _open(files.first.path);
      } else {
        await _scan(files.map((f) => f.path).toList());
      }
    } on PlatformException catch (e) {
      _showError('FILE_PICKER', e.message ?? '选不了文件');
    }
  }

  /// 选一个文件夹：目录里可能混着一堆无关文件，交给引擎筛
  Future<void> _pickFolder() async {
    try {
      final dir = await getDirectoryPath();
      if (dir != null) await _scan([dir]);
    } on PlatformException catch (e) {
      _showError('FILE_PICKER', e.message ?? '选不了文件夹');
    }
  }

  /// 扫一批路径 → 队列（目录展开、分卷归并都在引擎里做）
  Future<void> _scan(List<String> paths) async {
    setState(() {
      _phase = Phase.listing;
      _queue.clear();
      _batch = null;
      _notes.clear();
      _errorCode = null;
    });
    try {
      final r = await _client.scan(paths);
      if (!mounted) return;
      final items = (r['items'] as List?) ?? const [];
      setState(() {
        for (final raw in items) {
          final i = (raw as Map).cast<String, dynamic>();
          _queue.add(_QueueItem(
            path: i['path'] as String? ?? '',
            name: i['name'] as String? ?? '',
            size: (i['size'] as num?)?.toInt() ?? 0,
            kind: i['kind'] as String? ?? 'UNKNOWN',
            volumes: (i['volumes'] as num?)?.toInt() ?? 1,
            archive: i['archive'] == true,
          ));
        }
        _phase = Phase.queue;
      });
      widget.onStatus?.call('找到 ${_queue.where((q) => q.archive).length} 个可解包');
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  /// 开始批量解压（走引擎 `batch`，一次调用跑完整个队列）
  Future<void> _startQueue() async {
    final ready = _queue.where((q) => q.archive).toList();
    if (ready.isEmpty) return;
    setState(() {
      _phase = Phase.extracting;
      _batch = null;
      _progress = null;
      _current = null;
      for (final q in ready) {
        q.status = 'pending';
      }
      _batchOrder = ready.map((q) => q.path).toList();
    });
    widget.onStatus?.call('正在批量解压 ${ready.length} 个…');
    try {
      final r = await _client.batch(
        paths: _batchOrder,
        password: _password,
      );
      if (!mounted) return;
      final jobs = (r['jobs'] as List?) ?? const [];
      setState(() {
        for (final raw in jobs) {
          final j = (raw as Map).cast<String, dynamic>();
          final path = j['path'] as String? ?? '';
          final idx = _queue.indexWhere((q) => q.path == path);
          if (idx < 0) continue;
          final oc = j['outcome'] as String? ?? '';
          _queue[idx].status =
              oc == 'ok' ? 'ok' : (oc == 'partial' ? 'partial' : 'failed');
        }
        _batch = r;
        _phase = Phase.done;
      });
      final ok = (r['ok'] as num?)?.toInt() ?? 0;
      final failed = (r['failed'] as num?)?.toInt() ?? 0;
      widget.onStatus?.call('批量完成：$ok 成功 / $failed 失败');
      // 历史：每个任务一条
      for (final raw in jobs) {
        final j = (raw as Map).cast<String, dynamic>();
        final oc = j['outcome'] as String? ?? '';
        await HistoryStore.add(HistoryEntry(
          time: DateTime.now().toIso8601String(),
          input: j['path'] as String? ?? '',
          dest: j['dest'] as String?,
          ok: oc == 'ok' || oc == 'partial',
          bytes: (j['bytes'] as num?)?.toInt() ?? 0,
        ));
      }
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  void _resetQueue() {
    setState(() {
      _queue.clear();
      _batch = null;
      _phase = Phase.empty;
      _file = null;
    });
  }

  Future<void> _open(String path) async {
    // 拖进来的是文件夹 → 队列模式（目录里通常混着一堆无关文件）
    if (await Directory(path).exists()) {
      await _scan([path]);
      return;
    }
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
      widget.onStatus?.call('已识别：${pv.fileCount} 个文件');
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
      widget.onStatus?.call('正在解压…');
      final r = await _client.extract(path, password: _password);
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
      widget.onStatus?.call(r.ok ? '完成：${r.entriesDone} 个文件' : '部分完成');
      // 记一条历史（历史页读它）
      await HistoryStore.add(HistoryEntry(
        time: DateTime.now().toIso8601String(),
        input: path,
        dest: r.dest,
        ok: r.ok,
        bytes: r.bytesWritten,
      ));
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  void _showError(String code, String note, [EngineError? e]) {
    if (!mounted) return;
    setState(() {
      _phase = Phase.error;
      _errorCode = code;
      _errorDetail = e?.detail;
      _errorNote = note;
      _error = e;
    });
  }

  /// 一键生成诊断包（路径已脱敏），发给开发者就能定位问题
  Future<void> _makeReport() async {
    try {
      final r = await _client.report(path: _file);
      final p = r['path'] as String? ?? '';
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('诊断包已生成（路径已脱敏）：$p\n把它发给我就行')),
      );
    } on EngineError catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('生成失败：${_errText(e.code)}')));
    }
  }

  /// 从清单里猜「接下来该点哪个」——解压出一堆文件后新手最迷茫的就是这个
  String? _entryHint() {
    final pv = _preview;
    if (pv == null) return null;
    bool looksLikeEntry(String n) {
      final l = n.toLowerCase();
      return l.contains('setup') ||
          l.contains('install') ||
          l.contains('启动') ||
          l.contains('开始游戏') ||
          l.endsWith('.bat') ||
          l.endsWith('run.sh') ||
          l.endsWith('.appimage') ||
          l.endsWith('game.exe');
    }

    final hits = pv.entries.where((e) => !e.isDir && looksLikeEntry(e.path)).toList();
    return hits.isEmpty ? null : hits.first.path;
  }

  /// 这次失败是不是"缺解压码"（资源分享里最常见的情况）
  bool get _needsPassword =>
      _errorCode == 'PASSWORD_REQUIRED' || _errorCode == 'PASSWORD_WRONG';

  /// 用用户输入的密码再试一次
  Future<void> _retryWithPassword() async {
    final pw = _pwCtrl.text.trim();
    if (pw.isEmpty || _file == null) return;
    setState(() {
      _password = pw;
      _phase = Phase.extracting;
      _progress = null;
      _current = null;
    });
    try {
      final r = await _client.extract(_file!, password: pw);
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
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
    // 整个窗口都能拖进来（新手第一反应就是拖）
    return DropTarget(
      onDragEntered: (_) => setState(() => _dragging = true),
      onDragExited: (_) => setState(() => _dragging = false),
      onDragDone: (detail) {
        setState(() => _dragging = false);
        if (detail.files.isEmpty) return;
        final paths = detail.files.map((f) => f.path).toList();
        if (paths.length == 1) {
          _open(paths.first);
        } else {
          _scan(paths);
        }
      },
      child: _scaffold(),
    );
  }

  Widget _scaffold() {
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
          constraints: const BoxConstraints(maxWidth: 920),
          child: Padding(
            padding: const EdgeInsets.all(16),
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
      Phase.done => _batch == null ? _doneView() : _batchDoneView(),
      Phase.queue => _queueView(),
      Phase.error => _errorView(),
    };
  }

  // ───────────────────────── 批量队列（S13）─────────────────────────

  Widget _queueView() {
    final cs = Theme.of(context).colorScheme;
    final ready = _queue.where((q) => q.archive).length;
    return Column(
      key: const ValueKey('queue'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Text('找到 $ready 个可解包',
                style: Theme.of(context).textTheme.titleMedium),
            if (_queue.length > ready)
              Padding(
                padding: const EdgeInsets.only(left: 8),
                child: Text('（跳过 ${_queue.length - ready} 个无关文件）',
                    style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant)),
              ),
            const Spacer(),
            TextButton(onPressed: _resetQueue, child: const Text('清空')),
          ],
        ),
        // P2 智能解压：规则在引擎里（Bandizip 三规则），但要让用户知道会发生什么
        Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: Text(
            '会按包里的结构自动决定要不要多建一层文件夹：只有一个顶层目录就直接铺开，'
            '一堆散文件才包一层（跟 Bandizip 的「智能解压」同一套规则）',
            style: TextStyle(fontSize: 11, height: 1.5, color: cs.onSurfaceVariant),
          ),
        ),
        const SizedBox(height: 0),
        Expanded(
          child: Card(
            clipBehavior: Clip.antiAlias,
            child: ListView.separated(
              itemCount: _queue.length,
              separatorBuilder: (_, __) => const Divider(height: 1),
              itemBuilder: (context, i) {
                final q = _queue[i];
                return ListTile(
                  dense: true,
                  leading: Icon(
                    q.archive ? Icons.inventory_2_outlined : Icons.help_outline,
                    size: 20,
                    color: q.archive ? cs.primary : cs.onSurfaceVariant,
                  ),
                  title: Row(
                    children: [
                      Flexible(
                        child: Text(q.name,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(fontSize: 13)),
                      ),
                      if (q.volumes > 1) ...[
                        const SizedBox(width: 6),
                        Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 6, vertical: 1),
                          decoration: BoxDecoration(
                            color: cs.secondaryContainer,
                            borderRadius: BorderRadius.circular(4),
                          ),
                          child: Text('分卷 ${q.volumes}',
                              style: TextStyle(
                                  fontSize: 10, color: cs.onSecondaryContainer)),
                        ),
                      ],
                    ],
                  ),
                  subtitle: Text(
                    q.archive ? '${q.kind.toUpperCase()} · ${_size(q.size)}'
                        : '看起来不是压缩包 · ${_size(q.size)}',
                    style: const TextStyle(fontSize: 11),
                  ),
                  trailing: _queueStatusChip(q),
                );
              },
            ),
          ),
        ),
        const SizedBox(height: 12),
        FilledButton.icon(
          onPressed: ready == 0 ? null : _startQueue,
          icon: const Icon(Icons.play_arrow),
          label: Text(ready == 0 ? '没有可解压的包' : '开始解压全部（$ready 个）'),
          style: FilledButton.styleFrom(
              padding: const EdgeInsets.symmetric(vertical: 14)),
        ),
      ],
    );
  }

  Widget _queueStatusChip(_QueueItem q) {
    final cs = Theme.of(context).colorScheme;
    final (text, color) = switch (q.status) {
      'running' => ('处理中', cs.primary),
      'ok' => ('完成', cs.primary),
      'partial' => ('部分', Colors.orange),
      'failed' => ('失败', cs.error),
      _ => ('等待', cs.onSurfaceVariant),
    };
    if (q.status == 'pending' && !q.archive) {
      return Text('跳过', style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant));
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (q.status == 'running')
          const SizedBox(
            width: 12, height: 12,
            child: CircularProgressIndicator(strokeWidth: 1.5),
          )
        else
          Icon(
            switch (q.status) {
              'ok' => Icons.check_circle_outline,
              'partial' => Icons.warning_amber_outlined,
              'failed' => Icons.error_outline,
              _ => Icons.schedule,
            },
            size: 14,
            color: color,
          ),
        const SizedBox(width: 4),
        Text(text, style: TextStyle(fontSize: 11, color: color)),
      ],
    );
  }

  /// 批量完成卡（Sunpack 三段式：结果标题 / 量化副标题 / 直达按钮）
  Widget _batchDoneView() {
    final cs = Theme.of(context).colorScheme;
    final r = _batch!;
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
    final firstDest = jobs
        .cast<Map>()
        .map((j) => j['dest'] as String?)
        .firstWhere((d) => d != null, orElse: () => null);

    return Column(
      key: const ValueKey('batch-done'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 6),
        Icon(failed == 0 ? Icons.check_circle : Icons.info_outline,
            size: 44, color: failed == 0 ? cs.primary : Colors.orange),
        const SizedBox(height: 8),
        Text('解压完成',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 6),
        Text(
          '$ok 成功'
          '${partial > 0 ? ' · $partial 部分' : ''}'
          '${failed > 0 ? ' · $failed 失败' : ''}'
          ' · 用时 ${(ms / 1000).toStringAsFixed(1)} 秒'
          ' · 写出 ${_size(bytes)}',
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
                    subtitle: Text(_batchOutcomeText(j['outcome'] as String? ?? ''),
                        style: const TextStyle(fontSize: 11)),
                    // 失败项点一下在文件管理器里定位（调研 07:135：静默隔离是反面教材）
                    onTap: () => _revealInFolder(j['path'] as String? ?? ''),
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
                child: FilledButton.icon(
                  onPressed: () => _openFolder(firstDest),
                  icon: const Icon(Icons.folder_open, size: 18),
                  label: const Text('打开输出目录'),
                ),
              ),
            if (firstDest != null) const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _resetQueue,
                icon: const Icon(Icons.refresh, size: 18),
                label: const Text('再来一批'),
              ),
            ),
          ],
        ),
      ],
    );
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
          Text(
            _dragging ? '松手就行' : '把伪装包交给我',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            '网盘下载的「.mp4 / .png」，其实可能是压缩包\n直接拖进这个窗口，或者点下面选择',
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
                onPressed: _pick,
                icon: const Icon(Icons.add),
                label: const Text('选择文件'),
                style: FilledButton.styleFrom(
                  padding: const EdgeInsets.symmetric(horizontal: 26, vertical: 18),
                ),
              ),
              const SizedBox(width: 8),
              // 次入口弱化成文字按钮（调研 07:921-923：第一屏只该有一个最强入口）
              TextButton.icon(
                onPressed: _pickFolder,
                icon: const Icon(Icons.folder_open_outlined, size: 16),
                label: const Text('选整个文件夹', style: TextStyle(fontSize: 12)),
              ),
            ],
          ),
          const SizedBox(height: 24),
          Text(
            '认不出的格式也能试 —— 拖进来就行',
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
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
        // 加密方式提示（DESIGN §17.4）：ZipCrypto 有 1/256 误判概率，
        // 是「密码像是对的、文件却全坏」的唯一解释路径。
        if (pv.encryptedCount > 0 && pv.encKind != null)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(
              switch (pv.encKind) {
                'zipcrypto' => '🔓 老式加密（ZipCrypto）：密码可能蒙对，但解出的文件可能是坏的',
                'aes256' => '🔐 AES-256 加密：密码校验可靠',
                'aes128' || 'aes192' => '🔐 AES 加密：密码校验可靠',
                'aes' => '🔐 AES 加密',
                _ => '🔐 加密方式未知',
              },
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
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
    // 扫描阶段没有 _progress：用 SCAN_PROGRESS 的两个数字画进度条（D1）
    final frac = p?.fraction ??
        (_scanTotal > 0 ? (_scanDone / _scanTotal).clamp(0.0, 1.0) : null);
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
                      : '${_size(p.doneBytes)} / ${p.totalBytes == 0 ? "?" : _size(p.totalBytes)}${_etaText(p)}',
                  style: const TextStyle(fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 6),
            if (_pwTry > 0 && _pwTotal > 0)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: Text(
                  '🔑 正在试第 $_pwTry/$_pwTotal 个解压码…',
                  style: TextStyle(fontSize: 12, color: cs.primary),
                ),
              ),
            if (_stage == 'LOCATE' && _scanTotal > 0)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: Text(
                  '🔍 正在扫大文件：${_size(_scanDone)} / ${_size(_scanTotal)}',
                  style: TextStyle(fontSize: 12, color: cs.primary),
                ),
              ),
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
          // 封面分离结果（DESIGN §17.5）：伪装包里的原版视频/图片
          if (r.cover != null) ...[
            const SizedBox(height: 10),
            Text(
              '🎬 视频已分离：${(r.cover!['path'] as String).split('/').last}'
              '（${_size((r.cover!['size'] as num?)?.toInt() ?? 0)}）',
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            TextButton.icon(
              onPressed: () => _openFolder(r.cover!['path'] as String),
              icon: const Icon(Icons.play_circle_outline, size: 16),
              label: const Text('打开封面', style: TextStyle(fontSize: 12)),
            ),
          ],
          if (_entryHint() case final hint?) ...[
            const SizedBox(height: 12),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
              decoration: BoxDecoration(
                color: cs.primaryContainer.withValues(alpha: 0.35),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Text(
                '接下来点这个：$hint',
                style: const TextStyle(fontSize: 13),
              ),
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
                onPressed: () {
                  // 分享文案（调研 07:952-954）：资源圈的真实动作是「把方法告诉对方」
                  final name = (_file ?? '').split('/').last;
                  final pw = (_password == null || _password!.isEmpty)
                      ? ''
                      : '解压码：$_password\n';
                  Clipboard.setData(ClipboardData(
                    text: '「$name」不是视频/图片，是伪装包。\n'
                        '用「杨三海解压」（yshjy）打开它，拖进去就行。\n'
                        '$pw'
                        '（普通播放器打不开是正常的）',
                  ));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('已复制，直接发给对方就行')),
                  );
                },
                icon: const Icon(Icons.copy_all_outlined, size: 18),
                label: const Text('复制解压方法'),
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
    // 分卷中间块本来就没有文件头，探测结论必然是"认不出" —— 说出来只会误导
    final hint = _errorCode == 'MISSING_VOLUME' ? null : _typeHint();
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
            if (hint != null) ...[
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
                    color: hint!.warn ? cs.error : null,
                    height: 1.5,
                  ),
                ),
              ),
            ],
            // 缺分卷是网盘资源最高频的失败原因 —— 直接说缺哪一块、怎么补救
            if (_errorCode == 'MISSING_VOLUME') ...[
              const SizedBox(height: 12),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                decoration: BoxDecoration(
                  color: cs.errorContainer.withValues(alpha: 0.35),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      (_errorDetail?['missing'] as String?) != null
                          ? '缺这一块：${_errorDetail!['missing']}'
                          : '分卷不全',
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: cs.error,
                      ),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      (_errorDetail?['found'] as String?) != null
                          ? '已经找到 ${_errorDetail!['found']} 块。'
                              '把缺的那几块一起下下来（网盘里通常是同一个文件夹），再拖进来。'
                          : '把缺的那几块一起下下来，再拖进来。',
                      style: const TextStyle(fontSize: 12, height: 1.5),
                    ),
                  ],
                ),
              ),
            ],
            // 技术详情（`Error { code: … }`）对新手是噪音，默认收起来：
            // 需要报障时一键复制，而不是糊在脸上
            if ((_errorNote ?? '').isNotEmpty)
              TextButton.icon(
                onPressed: _makeReport,
                icon: const Icon(Icons.bug_report_outlined, size: 15),
                label: const Text('生成诊断包（报障用）', style: TextStyle(fontSize: 12)),
              ),
            if ((_errorNote ?? '').isNotEmpty)
              TextButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: _errorNote!));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('已复制错误详情 —— 报障时贴给我就行')),
                  );
                },
                icon: const Icon(Icons.copy, size: 15),
                label: const Text('复制错误详情', style: TextStyle(fontSize: 12)),
              ),
            if (_needsPassword) ...[
              const SizedBox(height: 18),
              // 密码本里的常用解压码：点一下直接填入并重试（调研 07:874-878）
              if (_recentPw.isNotEmpty) ...[
                Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    '常用解压码（来自密码本，点一下就用）',
                    style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
                  ),
                ),
                const SizedBox(height: 6),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final pw in _recentPw)
                      ActionChip(
                        avatar: const Icon(Icons.history, size: 14),
                        label: Text(pw, style: const TextStyle(fontSize: 12)),
                        visualDensity: VisualDensity.compact,
                        onPressed: () {
                          _pwCtrl.text = pw;
                          _retryWithPassword();
                        },
                      ),
                  ],
                ),
                const SizedBox(height: 10),
              ],
              TextField(
                controller: _pwCtrl,
                obscureText: !_showPw,
                autofocus: true,
                onSubmitted: (_) => _retryWithPassword(),
                textInputAction: TextInputAction.done,
                decoration: InputDecoration(
                  labelText: '解压码',
                  hintText: '一般写在群公告、文件名里，或旁边的 txt',
                  border: const OutlineInputBorder(),
                  isDense: true,
                  prefixIcon: const Icon(Icons.key),
                  // 眼睛：点一下看明文，再点回密文（打错一个字符不用整段重输）
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
              const SizedBox(height: 10),
              FilledButton.icon(
                onPressed: _retryWithPassword,
                icon: const Icon(Icons.lock_open),
                label: const Text('用这个解压码重试'),
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

  /// 批量结果里的 `outcome` 是 Rust 的 Debug 名（`WrongPassword` / `CrcError`），
  /// 直接 toLowerCase 会把英文裸奔给用户 —— 转成错误码再查人话。
  String _batchOutcomeText(String oc) {
    if (oc == 'ok') return '完成';
    if (oc == 'partial') return '部分完成';
    if (oc == 'cancelled') return '已取消';
    final b = StringBuffer();
    for (var i = 0; i < oc.length; i++) {
      final c = oc[i];
      if (i > 0 && c.toUpperCase() == c && c.toLowerCase() != c) b.write('_');
      b.write(c.toUpperCase());
    }
    return _errText(b.toString());
  }

  /// 在文件管理器里定位某个文件（失败项用）
  Future<void> _revealInFolder(String path) async {
    if (path.isEmpty) return;
    try {
      await Process.run('xdg-open', [File(path).parent.path]);
    } catch (_) {}
  }

  /// 读密码本，给密码框做「常用解压码」chips（只读，不回写）
  ///
  /// 路径与引擎一致：`$XDG_CONFIG_HOME/yshjy/passwords.txt`（回退 `~/.config`）。
  Future<void> _loadRecentPasswords() async {
    try {
      final env = Platform.environment;
      final base = env['XDG_CONFIG_HOME'] ?? '${env['HOME']}/.config';
      final f = File('$base/yshjy/passwords.txt');
      if (!await f.exists()) return;
      final lines = await f.readAsLines();
      final pws = <String>[];
      for (final l in lines) {
        final t = l.trim();
        if (t.isEmpty || t.startsWith('#')) continue;
        if (!pws.contains(t)) pws.add(t);
        if (pws.length >= 6) break;
      }
      if (mounted && pws.isNotEmpty) setState(() => _recentPw = pws);
    } catch (_) {
      // 密码本读不出来不影响主流程
    }
  }

  /// 用两次 progress 的时间差算速度（0.7/0.3 平滑，避免数字乱跳）
  void _tickSpeed(int done) {
    final now = DateTime.now();
    final last = _lastTick;
    if (last == null || done < _lastBytes) {
      _lastTick = now;
      _lastBytes = done;
      return;
    }
    final dt = now.difference(last).inMilliseconds / 1000.0;
    if (dt < 0.4) return; // 太密会抖
    final inst = (done - _lastBytes) / dt;
    _speed = _speed <= 0 ? inst : _speed * 0.7 + inst * 0.3;
    _lastTick = now;
    _lastBytes = done;
  }

  /// 「 · 12.3 MB/s · 剩 41 秒」；速度未知就返回空串
  String _etaText(Progress p) {
    if (_speed <= 0 || p.totalBytes <= 0) return '';
    final left = p.totalBytes - p.doneBytes;
    if (left <= 0) return '';
    final sec = (left / _speed).round();
    final eta = sec >= 60 ? '${sec ~/ 60} 分 ${sec % 60} 秒' : '$sec 秒';
    return ' · ${_size(_speed.round())}/s · 剩 $eta';
  }

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
