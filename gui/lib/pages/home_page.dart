import 'dart:async';
import 'dart:io';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app_settings.dart';
import '../engine.dart';
import '../entry_tree.dart';
import '../history_store.dart';
import '../humanize.dart';
import '../notify.dart';
import '../platform_paths.dart';

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

  /// pending / running / ok / partial / failed / cancelled
  String status = 'pending';

  /// 引擎给这一行的任务号（`JOB_STARTED` 里带回来）；还没开始跑就是 null
  ///
  /// D5 用：单独取消某一行时要把这个号发给引擎，引擎才能只跳这一条。
  int? jobNo;

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

  /// 测试用：直接灌一份预览结果并进预览页（生产代码永远传 null）。
  ///
  /// 为什么留这个口子：预览树的交互（勾父 → 子全选、取消一子 → 父半选、面包屑下钻）
  /// 是**状态机**，用绝对指针真机手点验不了（落点差几像素就点到别的行，实测过一次
  /// 把「已选 1001」点成「已选 1」，看不出是功能错还是手滑）。留这个口子才能把它
  /// 变成确定性的 widget 测试 —— 和 `autoStart` 同一个理由：只影响测试，不影响行为。
  final Preview? debugPreview;

  const HomePage({
    super.key,
    this.initialFile,
    this.onStatus,
    this.autoStart = true,
    this.debugPreview,
  });

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

  /// 用户在界面上单独取消掉的任务号（D5）——
  /// 批量结果回来时要保住「已取消」这个状态，别被「失败」覆盖掉
  final Set<int> _cancelled = {};

  /// 这一轮扫的是几个文件夹（D6：拖文件夹进来要明说「会自动展开」）
  int _foldersScanned = 0;
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
  /// 解压产物里还套着的压缩包（嵌套包下钻：先问，再解一层）
  List<String> _nestedArchives = [];
  final List<String> _notes = [];

  /// 解压页那两个勾选（GUI-PLAN §5.2①）：封面和内容各管各的
  bool _wantCover = true;
  bool _wantExtract = true;

  /// 文件级挑选（P23-1）：勾中的条目序号；等于全部时不传给引擎（保持老行为）
  final Set<int> _picked = {};
  /// 单任务输出目录（P23-2）：null = 解到包所在目录（引擎默认，用户验收定的规则）
  String? _outDir;
  /// 文件名编码（P23-3）：auto = 引擎自动链（UTF-8 → 0x7075 → GB18030）
  String _encoding = 'auto';
  /// 文件列表的滚动条控制器（用户报过「滚轮没反应」——至少要有可见滚动条能拖）
  final ScrollController _listCtrl = ScrollController();

  /// 预览列表的视图：树（默认，第三期 A）/ 平铺（老样子，留着给「只想全选」的习惯）
  bool _treeView = true;

  /// 树里展开了哪些目录（键 = 归档内路径）。打开新包时默认**全展开**——
  /// 用户要的是「一眼看到里面有什么」，树只是把层级画出来；想收就点箭头。
  final Set<String> _expanded = {};

  /// 面包屑当前停在哪一层（`''` = 根）。非空时列表只显示该目录的下一级
  /// （PeaZip 的面包屑是真过滤，不是滚动定位，调研 07:616-631）。
  String _crumb = '';

  /// 当前包的树（缓存，别每帧重建：1000 条目重建一次要几十毫秒）
  List<EntryNode> _tree = const [];
  /// 引擎的 ASK_USER 已经在弹窗里等答案（防止重复弹）
  bool _askOpen = false;

  @override
  void initState() {
    super.initState();
    // 引擎没启动也别让界面崩：错误在点"选择文件"时才提示
    if (widget.autoStart) _client.start().catchError((_) {});
    _sub = _client.events.listen(_onEvent);
    _loadRecentPasswords();
    // 测试注入口：直接进预览页（不连引擎）。初始化逻辑与 _open() 里那段保持一致，
    // 两处要一起改 —— 否则测试验的是另一套初始状态。
    final pv0 = widget.debugPreview;
    if (pv0 != null) {
      _file = '/tmp/测试包.zip';
      _preview = pv0;
      _phase = Phase.preview;
      _wantCover = pv0.hasCover;
      _picked
        ..clear()
        ..addAll(pv0.entries.where((e) => !e.isDir).map((e) => e.index));
      _tree = buildEntryTree(pv0.entries);
      _expanded
        ..clear()
        ..addAll(allDirPaths(_tree));
      _crumb = '';
    }
    // 从右键菜单带文件启动：等首帧画完再开，避免 setState 早于 build
    final f = widget.initialFile;
    if (f != null && f.isNotEmpty && pv0 == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _open(f));
    }
  }

  @override
  void dispose() {
    _pwCtrl.dispose();
    _listCtrl.dispose();
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
              _queue[idx].jobNo = jobNo; // D5：记住任务号，才能单独取消这一行
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
      case 'RESUMED':
        // 断点续传：上次没解完，这次接着来。
        // 必须给用户一句话，否则「一上来就有一大截进度、前面那堆文件没动」会被当成 bug。
        final done = (e.data['done'] as num?)?.toInt() ?? 0;
        final text = '接着上次没解完的来（上次已完成 $done 项）';
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
      case 'ASK_USER':
        // 限额命中（压缩炸弹 / 解压体积超限）：引擎在等工作线程里等我们回答，
        // 60 秒不答它就自己按「跳过」往下走（USER，2026-10-17 定的交互）。
        _askUser(e);
      // 不认识的 kind 一律忽略 —— 后端加新事件不会弄坏旧界面
      default:
        break;
    }
  }

  /// 引擎反问：限额命中要不要继续。三选一 → answer: continue / skip / cancel
  Future<void> _askUser(EngineEvent e) async {
    if (_askOpen) return;
    _askOpen = true;
    final reason = e.data['reason']?.toString() ?? '';
    final written = (e.data['written'] as num?)?.toInt() ?? 0;
    final limit = (e.data['limit'] as num?)?.toInt() ?? 0;
    final why = switch (reason) {
      'possible_zip_bomb' => '这个包解开后可能非常大（压缩比异常，像压缩炸弹）',
      'extract_size_limit_exceeded' => '解压出来的文件超过安全限额',
      _ => '这次解压超过了安全限额',
    };
    final choice = await showDialog<String>(
      context: context,
      barrierDismissible: false, // 误点外面 = 不回答，等于让用户白等 60 秒
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.warning_amber_rounded, size: 32),
        title: const Text('要继续解压吗？'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(why, style: const TextStyle(fontSize: 13, height: 1.5)),
            const SizedBox(height: 10),
            Text(
              '已经写出 ${_size(written)}'
              '${limit > 0 ? '，限额 ${_size(limit)}' : ''}',
              style: const TextStyle(fontSize: 12),
            ),
            const SizedBox(height: 8),
            Text(
              '60 秒不选会按「跳过」处理。',
              style: TextStyle(
                fontSize: 12,
                color: Theme.of(ctx).colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, 'cancel'),
            child: const Text('取消整个任务'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, 'skip'),
            child: const Text('跳过这一个'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, 'continue'),
            child: const Text('继续解压'),
          ),
        ],
      ),
    );
    _askOpen = false;
    // 关窗/超时都按「跳过」，与引擎的 60 秒默认一致
    final value = choice ?? 'skip';
    try {
      await _client.answer(value);
    } catch (_) {
      // 迟到答案会被引擎丢掉；这里不该因为「引擎已经在跑了」而炸界面
    }
  }

  /// 诊断码 → 人话。表在 `lib/humanize.dart`（D21：查不到也不露原始码）
  String _diagText(String code) => diagText(code);

  /// 错误码 → 人话（**"引擎只给码、壳查文案"的落地**，表在 `lib/humanize.dart`）
  String _errText(String code) => errorText(code);

  // ───────────────────────── 动作 ─────────────────────────

  Future<void> _pick() async {
    // 不设扩展名白名单：伪装包的外壳可能是 .pdf / .docx / .mp3 / .exe 之类，
    // 名单一写死用户在对话框里就看不到（拖拽却能用，行为不一致）。
    const group = XTypeGroup(label: '压缩包或伪装文件');
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
    // D6：传进来的是文件夹 → 队列顶部要明说「会自动展开里面的压缩包」
    // 注意：Dart 和 bash 一样**不认中文变量名**（这个坑踩两次了）
    final folderCount = paths.where((p) => Directory(p).existsSync()).length;
    setState(() {
      _phase = Phase.listing;
      _foldersScanned = folderCount;
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

  /// 解压完扫一眼产物：里面若还套着压缩包，在完成卡上问一句「继续解压」
  ///
  /// 不自动下钻 —— 大包解出来的 7z 再解一层可能又跑十几分钟，
  /// 让用户点一下更稳妥（DESIGN §17.6 的自动下钻留到下一批）。
  Future<void> _probeNested() async {
    final dest = _result?.dest;
    if (dest == null) return;
    try {
      final r = await _client.scan([dest]);
      if (!mounted) return;
      final items = (r['items'] as List?) ?? const [];
      final nested = <String>[];
      for (final raw in items) {
        final i = (raw as Map).cast<String, dynamic>();
        if (i['archive'] == true) {
          final p = i['path'] as String?;
          if (p != null) nested.add(p);
        }
      }
      setState(() => _nestedArchives = nested.take(20).toList());
    } catch (_) {
      // 探测失败就当没有，绝不影响解压结果
      if (mounted) setState(() => _nestedArchives = []);
    }
  }

  /// 只取消这一行（D5）：其余任务照跑
  ///
  /// 两种情形都走同一条协议：
  /// - 还在排队 → 直接本地标「已取消」，同时告诉引擎跳过它
  /// - 正在跑 → 发给引擎，引擎判断「正在跑的就是它」才真的打断
  Future<void> _cancelRow(int i) async {
    if (i < 0 || i >= _queue.length) return;
    final q = _queue[i];
    if (!q.archive) return;
    if (q.status == 'ok' || q.status == 'cancelled') return;
    // 引擎任务号：跑过的用它记下的；还在排队的就是它在批次里的位置
    final jobNo = q.jobNo ?? (_batchOrder.indexOf(q.path) + 1);
    setState(() {
      q.status = 'cancelled';
      if (jobNo >= 1) _cancelled.add(jobNo);
    });
    if (jobNo >= 1 && _batch != null) {
      await _client.cancelJob(jobNo);
      if (mounted) widget.onStatus?.call('已取消这一项，其余继续');
    } else if (mounted) {
      widget.onStatus?.call('已跳过这一项（还没开始跑）');
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
      _cancelled.clear();
      for (final q in ready) {
        q.status = 'pending';
        q.jobNo = null;
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
          final no = (j['id'] as num?)?.toInt() ?? _queue[idx].jobNo;
          // D5：用户单独取消过（或引擎回 cancelled）的行显示「已取消」，不是「失败」
          if (oc == 'cancelled' || (no != null && _cancelled.contains(no))) {
            _queue[idx].status = 'cancelled';
          } else {
            _queue[idx].status =
                oc == 'ok' ? 'ok' : (oc == 'partial' ? 'partial' : 'failed');
          }
        }
        _batch = r;
        _phase = Phase.done;
      });
      final ok = (r['ok'] as num?)?.toInt() ?? 0;
      final failed = (r['failed'] as num?)?.toInt() ?? 0;
      widget.onStatus?.call('批量完成：$ok 成功 / $failed 失败');
      // 设置里开了「解完自动打开」才开，且只开第一个有产物的目录（不逐个弹窗口）
      final firstDest = jobs
          .cast<Map>()
          .map((j) => j['dest'] as String?)
          .firstWhere((d) => d != null && d.isNotEmpty, orElse: () => null);
      await _maybeOpenAfterDone(firstDest);
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

  /// 解压完成后是否自动打开输出目录（设置页那个开关，存 `[general] open_after`，默认关）
  ///
  /// 为什么默认关：批量解 20 个包不该弹 20 个窗口 —— 业界也是这么分的
  /// （NanaZip 做成选项、7-Zip 干脆不做）。界面本来就有「打开输出目录」按钮，
  /// 主动点一下就够；想让「解完自动跳到文件夹」的人在设置里打开。
  Future<void> _maybeOpenAfterDone(String? dest) async {
    if (dest == null || dest.isEmpty) return;
    if (!await openAfterExtractEnabled()) return;
    if (!mounted) return;
    await openDirectory(dest);
  }

  void _resetQueue() {
    setState(() {
      _queue.clear();
      _batch = null;
      _phase = Phase.empty;
      _file = null;
      _foldersScanned = 0;
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
        // 有封面区才默认勾「伪装封面」；普通 zip 不显示那一行，也不提取
        _wantCover = pv.hasCover;
        _wantExtract = true;
        // 新包默认全选（取消勾选才是「只解一部分」）；输出目录/编码回默认
        _picked
          ..clear()
          ..addAll(pv.entries.where((e) => !e.isDir).map((e) => e.index));
        _outDir = null;
        _encoding = 'auto';
        // 第三期 A（预览树）：建一次树、默认全展开、面包屑回到根
        _tree = buildEntryTree(pv.entries);
        _expanded
          ..clear()
          ..addAll(allDirPaths(_tree));
        _crumb = '';
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
      _nestedArchives = [];
    });
    try {
      widget.onStatus?.call('正在解压…');
      final pv = _preview;
      // 全勾 = 不传 select（引擎默认全解，与以前行为完全一致）
      final pickedAll = pv == null ||
          _picked.length == pv.entries.where((e) => !e.isDir).length;
      final r = await _client.extract(
        path,
        password: _password,
        cover: _wantCover,
        wantExtract: _wantExtract,
        dest: _outDir,
        select: (_wantExtract && !pickedAll) ? _picked.toList() : null,
        encoding: _encoding,
      );
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
      widget.onStatus?.call(r.ok ? '完成：${r.entriesDone} 个文件' : '部分完成');
      // 解压完发桌面通知：切到别的窗口也知道；发不出不影响主流程
      Notifier.send(
        r.ok ? '解压完成' : '解压部分完成',
        '${r.entriesDone} 个文件 · ${_size(r.bytesWritten)}',
      );
      await _maybeOpenAfterDone(r.dest);
      // 产物里若还套着压缩包，问一句要不要再解一层（不自动，免得大包解到天荒地老）
      _probeNested();
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

  /// 这些失败还能抢救：索引（中央目录）坏了、数据只坏了一部分。
  ///
  /// 密码错、缺分卷、目标无权限这类救不了 —— 给按钮只会让用户白等一场。
  /// `NOT_ARCHIVE` 要收窄：认不出压缩包时，只有「内容其实是 ZIP、只是索引坏了」
  /// 才值得试（否则用户拖进来的就是个视频，救援也只会再说一遍认不出）。
  bool get _canRescue {
    final code = _errorCode;
    if (code == null || _needsPassword || code == 'MISSING_VOLUME') return false;
    if (code == 'NOT_ARCHIVE') {
      return (_errorDetail?['looks_like'] as String?)?.toLowerCase() == 'zip';
    }
    return const {'DATA_LOST', 'PARTIAL_DATA', 'CHECKSUM_FAILED'}.contains(code);
  }

  /// 救援模式：索引坏了也能按文件头把数据捞出来（协议 `rescue`）
  ///
  /// 慢路径（要扫全文件），所以界面切到进度态再走，别让按钮点下去像没反应。
  Future<void> _rescue() async {
    final path = _file;
    if (path == null) return;
    setState(() {
      _phase = Phase.extracting;
      _progress = null;
      _current = null;
      _nestedArchives = [];
    });
    try {
      widget.onStatus?.call('救援模式：按文件头重建清单…');
      final r = await _client.rescue(
        path,
        dest: _outDir,
        encoding: _encoding,
      );
      if (!mounted) return;
      setState(() {
        _result = ExtractResult.fromJson(r);
        _phase = Phase.done;
      });
      final savedCount = (r['files'] as List?)?.length ?? 0;
      widget.onStatus?.call('救援完成：救回 $savedCount 个文件');
      Notifier.send('救援完成', '救回 $savedCount 个文件');
      await _maybeOpenAfterDone(r['dest'] as String?);
      await HistoryStore.add(HistoryEntry(
        time: DateTime.now().toIso8601String(),
        input: path,
        dest: r['dest'] as String?,
        ok: true,
        bytes: (r['bytes_written'] as num?)?.toInt() ?? 0,
      ));
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  /// 紧凑小按钮：默认 TextButton 高 48，一行塞四个会把上面的列表挤没（窄窗口致命）
  Widget _miniBtn(String text, VoidCallback onTap, {IconData? icon}) {
    const st = ButtonStyle(
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      visualDensity: VisualDensity.compact,
      minimumSize: WidgetStatePropertyAll(Size(0, 28)),
      padding: WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 8)),
      textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 12)),
    );
    final label = Text(text, style: const TextStyle(fontSize: 12));
    return icon == null
        ? TextButton(style: st, onPressed: onTap, child: label)
        : TextButton.icon(
            style: st,
            onPressed: onTap,
            icon: Icon(icon, size: 15),
            label: label,
          );
  }

  /// 高级选项弹窗（P23-2 输出目录 / P23-3 文件名编码）
  ///
  /// 做成弹窗而不是页面上的一块：页面上放不下时会挤掉列表高度（窄窗口下列表为 0 高、
  /// 滚轮没反应）。弹窗不占主界面布局。
  Future<void> _showAdvanced() async {
    await showDialog<void>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDlg) => AlertDialog(
          title: const Text('高级选项', style: TextStyle(fontSize: 15)),
          contentPadding: const EdgeInsets.fromLTRB(16, 4, 16, 4),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.drive_folder_upload_outlined, size: 18),
                title: const Text('输出到指定目录', style: TextStyle(fontSize: 13)),
                subtitle: Text(
                  _outDir ?? '默认：解到包所在目录旁边（推荐）',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 11),
                ),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (_outDir != null)
                      IconButton(
                        tooltip: '改回默认（包旁边）',
                        icon: const Icon(Icons.restart_alt, size: 18),
                        onPressed: () => setDlg(() => _outDir = null),
                      ),
                    TextButton(
                      onPressed: () async {
                        final d = await getDirectoryPath(confirmButtonText: '就解到这里');
                        if (d != null) setDlg(() => _outDir = d);
                      },
                      child: const Text('选择', style: TextStyle(fontSize: 12)),
                    ),
                  ],
                ),
              ),
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.translate, size: 18),
                title: const Text('文件名编码', style: TextStyle(fontSize: 13)),
                subtitle: const Text('老包在中文/日文 Windows 上打的，名字变乱码时手动指定',
                    style: TextStyle(fontSize: 11)),
                trailing: DropdownButton<String>(
                  value: _encoding,
                  isDense: true,
                  style: const TextStyle(fontSize: 12),
                  items: const [
                    DropdownMenuItem(value: 'auto', child: Text('自动')),
                    DropdownMenuItem(value: 'gbk', child: Text('GBK 中文')),
                    DropdownMenuItem(value: 'big5', child: Text('Big5 繁体')),
                    DropdownMenuItem(value: 'sjis', child: Text('Shift-JIS 日文')),
                    DropdownMenuItem(value: 'utf-8', child: Text('UTF-8')),
                  ],
                  onChanged: (v) => setDlg(() => _encoding = v ?? 'auto'),
                ),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: const Text('知道了'),
            ),
          ],
        ),
      ),
    );
    // 弹窗里改的值要反映到主界面那一行摘要上
    if (mounted) setState(() {});
  }

  void _showError(String code, String note, [EngineError? e]) {
    if (!mounted) return;
    Notifier.send('解压没成功', note.isEmpty ? code : note);
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
      final pv = _preview;
      final pickedAll = pv == null ||
          _picked.length == pv.entries.where((e) => !e.isDir).length;
      final r = await _client.extract(
        _file!,
        password: pw,
        dest: _outDir,
        select: pickedAll ? null : _picked.toList(),
        encoding: _encoding,
      );
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
      await _maybeOpenAfterDone(r.dest);
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
    final ok = await openDirectory(dir);
    if (!ok) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('打不开文件管理器：$dir')));
    }
  }

  Future<void> _cancel() async {
    await _client.cancel();
  }

  /// 批量里只跳过当前这一项（引擎的 skip 命令；整批继续跑）
  Future<void> _skipCurrent() async {
    try {
      await _client.skipCurrent();
    } catch (_) {
      // 引擎刚好已经切到下一项时 skip 会返回 skipped:false —— 不算错误
    }
    if (mounted) widget.onStatus?.call('已跳过这一项');
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
        // D6：拖/选的是文件夹 → 明说会怎么处理，别让人猜
        if (_foldersScanned > 0)
          Padding(
            padding: const EdgeInsets.only(top: 4, bottom: 4),
            child: Text(
              '这是个文件夹，会自己翻开找里面的压缩包和分卷（只读文件头，不动你的文件）；'
              '不是压缩包的文件会列出来但跳过',
              style: TextStyle(fontSize: 11, height: 1.5, color: cs.primary),
            ),
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
                return Focus(
                  autofocus: i == 0,
                  // Del / Backspace 移出队列（D4：以前只能整队清空）
                  onKeyEvent: (node, event) {
                    if (event is KeyDownEvent &&
                        (event.logicalKey == LogicalKeyboardKey.delete ||
                            event.logicalKey == LogicalKeyboardKey.backspace)) {
                      setState(() => _queue.removeAt(i));
                      return KeyEventResult.handled;
                    }
                    return KeyEventResult.ignored;
                  },
                  child: ListTile(
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
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      // D5：只取消这一行（其余任务照跑，和「停掉整批」不是一回事）
                      if (q.archive &&
                          (q.status == 'pending' || q.status == 'running'))
                        IconButton(
                          icon: const Icon(Icons.block, size: 16),
                          tooltip: q.status == 'running'
                              ? '取消这一项（其余继续）'
                              : '这一项不跑',
                          visualDensity: VisualDensity.compact,
                          onPressed: () => _cancelRow(i),
                        ),
                      _queueStatusChip(q),
                      IconButton(
                        icon: const Icon(Icons.close, size: 16),
                        tooltip: '移出队列（或按 Del）',
                        visualDensity: VisualDensity.compact,
                        onPressed: () => setState(() => _queue.removeAt(i)),
                      ),
                    ],
                  ),
                ),
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
      'cancelled' => ('已取消', cs.onSurfaceVariant),
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
              'cancelled' => Icons.block,
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
              // ⚠️ 别改成 FilledButton —— widget_test 里有条测试专门钉着这件事
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
    // 整页接住滚轮：用户在窗口任何位置滚，都滚文件列表。
    // 只在 ListView 里滚的话，鼠标停在头部/卡片上就会「滚了没反应」（用户实测报过）。
    return Listener(
      onPointerSignal: (e) {
        if (e is PointerScrollEvent && _listCtrl.hasClients) {
          final pos = _listCtrl.position;
          final next = (_listCtrl.offset + e.scrollDelta.dy)
              .clamp(pos.minScrollExtent, pos.maxScrollExtent);
          if (next != _listCtrl.offset) _listCtrl.jumpTo(next);
        }
      },
      child: Column(
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
        // 智能解压的结果预告（调研 07:864-865）：点「开始解压」之前就知道会多出什么
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Row(
            children: [
              Icon(Icons.auto_awesome, size: 15, color: cs.primary),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  _smartDirHint(pv),
                  style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
                ),
              ),
            ],
          ),
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
        // 封面与内容上下并列、各自带勾选（GUI-PLAN §5.2①，用户验收原话）
        Card(
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: Column(
            children: [
              if (pv.hasCover)
                CheckboxListTile(
                  key: const ValueKey('want-cover'),
                  dense: true,
                  value: _wantCover,
                  onChanged: (v) => setState(() => _wantCover = v ?? false),
                  secondary: const Icon(Icons.movie_outlined, size: 20),
                  title: Text(
                    '伪装封面（${_size((pv.cover!['size'] as num?)?.toInt() ?? 0)}'
                    '${pv.cover!['ext'] != null ? ' · ${pv.cover!['ext']}' : ''}）',
                    style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
                  ),
                  subtitle: const Text(
                    '勾上就把里面的原视频/图片切出来',
                    style: TextStyle(fontSize: 11),
                  ),
                ),
              CheckboxListTile(
                key: const ValueKey('want-extract'),
                dense: true,
                value: _wantExtract,
                onChanged: (v) => setState(() => _wantExtract = v ?? false),
                secondary: const Icon(Icons.folder_zip_outlined, size: 20),
                title: Text(
                  '压缩包内容（${pv.fileCount} 个文件 · ${_size(pv.totalSize)}）',
                  style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
                ),
                subtitle: const Text(
                  '勾上就解压里面的文件',
                  style: TextStyle(fontSize: 11),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        // 文件级挑选（P23-1）：统计 + 全选 + 高级入口压成**一行**。
        // 别堆成两三块：挤在列表上方会吃掉列表高度，窄窗口下列表被压成 0 高，
        // 滚轮看起来就「完全没反应」（用户实测报过）。
        //
        // 第三期 A：这一行同时当**状态栏**用（PeaZip 的 `3 dir(s), 6 file(s), 1.1 GB`）。
        // 「已选」按**文件条数**算、不按目录条数算，合计只算勾上的文件（方案 §A 决策 4）。
        Row(
          children: [
            Text(
              '已选 ${_picked.length} / ${pv.fileCount} 个文件 · ${_size(_pickedBytes(pv))}',
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            _miniBtn('全选', () => setState(() {
                  _picked
                    ..clear()
                    ..addAll(pv.entries.where((e) => !e.isDir).map((e) => e.index));
                })),
            _miniBtn('全不选', () => setState(() => _picked.clear())),
            const Spacer(),
            // 树 / 平铺切换：树是默认（看层级 + 按目录勾），平铺留给「只想全选」的老习惯
            _miniBtn(
              _treeView ? '平铺' : '树形',
              () => setState(() => _treeView = !_treeView),
              icon: _treeView ? Icons.view_list_outlined : Icons.account_tree_outlined,
            ),
            _miniBtn(
              '高级：${_outDir == null ? '包旁边' : '自定义'} · '
              '${_encoding == 'auto' ? '自动编码' : _encoding.toUpperCase()}',
              _showAdvanced,
              icon: Icons.tune,
            ),
          ],
        ),
        // 面包屑只在树视图里出现（平铺没有层级，给面包屑是骗人的）
        if (_treeView && _tree.isNotEmpty) _crumbBar(),
        Expanded(child: _fileList(pv)),
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
              onPressed: (_wantExtract && _picked.isEmpty) ||
                      (!_wantExtract && !(pv.hasCover && _wantCover))
                  ? null
                  : _extract,
              icon: Icon(_wantExtract ? Icons.download : Icons.movie_outlined),
              label: Text(
                _wantExtract
                    ? ((pv.hasCover && _wantCover)
                        ? '开始解压 ${_picked.length} 个 + 提取封面'
                        : '开始解压（${_picked.length} 个文件）')
                    : (pv.hasCover && _wantCover ? '只提取封面' : '请勾选要提取的内容'),
              ),
              style: FilledButton.styleFrom(
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
              ),
            ),
          ],
        ),
      ],
      ),
    );
  }

  // ─────────────── 第三期 A：预览树 / 面包屑 / 三态勾选 ───────────────
  //
  // 树的构建与勾选逻辑全在 `entry_tree.dart`（纯 Dart，可单测）；
  // 这里只管「怎么画」。别把聚合算法写进 build 里 —— 那玩意儿没法测。

  /// 勾上的文件合计多少字节（状态栏用；目录不算，它自己不产出文件）
  int _pickedBytes(Preview pv) {
    var n = 0;
    for (final e in pv.entries) {
      if (!e.isDir && _picked.contains(e.index)) n += e.size;
    }
    return n;
  }

  /// 面包屑：**真过滤**（点哪一级就只看那一层的下一级），最后一节高亮。
  /// 第一节显示包名（比「全部」更好认），点它回根。
  /// 依据：PeaZip 的面包屑是被夸的点，7-Zip 的下拉框是被吐槽的点（调研 07:616-631 / :682）。
  Widget _crumbBar() {
    final cs = Theme.of(context).colorScheme;
    final crumbs = breadcrumbs(_crumb);
    final pkg = (_file ?? '').split('/').last;
    return Padding(
      padding: const EdgeInsets.only(top: 4, bottom: 2),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (var i = 0; i < crumbs.length; i++) ...[
              if (i > 0)
                Icon(Icons.chevron_right, size: 16, color: cs.onSurfaceVariant),
              InkWell(
                onTap: () => setState(() => _crumb = crumbs[i].path),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                  child: Text(
                    i == 0 ? (pkg.isEmpty ? crumbs[i].name : pkg) : crumbs[i].name,
                    style: TextStyle(
                      fontSize: 12,
                      color: i == crumbs.length - 1 ? cs.onSurface : cs.primary,
                      fontWeight: i == crumbs.length - 1
                          ? FontWeight.w600
                          : FontWeight.normal,
                    ),
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  /// 文件清单卡片（树 / 平铺共用同一块卡片与滚动条，滚动位置不丢）
  Widget _fileList(Preview pv) {
    final List<EntryNode> rows;
    if (_treeView && _tree.isNotEmpty) {
      rows = flattenTree(_tree, expanded: _expanded, crumb: _crumb);
    } else {
      // 平铺：把条目包成「没有子项的单层节点」，勾选/图标/大小走同一套渲染，
      // 免得两套渲染以后各自长歪（状态栏的「已选」也必须两边一致）
      rows = [
        for (final e in pv.entries)
          EntryNode(
            name: e.name,
            path: e.path,
            level: 0,
            isDir: e.isDir,
            index: e.index,
            size: e.size,
            packed: e.packed,
            fileCount: e.isDir ? 0 : 1,
            encryptedCount: e.encrypted ? 1 : 0,
            modified: e.modified,
            risky: e.risky,
            encrypted: e.encrypted,
            children: const [],
          ),
      ];
    }
    // 缩进基准：面包屑停在哪一层，那一层的子项就不缩进
    final base = _treeView ? depthOfPath(_crumb) + 1 : 0;
    // 窄窗口不挤「压缩后」那一列（76+72+图标 ≈ 200px，窗口窄了文件名就没地方了）
    final wide = MediaQuery.of(context).size.width >= 720;
    // 列标题：PeaZip 有表头，两个数字光秃秃并排会看不懂哪个是「压缩后」
    final hdr = TextStyle(fontSize: 11, color: Theme.of(context).colorScheme.onSurfaceVariant);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.only(right: 14, bottom: 2),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              SizedBox(
                width: 76,
                child: Text('大小', textAlign: TextAlign.right, style: hdr),
              ),
              if (wide)
                SizedBox(
                  width: 72,
                  child: Text('压缩后', textAlign: TextAlign.right, style: hdr),
                ),
            ],
          ),
        ),
        Expanded(
          child: Card(
            clipBehavior: Clip.antiAlias,
            // 常显滚动条：列表比视口长时必须看得出来能滚（用户报过「滚轮没反应」，
            // 至少要有能拖的条；Flutter 在 Linux 桌面默认不画滚动条）
            child: Scrollbar(
              controller: _listCtrl,
              thumbVisibility: true,
              child: ListView.separated(
                controller: _listCtrl,
                itemCount: rows.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (context, i) {
                  final n = rows[i];
                  final indent = (_treeView && n.level > base) ? n.level - base : 0;
                  return _entryRow(n, indent: indent, tree: _treeView, wide: wide);
                },
              ),
            ),
          ),
        ),
      ],
    );
  }

  /// 一行：三态复选框 + 图标 + 名字（+目录的文件数）+ 加密/风险 + 大小 + 压缩后
  Widget _entryRow(
    EntryNode n, {
    required int indent,
    required bool tree,
    required bool wide,
  }) {
    final cs = Theme.of(context).colorScheme;
    final st = pickStateOf(n, _picked);
    final hasFiles = n.fileCount > 0; // 空目录没有可勾的东西
    final expanded = _expanded.contains(n.path);
    final sizeText = (!n.isDir || hasFiles) ? _size(n.size) : '';
    return ListTile(
      dense: true,
      contentPadding: EdgeInsets.only(left: 6.0 + indent * 18.0, right: 12),
      // 目录：点一行 = 展开/收起；文件：点一行 = 勾上/取消
      onTap: n.isDir
          ? (tree
              ? () => setState(() {
                    if (expanded) {
                      _expanded.remove(n.path);
                    } else {
                      _expanded.add(n.path);
                    }
                  })
              : null)
          : () => setState(() => applyPick(n, _picked, st != PickState.all)),
      leading: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          SizedBox(
            width: 30,
            child: hasFiles
                ? Checkbox(
                    tristate: n.isDir,
                    value: n.isDir
                        ? (st == PickState.all
                            ? true
                            : (st == PickState.some ? null : false))
                        : (st == PickState.all),
                    // 点一下：没全选就整棵选中，已经全选就整棵取消
                    // （目录半选也走这里 —— 半选时点一下的直觉是「全选」）
                    onChanged: (_) =>
                        setState(() => applyPick(n, _picked, st != PickState.all)),
                  )
                : null,
          ),
          Icon(
            n.isDir
                ? (tree && expanded ? Icons.folder_open : Icons.folder_outlined)
                : Icons.insert_drive_file_outlined,
            size: 20,
            color: n.isDir ? cs.primary : cs.onSurfaceVariant,
          ),
        ],
      ),
      title: Text(
        tree ? n.name : n.path,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 13,
          fontWeight: n.isDir ? FontWeight.w500 : FontWeight.normal,
        ),
      ),
      // 目录带一行「N 个文件」；窄窗口省掉（把高度让给列表）
      subtitle: (tree && n.isDir && wide)
          ? Text(
              n.isEmptyDir ? '空目录（没有东西可解）' : '${n.fileCount} 个文件',
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
            )
          : null,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          // 目录行的「只看这一层」入口：点它 = 面包屑下钻到该目录
          // （点目录名是展开/收起；要「进去」得有个明确入口，否则面包屑只能回不能进）
          if (tree && n.isDir && !n.isEmptyDir)
            SizedBox(
              width: 30,
              child: IconButton(
                padding: EdgeInsets.zero,
                visualDensity: VisualDensity.compact,
                tooltip: '只看这一层',
                icon: const Icon(Icons.subdirectory_arrow_right, size: 16),
                onPressed: () => setState(() => _crumb = n.path),
              ),
            ),
          if (n.encrypted) const Icon(Icons.lock, size: 15),
          if (n.risky)
            const Padding(
              padding: EdgeInsets.only(left: 4),
              child: Icon(Icons.warning_amber_rounded, size: 16),
            ),
          const SizedBox(width: 8),
          SizedBox(
            width: 76,
            child: Text(
              sizeText,
              textAlign: TextAlign.right,
              style: const TextStyle(fontSize: 12),
            ),
          ),
          // 压缩后大小（PeaZip 默认列，调研 07:632）：一眼看出哪个文件「压不动」
          if (wide)
            SizedBox(
              width: 72,
              child: Text(
                (!n.isDir && n.packed != null) ? _size(n.packed!) : '',
                textAlign: TextAlign.right,
                style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
              ),
            ),
        ],
      ),
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
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                // 批量里跳过当前这一个（组 B 的 skip 语义：不是整批取消）
                if (_batchOrder.length > 1) ...[
                  OutlinedButton.icon(
                    onPressed: _skipCurrent,
                    icon: const Icon(Icons.skip_next_outlined),
                    label: const Text('跳过这项'),
                  ),
                  const SizedBox(width: 8),
                ],
                OutlinedButton.icon(
                  onPressed: _cancel,
                  icon: const Icon(Icons.stop_circle_outlined),
                  label: Text(_batchOrder.length > 1 ? '取消整批' : '取消'),
                ),
              ],
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
    // 只勾了「伪装封面」那一项：解出 0 个文件是正常的，别吓用户
    final onlyCover = r.entriesDone == 0 && r.cover != null;
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
            ok
                ? (onlyCover
                    ? '封面已提取出来'
                    : '解开了 ${r.entriesDone} 个文件')
                : '只解出了一部分（${r.entriesDone}/${r.entriesTotal}）',
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
          // 嵌套包（产物里还套着压缩包）：问一句，不自动解
          if (_nestedArchives.isNotEmpty) ...[
            const SizedBox(height: 10),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
              decoration: BoxDecoration(
                color: cs.secondaryContainer,
                borderRadius: BorderRadius.circular(10),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.inventory_2_outlined,
                      size: 16, color: cs.onSecondaryContainer),
                  const SizedBox(width: 8),
                  Text(
                    '里面还有 ${_nestedArchives.length} 个压缩包',
                    style: TextStyle(fontSize: 12, color: cs.onSecondaryContainer),
                  ),
                  TextButton(
                    onPressed: () => _scan(_nestedArchives),
                    child: const Text('继续解压'),
                  ),
                ],
              ),
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
                    color: hint.warn ? cs.error : null,
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
            // 索引坏了也有救：CLI 的 --rescue 在界面上就是这个按钮。
            // 用 OutlinedButton 不用 FilledButton —— 首屏「唯一最强入口」的约束
            // 有 widget 测试钉着，这里再放一个实心按钮会诱导误点。
            if (_canRescue) ...[
              const SizedBox(height: 14),
              OutlinedButton.icon(
                key: const ValueKey('rescue'),
                onPressed: _rescue,
                icon: const Icon(Icons.healing_outlined, size: 18),
                label: const Text('试着救出来（索引坏了也能抢救）'),
              ),
              const SizedBox(height: 4),
              Text(
                '按文件头把能救的条目捞出来，可能慢一些，也可能缺东西',
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
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

  /// 批量结论 → 人话（老引擎给 Debug 名、新引擎给码，两种都认；见 `humanize.dart`）
  String _batchOutcomeText(String oc) => batchOutcomeText(oc);

  /// 在文件管理器里定位某个文件（失败项用）
  Future<void> _revealInFolder(String path) async {
    if (path.isEmpty) return;
    await openDirectory(File(path).parent.path);
  }

  /// 智能解压的结果预告（与引擎的三条规则对齐）
  ///
  /// 单文件 → 直接解；包里只有一个顶层目录 → 解到那个目录；散文件 → 新建同名文件夹。
  /// 说清楚这句，用户点「开始解压」前就知道磁盘上会多出什么。
  String _smartDirHint(Preview pv) {
    final files = pv.entries.where((e) => !e.isDir).toList();
    if (files.isEmpty) return '包里没有文件';
    if (files.length == 1) return '会直接解出这 1 个文件';
    final roots = files.map((e) => e.path.split('/').first).toSet();
    final allNested = files.every((e) => e.path.contains('/'));
    if (roots.length == 1 && allNested) {
      return '会解到「${roots.first}/」里（包里本来就有这个目录，不会多套一层）';
    }
    return '会新建一个同名文件夹，把 ${files.length} 个文件放进去（散文件）';
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
