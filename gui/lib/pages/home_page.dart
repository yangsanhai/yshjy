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
import '../l10n.dart';
import '../format.dart';

import 'home_queue_view.dart';

/// 界面状态机（对应 UI-SPEC 的 S1 ~ S9 / S11）
enum Phase { empty, listing, preview, extracting, done, error, queue }

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

  /// 测试用：直接灌一个失败态（生产代码永远传 null）。
  ///
  /// 和 `debugPreview` 同一个理由 —— 「加密包失败后要出现解压码输入框」这条链路
  /// 以前**一条测试都没有**，所以 43 条全绿也能漏着「判断条件恒假」的死 bug
  /// （2026-10-30 用户在 Windows 上撞到：只见报错，永远没有输入框）。
  final String? debugErrorCode;
  final String? debugErrorNote;

  /// 测试用：失败态的结构化 detail（空间不足的 need / available 就在这里面）。
  /// 没有它，「三个数」只能靠真机撞小容量盘才验得到 —— 那条路要 sudo 挂 loop 盘。
  final Map<String, dynamic>? debugErrorDetail;

  /// 测试用：直接灌一份 `scan` 原始结果进队列（生产代码永远传 null）。
  ///
  /// 传的是**引擎原样输出**（`items` 里那几个 Map），不是界面自己造的模型 ——
  /// 这样「解析 + 渲染」整条链都被测到，不会出现"测试看着绿、真机字段对不上"。
  final List<Map<String, dynamic>>? debugScanItems;

  /// 测试用：预设「输出到指定目录」（生产代码永远 null）。
  ///
  /// 为什么要这个口子：「智能解压」开关在**选了输出目录时必须禁用**（引擎给了 dest
  /// 就忽略 smart_dir），而选目录要走系统对话框、widget 测试点不到 ——
  /// 不给口子这条「禁用 + 说明」就永远没人验，下一轮很容易被改回一个点了没反应的开关。
  final String? debugOutDir;

  /// 测试用：直接进「批量解压中」这一屏，并造 N 个排队路径（生产代码永远 null）
  ///
  /// 为什么要这个口子：「全部暂停」按钮只在批量跑的时候出现，而真跑批量要起
  /// 真引擎 —— widget 测试里跑不起来。没有它，「按钮该出现的时候出现了、
  /// 不该出现的时候没出现」这两条都只能靠手点。
  final int? debugBatchCount;

  /// 测试用：把界面置成「引擎已确认暂停」（生产代码永远 false）
  ///
  /// 单独一个口子的理由：真实暂停要等引擎发 `PAUSED` 事件，测试里没有引擎；
  /// 少了它，「已暂停时按钮翻成继续 + 出现状态说明」就验不到。
  final bool debugPaused;

  /// 测试用：首帧之后按顺序喂给事件处理器的事件（模拟引擎推过来的 PAUSED 等）
  ///
  /// 和 `debugPaused` 的区别：那个只摆初始状态，这个能验**状态转移** ——
  /// 「恢复」时到底有没有把「已暂停」那句清掉，只能靠喂一条 `PAUSED{paused:false}`
  /// 才测得到（这是用户真机验收抓到的 bug）。
  final List<EngineEvent>? debugEvents;

  const HomePage({
    super.key,
    this.initialFile,
    this.onStatus,
    this.autoStart = true,
    this.debugPreview,
    this.debugErrorCode,
    this.debugErrorNote,
    this.debugErrorDetail,
    this.debugScanItems,
    this.debugOutDir,
    this.debugBatchCount,
    this.debugPaused = false,
    this.debugEvents,
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
  // ── 移动设备落盘检验（引擎的 FLUSH_PROGRESS / FLUSH_FINISHED）──
  // 用户 2026-10-30 拍板：只有主动开了开关的人才会看到这一段，
  // 而且必须给他「有东西在动」—— 关键技术上写 300 MB 要好几分钟，一动不动就是卡死。
  bool _flushing = false;
  int _flushPending = 0; // 还没真写进设备的字节数（内核读数，会一路往下降）
  int _flushSecs = 0; // 已经等了多少秒
  int _flushPeak = 0; // 见过的最大待写入量：用来算进度条，保证只前进不回退
  ExtractResult? _result;
  String? _errorCode;

  /// 这一轮是「只检查」而不是真解压（E4 / 协议 `extract.verify_only`）
  ///
  /// 结果页据此换文案：验证时引擎照样报 `done=2 / bytes=13440`（它确实解码+校验了），
  /// 但**一个字节都没落盘**，`dest` 目录也不存在。照普通成功页写「解开了 2 个文件」
  /// 加「打开目录」，用户点进去只会看到「目录不存在」—— 和以前那种假成功是同一类毛病。
  bool _verifyMode = false;

  /// 引擎给的结构化细节（如缺分卷的 missing/found）—— 失败卡据此说"缺哪一块"
  Map<String, dynamic>? _errorDetail;

  /// 空间不足的两个数（引擎的 `SPACE_SHORTAGE` 事件先到，错误对象随后才到）
  ///
  /// 为什么单独存：2026-10-30 真机上协议层把 detail 吞了，`_errorDetail` 是空的，
  /// 界面上只剩一句「清一点空间再试」、三个数全无。事件是第二个来源，双保险。
  int? _spaceNeed;
  int? _spaceAvail;
  String? _errorNote;
  EngineError? _error;
  final TextEditingController _pwCtrl = TextEditingController();
  bool _dragging = false;

  /// 队列（S13）与批量结果
  final List<QueueItem> _queue = [];
  List<String> _batchOrder = [];

  /// 用户在界面上单独取消掉的任务号（D5）——
  /// 批量结果回来时要保住「已取消」这个状态，别被「失败」覆盖掉
  final Set<int> _cancelled = {};

  /// 队列「全部暂停」（T18-4）
  ///
  /// 两个状态必须分开，不然界面会撒谎：
  /// - `_pauseRequested` = 请求已发出，**引擎还没停**（当前文件还在解）
  /// - `_paused` = 引擎发来的 `PAUSED` 事件确认了「真的停住了」
  bool _pauseRequested = false;
  bool _paused = false;

  /// 这一轮扫的是几个文件夹（D6：拖文件夹进来要明说「会自动展开」）
  int _foldersScanned = 0;
  Map<String, dynamic>? _batch;

  /// S10：正在挨个换候选解压（按钮据此禁用，避免连点堆起一串任务）
  bool _switching = false;
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

  /// 智能解压（E5 / 协议 `extract.smart_dir`）
  ///
  /// 默认开，与引擎/命令行一致：包里只有一个顶层目录或只有一个文件时**不**多套一层。
  /// 关掉 = 永远建一个和包同名的文件夹（`--no-smart-dir` 的语义）。
  /// ⚠️ 选了「输出到指定目录」时它不生效（引擎两种模式都直接铺进 dest）—— 界面要
  /// 禁用并说明，不能留个点了没反应的开关。
  bool _smartDir = true;
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
      _file = t('home.samplePath');
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
    // 测试注入口：预设「输出到指定目录」（与 debugPreview 同理，只影响测试）
    if (widget.debugOutDir != null) _outDir = widget.debugOutDir;
    // 测试注入口：直接进「批量解压中」（与 debugPreview 同理，只影响测试）
    final bc0 = widget.debugBatchCount;
    if (bc0 != null && bc0 > 0) {
      _batchOrder = List.generate(bc0, (i) => '/tmp/第${i + 1}个.zip');
      _phase = Phase.extracting;
      _stage = 'EXTRACT';
      _paused = widget.debugPaused;
    }
    // 测试注入口：直接进失败态（与 debugPreview 同样只影响测试）
    final ec0 = widget.debugErrorCode;
    if (ec0 != null && ec0.isNotEmpty) {
      _file = t('home.samplePath');
      _errorCode = ec0;
      _errorNote = widget.debugErrorNote ?? '';
      _errorDetail = widget.debugErrorDetail;
      _phase = Phase.error;
    }
    // 测试注入口：直接灌一份 scan 结果进队列（与 debugPreview 同一个理由）
    final q0 = widget.debugScanItems;
    if (q0 != null) {
      _applyScanItems(q0);
      _phase = Phase.queue;
    }
    // 从右键菜单带文件启动：等首帧画完再开，避免 setState 早于 build
    final f = widget.initialFile;
    if (f != null && f.isNotEmpty && pv0 == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _open(f));
    }
    // 测试注入口：首帧之后按顺序喂事件（模拟引擎推过来的 PAUSED 等）
    //
    // 为什么要这个口子：「恢复后必须把『已暂停』清掉」这条只有事件能驱动 ——
    // 这正是用户真机验收抓到的 bug（进度在涨、状态栏还挂着已暂停）。
    // 没有它，回归测试只能验「暂停时显示了对的字」，验不到「恢复后把字清了」。
    final preEvents = widget.debugEvents;
    if (preEvents != null && preEvents.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        for (final e in preEvents) {
          _onEvent(e);
        }
      });
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
      case 'FLUSH_PROGRESS':
        final pending = (e.data['pending'] as num?)?.toInt() ?? 0;
        final ms = (e.data['elapsed_ms'] as num?)?.toInt() ?? 0;
        setState(() {
          _flushing = true;
          _flushPending = pending;
          _flushSecs = ms ~/ 1000;
          if (pending > _flushPeak) _flushPeak = pending;
        });
      case 'FLUSH_FINISHED':
        final outcome = e.data['outcome']?.toString() ?? '';
        final ms = (e.data['millis'] as num?)?.toInt() ?? 0;
        final secs = (ms / 1000).toStringAsFixed(1);
        final text = switch (outcome) {
          'DONE' => t('home.flushOk', {'secs': secs}),
          'CANCELLED' => t('home.flushCancelled', {'secs': secs}),
          _ => t('home.flushNoCheck'),
        };
        setState(() {
          _flushing = false;
          _flushPeak = 0;
          if (!_notes.contains(text)) _notes.add(text);
        });
      case 'DIAGNOSTIC':
        final code = e.data['code']?.toString() ?? '';
        final text = _diagText(code);
        if (!_notes.contains(text)) setState(() => _notes.add(text));
      case 'RESUMED':
        // 断点续传：上次没解完，这次接着来。
        // 必须给用户一句话，否则「一上来就有一大截进度、前面那堆文件没动」会被当成 bug。
        final done = (e.data['done'] as num?)?.toInt() ?? 0;
        final text = t('home.resumeFromLast', {'done': '$done'});
        if (!_notes.contains(text)) setState(() => _notes.add(text));
      case 'PAUSED':
        // T18-4：引擎确认「真停住了 / 已恢复」。
        // 注意这**不是** pause 命令的回执（那个只说明请求送到了）——
        // 按钮必须等这个事件才翻，否则用户点了会以为没反应。
        final paused = e.data['paused'] == true;
        setState(() {
          _paused = paused;
          if (paused) _pauseRequested = false;
        });
        if (paused) {
          widget.onStatus?.call(t('home.pausedNote'));
        } else {
          // 恢复时必须把「已暂停」那句**清掉** —— 用户真机验收时抓到：
          // 点了「继续」、进度已经在涨（331.8 MB / 1.38 GB），底部状态栏却还挂着
          // 「已暂停，点「继续」接着跑」，界面在撒谎。改成回到「正在解压…」。
          widget.onStatus?.call(t('home.extracting'));
        }
      case 'PASSWORD_TRY':
        setState(() {
          _pwTry = (e.data['index'] as num?)?.toInt() ?? 0;
          _pwTotal = (e.data['total'] as num?)?.toInt() ?? 0;
        });
      case 'PASSWORD_NEEDED':
        if (!_notes.contains(t('home.needPassword'))) setState(() => _notes.add(t('home.needPassword')));
      case 'SPACE_SHORTAGE':
        // 预检失败时引擎先发这个事件，错误对象随后才到。
        // 两路都留着：错误 detail 是主路径，事件是兜底（真机上曾出现 detail 被吞）。
        final need = (e.data['needed'] as num?)?.toInt();
        final avail = (e.data['available'] as num?)?.toInt();
        setState(() {
          if (need != null) _spaceNeed = need;
          if (avail != null) _spaceAvail = avail;
        });
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
      'possible_zip_bomb' => t('home.bombWarn'),
      'extract_size_limit_exceeded' => t('home.limitFiles'),
      _ => t('home.limitTotal'),
    };
    final choice = await showDialog<String>(
      context: context,
      barrierDismissible: false, // 误点外面 = 不回答，等于让用户白等 60 秒
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.warning_amber_rounded, size: 32),
        title: Text(t('home.bombConfirm')),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(why, style: const TextStyle(fontSize: 13, height: 1.5)),
            const SizedBox(height: 10),
            Text(
              t('home.writtenLimit', {
                'written': _size(written),
                'cap': limit > 0 ? t('home.writtenCap', {'size': _size(limit)}) : '',
              }),
              style: const TextStyle(fontSize: 12),
            ),
            const SizedBox(height: 8),
            Text(
              t('home.bombTimeout'),
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
            child: Text(t('home.cancelAll')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, 'skip'),
            child: Text(t('home.skipThis')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, 'continue'),
            child: Text(t('home.continueExtract')),
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
    final group = XTypeGroup(label: t('home.archiveOrCover'));
    try {
      // 多选：一次挑一堆包是常态（网盘一次下 8 个）
      final files = await openFiles(acceptedTypeGroups: [group]);
      if (files.isEmpty) return;
      if (files.length == 1) {
        await _open(files.first.path);
      } else {
        await _scan(files.map((f) => f.path).toList());
      }
    } on PlatformException catch (e) {
      _showError('FILE_PICKER', e.message ?? t('home.cantPickFile'));
    }
  }

  /// 选一个文件夹：目录里可能混着一堆无关文件，交给引擎筛
  Future<void> _pickFolder() async {
    try {
      final dir = await getDirectoryPath();
      if (dir != null) await _scan([dir]);
    } on PlatformException catch (e) {
      _showError('FILE_PICKER', e.message ?? t('home.cantPickDir'));
    }
  }

  /// 把 `scan` 的原始 items 变成队列行
  ///
  /// **生产与测试共用这一份**：`debugScanItems` 注入的也是引擎原样输出，
  /// 要是各写一份解析，就会出现"测试里字段对、真机解析对不上"的假绿。
  void _applyScanItems(List items) {
    for (final raw in items) {
      final i = (raw as Map).cast<String, dynamic>();
      _queue.add(QueueItem(
        path: i['path'] as String? ?? '',
        name: i['name'] as String? ?? '',
        size: (i['size'] as num?)?.toInt() ?? 0,
        kind: i['kind'] as String? ?? 'UNKNOWN',
        volumes: (i['volumes'] as num?)?.toInt() ?? 1,
        archive: i['archive'] == true,
        candidates: EngineClient.candidatesOf(i),
      ));
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
        _applyScanItems(items);
        _phase = Phase.queue;
      });
      widget.onStatus?.call(t('home.foundArchives', {'n': '${_queue.where((q) => q.archive).length}'}));
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

  /// S10 多候选：这个文件里像是有好几处藏着东西 —— 替用户挨个试，解开就停
  ///
  /// 为什么不让用户自己挑：新手面对「第 3 处 · 从开头算 5.2 GB 处」根本无从判断，
  /// 反而更慌。界面只做一件事：按候选顺序（可信度高的在前、默认那处放最后）替他试。
  ///
  /// 引擎侧走 `locate_at_base`（协议 `extract.candidate`）：**试哪段就解哪段**，
  /// 对不上就报 `NOT_ARCHIVE`，绝不会偷偷回头解默认那处（那种"假成功"最难发现）。
  Future<void> _tryOtherCandidates(int i) async {
    if (i < 0 || i >= _queue.length) return;
    final q = _queue[i];
    if (!q.hasManyCandidates) return;

    // 默认那处放最后：用户点这个按钮，多半是因为刚才「开始解压」用它已经失败了
    final order = [
      ...q.candidates.where((c) => !c.chosen),
      ...q.candidates.where((c) => c.chosen),
    ];

    setState(() {
      _phase = Phase.extracting;
      _progress = null;
      _current = q.name;
      _nestedArchives = [];
    });

    String lastError = '';
    for (var k = 0; k < order.length; k++) {
      final c = order[k];
      if (!mounted) return;
      setState(() => q.status = 'running');
      widget.onStatus?.call(
          t('home.tryingSpot', {'k': '${k + 1}', 'total': '${order.length}', 'offset': c.offsetText}));
      try {
        final r = await _client.extract(
          q.path,
          password: _password,
          dest: _outDir,
          encoding: _encoding,
          candidate: c.offset,
        );
        if (!mounted) return;
        setState(() {
          q.status = 'ok';
          _result = r;
          _phase = Phase.done;
        });
        widget.onStatus?.call(t('home.spotOpened', {'k': '${k + 1}', 'entries': '${r.entriesDone}'}));
        Notifier.send(
          t('home.done'),
          t('home.spotResult', {'k': '${k + 1}', 'total': '${order.length}', 'entries': '${r.entriesDone}', 'size': _size(r.bytesWritten)}),
        );
        await HistoryStore.add(HistoryEntry(
          time: DateTime.now().toIso8601String(),
          input: q.path,
          dest: r.dest,
          ok: r.ok,
          bytes: r.bytesWritten,
        ));
        await _maybeOpenAfterDone(r.dest);
        _probeNested();
        return;
      } on EngineError catch (e) {
        lastError = e.note.isEmpty ? e.code : e.note;
      }
    }

    // 几处全试完都不行：如实报错，别假装解开了
    setState(() => q.status = 'failed');
    _showError('NOT_ARCHIVE', lastError.isEmpty ? t('home.noEntryOpens') : lastError);
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
      if (mounted) widget.onStatus?.call(t('home.itemCancelled'));
    } else if (mounted) {
      widget.onStatus?.call(t('home.itemSkipped'));
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
      // 新一轮批量：暂停状态必须归零 —— 否则上一轮残留的「已暂停」
      // 会让这一轮一进来就显示「继续」，实际引擎在跑
      _paused = false;
      _pauseRequested = false;
      for (final q in ready) {
        q.status = 'pending';
        q.jobNo = null;
      }
      _batchOrder = ready.map((q) => q.path).toList();
    });
    widget.onStatus?.call(t('home.batchRunning', {'n': '${ready.length}'}));
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
        // T18-4 同类问题（顺藤摸出来的）：批量结束必须把暂停标志一起复位。
        // 否则「pause 请求发出去了、但任务在到达挂起点之前就解完了」会让
        // _pauseRequested 永远停在 true —— 下一轮进来按钮直接是灰的、点不动。
        // 规矩：凡是按钮状态标志，都要有「任务结束」这个终点复位。
        _paused = false;
        _pauseRequested = false;
      });
      final ok = (r['ok'] as num?)?.toInt() ?? 0;
      final failed = (r['failed'] as num?)?.toInt() ?? 0;
      widget.onStatus?.call(t('home.batchDone', {'ok': '$ok', 'failed': '$failed'}));
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
      // 换包必须清掉上一个包的解压码 —— 否则旧码会静默套到新包上，
      // 表现成「明明没输密码却报解压码不对」（2026-10-30 顺手补）
      _password = null;
      _pwCtrl.clear();
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
      widget.onStatus?.call(t('home.identified', {'files': '${pv.fileCount}'}));
    } on EngineError catch (e) {
      _showError(e.code, e.note, e);
    }
  }

  Future<void> _extract(
      {bool force = false, int? candidate, bool verifyOnly = false}) async {
    final path = _file;
    if (path == null) return;
    setState(() {
      _phase = Phase.extracting;
      _verifyMode = verifyOnly;
      _progress = null;
      _current = null;
      _nestedArchives = [];
    });
    try {
      widget.onStatus?.call(verifyOnly ? t('home.verifyNoWrite') : t('home.extracting'));
      final pv = _preview;
      // 全勾 = 不传 select（引擎默认全解，与以前行为完全一致）
      final pickedAll = pv == null ||
          _picked.length == pv.entries.where((e) => !e.isDir).length;
      final r = await _client.extract(
        path,
        password: _password,
        // 「只检查」时封面必须关：切封面是真写盘的另一条路，
        // 开着它就与「硬盘一个字节都不动」矛盾（实测 CLI --test 是零字节）。
        cover: verifyOnly ? false : _wantCover,
        // 校验不跟随界面勾选：勾了一半不等于包只坏了一半，
        // 而且用户点「只检查」想要的答案就是「整包完不完好」。
        wantExtract: verifyOnly ? true : _wantExtract,
        dest: verifyOnly ? null : _outDir,
        // S10：换候选解压时不带挑选序号 —— 另一段里的条目跟当前预览完全可能不同，
        // 带着旧序号去解等于用 A 包的清单挑 B 包的文件（会漏解或报错）。
        select: (verifyOnly || !_wantExtract || pickedAll || candidate != null)
            ? null
            : _picked.toList(),
        encoding: _encoding,
        // S5「继续试试」：跳过空间预检（引擎侧早支持，界面以前没传）
        force: force,
        // S10 多候选：只解用户点的那一处（预览页「换一处解压」）
        candidate: candidate,
        // E4：只检查不落盘（协议 verify_only）
        verifyOnly: verifyOnly,
        // E5：智能解压（不传 = 引擎默认 true，与老行为一致）
        smartDir: _smartDir,
      );
      if (!mounted) return;
      setState(() {
        _result = r;
        _phase = Phase.done;
      });
      widget.onStatus?.call(verifyOnly
          ? (r.ok ? t('home.verifyPassed', {'entries': '${r.entriesDone}'}) : t('home.verifyProblem'))
          : (r.ok ? t('home.doneEntries', {'entries': '${r.entriesDone}'}) : t('home.partialDone')));
      // 解压完发桌面通知：切到别的窗口也知道；发不出不影响主流程
      Notifier.send(
        verifyOnly
            ? (r.ok ? t('home.verifyOk') : t('home.verifyProblem'))
            : (r.ok ? t('home.done') : t('home.extractPartial')),
        verifyOnly
            ? t('home.doneNoWrite', {'entries': '${r.entriesDone}'})
            : t('home.doneWithSize', {'entries': '${r.entriesDone}', 'size': _size(r.bytesWritten)}),
      );
      // 「只检查」到此为止：没有产物、没有目录、也没有历史。
      // 这三样都别做 —— 否则历史页会多一条指向不存在目录的记录，
      // _maybeOpenAfterDone 还会真去打开它（就是那类「点了没反应」）。
      if (verifyOnly) return;
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

  /// S10：当前这处像是不对 —— 按候选顺序换别的，直接解出来
  ///
  /// 为什么不重新 list 一遍再让用户挑：用户要的是「解出对的东西」，不是再看清单；
  /// 而且多候选文件定位要走全文件扫描，重新预览等于再扫一遍（8 GB 包要几十秒）。
  Future<void> _extractOtherCandidate() async {
    final pv = _preview;
    if (pv == null || !pv.hasManyCandidates || _switching) return;
    // 当前这处（chosen）跳过，按可信度顺序试其余的
    final order = pv.candidates.where((c) => !c.chosen).toList();
    if (order.isEmpty) return;

    setState(() => _switching = true);
    widget.onStatus?.call(t('home.tryAllSpots', {'n': '${order.length}'}));
    for (var k = 0; k < order.length; k++) {
      if (!mounted) return;
      widget.onStatus?.call(t('home.tryingSpot', {'k': '${k + 1}', 'total': '${order.length}', 'offset': order[k].offsetText}));
      await _extract(candidate: order[k].offset);
      if (!mounted) return;
      // 解开了就停（_extract 成功会把 phase 设成 done）
      if (_phase == Phase.done) break;
    }
    if (mounted) setState(() => _switching = false);
  }

  /// 这些失败还能抢救：索引（中央目录）坏了、数据只坏了一部分。
  ///
  /// 密码错、缺分卷、目标无权限这类救不了 —— 给按钮只会让用户白等一场。
  /// `NOT_ARCHIVE` 要收窄：认不出压缩包时，只有「内容其实是 ZIP、只是索引坏了」
  /// 才值得试（否则用户拖进来的就是个视频，救援也只会再说一遍认不出）。
  bool get _canRescue =>
      codeCanRescue(_errorCode, looksLike: _errorDetail?['looks_like'] as String?);

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
      widget.onStatus?.call(t('home.rescueMode'));
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
      widget.onStatus?.call(t('home.rescueSaved2', {'n': '$savedCount'}));
      Notifier.send(t('home.rescueDone'), t('home.rescueSaved', {'n': '$savedCount'}));
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
          title: Text(t('home.advanced'), style: const TextStyle(fontSize: 15)),
          contentPadding: const EdgeInsets.fromLTRB(16, 4, 16, 4),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.drive_folder_upload_outlined, size: 18),
                title: Text(t('home.outCustom'), style: const TextStyle(fontSize: 13)),
                subtitle: Text(
                  _outDir ?? t('home.outDefault'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 11),
                ),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (_outDir != null)
                      IconButton(
                        tooltip: t('home.outReset'),
                        icon: const Icon(Icons.restart_alt, size: 18),
                        onPressed: () => setDlg(() => _outDir = null),
                      ),
                    TextButton(
                      onPressed: () async {
                        final d = await getDirectoryPath(confirmButtonText: t('home.outHere'));
                        if (d != null) setDlg(() => _outDir = d);
                      },
                      child: Text(t('home.choose'), style: const TextStyle(fontSize: 12)),
                    ),
                  ],
                ),
              ),
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.translate, size: 18),
                title: Text(t('home.encoding'), style: const TextStyle(fontSize: 13)),
                subtitle: Text(t('home.encodingHint'),
                    style: const TextStyle(fontSize: 11)),
                trailing: DropdownButton<String>(
                  value: _encoding,
                  isDense: true,
                  style: const TextStyle(fontSize: 12),
                  items: [
                    DropdownMenuItem(value: 'auto', child: Text(t('home.encodingAuto'))),
                    DropdownMenuItem(value: 'gbk', child: Text(t('home.encodingGbk'))),
                    DropdownMenuItem(value: 'big5', child: Text(t('home.encodingBig5'))),
                    DropdownMenuItem(value: 'sjis', child: Text(t('home.encodingSjis'))),
                    const DropdownMenuItem(value: 'utf-8', child: Text('UTF-8')),
                  ],
                  onChanged: (v) => setDlg(() => _encoding = v ?? 'auto'),
                ),
              ),
              // 智能解压（E5）：不重复套文件夹。
              // 选了「输出到指定目录」时它**不生效**（引擎给了 dest 就两种模式都直接铺进去）
              // —— 所以这里禁用并写明原因，不做点了没反应的假开关。
              SwitchListTile(
                key: const ValueKey('smart-dir'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                value: _smartDir,
                onChanged:
                    _outDir != null ? null : (v) => setDlg(() => _smartDir = v),
                secondary: const Icon(Icons.folder_copy_outlined, size: 18),
                title: Text(t('home.smartDir'),
                    style: const TextStyle(fontSize: 13)),
                subtitle: Text(
                  _outDir != null
                      ? t('home.smartDirOffWithOut')
                      : (_smartDir
                          ? t('home.smartDirOn')
                          : t('home.smartDirOff')),
                  style: const TextStyle(fontSize: 11),
                ),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: Text(t('home.gotit')),
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
    Notifier.send(t('home.extractFailed'), note.isEmpty ? code : note);
    setState(() {
      _phase = Phase.error;
      _errorCode = code;
      // 事件里的两个数并进 detail：协议层漏 detail 时（真机撞过）靠这条兜底
      _errorDetail = {
        ...?e?.detail,
        if (_spaceNeed != null) 'need': _spaceNeed,
        if (_spaceAvail != null) 'available': _spaceAvail,
      };
      _errorNote = note;
      _error = e;
    });
  }

  /// 一键生成诊断包（路径已脱敏），发给开发者就能定位问题
  ///
  /// E6：**必须给 out_dir**。引擎不给就落在进程当前目录（GUI 从哪启动就落在哪），
  /// 用户看到「已生成」却满电脑找不到文件 —— 比报错还气人。默认落「下载」，
  /// 并配一个「打开文件夹」，让他一眼看到东西在哪。
  Future<void> _makeReport() async {
    try {
      final dir = downloadsDir;
      final r = await _client.report(path: _file, outDir: dir);
      final p = r['path'] as String? ?? '';
      if (!mounted) return;
      final name = p.split(Platform.pathSeparator).last;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(t('home.diagSaved', {'path': prettyPath(dir), 'name': name})),
          duration: const Duration(seconds: 12),
          action: SnackBarAction(
            label: t('home.openFolder'),
            onPressed: () => openDirectory(dir),
          ),
        ),
      );
    } on EngineError catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(t('home.diagFailed', {'reason': _errText(e.code)}))));
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
          l.contains(t('home.launch')) ||
          l.contains(t('home.startGame')) ||
          l.endsWith('.bat') ||
          l.endsWith('run.sh') ||
          l.endsWith('.appimage') ||
          l.endsWith('game.exe');
    }

    final hits = pv.entries.where((e) => !e.isDir && looksLikeEntry(e.path)).toList();
    return hits.isEmpty ? null : hits.first.path;
  }

  /// 这次失败是不是"缺解压码"（资源分享里最常见的情况）
  ///
  /// 判断本身在 `humanize.dart` 的 `codeNeedsPassword`（有单测挡着）；页面里只委托，
  /// **别在这再手写码名** —— 2026-10-30 就是这么写成 `PASSWORD_REQUIRED` 的：
  /// 引擎从不发那个码，于是加密包在界面上只剩一句报错、永远没有输入框。
  bool get _needsPassword => codeNeedsPassword(_errorCode);

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
        'mp4' => t('home.fmt.mp4'),
        'mov' => t('home.fmt.mov'),
        'mkv' => t('home.fmt.mkv'),
        'webm' => t('home.fmt.webm'),
        'avi' => t('home.fmt.avi'),
        'flv' => t('home.fmt.flv'),
        'ts' => t('home.fmt.ts'),
        'png' => t('home.fmt.png'),
        'jpeg' => t('home.fmt.jpg'),
        'gif' => t('home.fmt.gif'),
        'webp' => t('home.fmt.webp'),
        'bmp' => t('home.fmt.bmp'),
        'tiff' => t('home.fmt.tiff'),
        'mp3' => t('home.fmt.mp3'),
        'flac' => t('home.fmt.flac'),
        'ogg' => t('home.fmt.ogg'),
        'wav' => t('home.fmt.wav'),
        'pdf' => t('home.fmt.pdf'),
        'elf' => t('home.fmt.elf'),
        'exe' => t('home.fmt.pe'),
        'zip' => t('home.fmt.zip'),
        '7z' => t('home.fmt.7z'),
        'rar' => t('home.fmt.rar'),
        'gzip' => t('home.fmt.gz'),
        'xz' => t('home.fmt.xz'),
        'zstd' => t('home.fmt.zst'),
        'bzip2' => t('home.fmt.bz2'),
        'lz4' => t('home.fmt.lz4'),
        'text' => t('home.fmt.text'),
        _ => t('home.fmt.unknown'),
      };

  /// 失败时那句「这个文件其实是……」
  ({String text, bool warn})? _typeHint() {
    final e = _error;
    if (e == null || e.detail.isEmpty) return null;
    final size = e.size;
    if (size != null && size == 0) {
      return (
        text: t('home.emptyFile'),
        warn: false
      );
    }
    final kind = _kindName(e.looksLike ?? 'unknown');
    final sizeText = size == null ? '' : '，${_size(size)}';
    if (e.extMismatch) {
      final extName = '.${e.ext ?? ''}';
      if (e.looksKind == 'archive') {
        return (text: t('home.guiseLikely', {'ext': extName, 'kind': kind, 'size': sizeText}), warn: false);
      }
      if (e.looksKind == 'program') {
        return (text: t('home.guiseProgram', {'ext': extName, 'kind': kind, 'size': sizeText}), warn: true);
      }
      return (text: t('home.guiseRenamed', {'ext': extName, 'kind': kind, 'size': sizeText}), warn: false);
    }
    if (e.looksKind == 'program') {
      return (text: t('home.guiseReal', {'kind': kind, 'size': sizeText}), warn: true);
    }
    if ((e.looksLike ?? '') == 'unknown') {
      return (text: t('home.unknownFile', {'size': sizeText}), warn: false);
    }
    return (text: t('home.fileLooksLike', {'kind': kind, 'size': sizeText}), warn: false);
  }

  Future<void> _openFolder(String? dir) async {
    if (dir == null) return;
    final ok = await openDirectory(dir);
    if (!ok) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(t('home.cantOpenFM', {'dir': dir}))));
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
    if (mounted) widget.onStatus?.call(t('home.itemSkippedShort'));
  }

  /// 队列「全部暂停 / 继续」（T18-4）
  ///
  /// 暂停是**软停**：引擎不会打断正在解的那个文件，而是在它解完那一刻停住
  /// （已解完的条目一条不丢、也不会重解）。所以点下去先显示「正在暂停…」，
  /// 等引擎发 `PAUSED` 事件才翻成「继续」—— 别让用户以为点了没反应。
  Future<void> _pauseOrResume() async {
    if (_paused) {
      setState(() => _pauseRequested = false);
      await _client.resumeQueue();
      return;
    }
    if (_pauseRequested) return; // 已经在等它停，重复发没意义
    setState(() => _pauseRequested = true);
    final r = await _client.pauseQueue();
    if (!mounted) return;
    if (r['delivered'] != true) {
      // 引擎那边已经没有任务在跑了（刚好结束 / 还没开始）：老实复位，不假装
      setState(() {
        _pauseRequested = false;
        _paused = false;
      });
    }
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
            Text(t('home.brandName')),
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
            tooltip: t('home.pickFile'),
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
      Phase.listing => _busyView(t('home.identifying')),
      Phase.preview => _previewView(),
      Phase.extracting => _progressView(),
      Phase.done => _batch == null ? _doneView() : _batchDoneView(),
      Phase.queue => _queueView(),
      Phase.error => _errorView(),
    };
  }

  // ───────────────────────── 批量队列（S13）─────────────────────────

  Widget _queueView() => QueueView(
        queue: _queue,
        foldersScanned: _foldersScanned,
        onReset: _resetQueue,
        // Del / Backspace 与那个 × 按钮改的是同一份队列，走同一个回调
        onRemove: (i) => setState(() => _queue.removeAt(i)),
        onCancelRow: _cancelRow,
        onTryOtherCandidates: _tryOtherCandidates,
        onStartAll: _startQueue,
      );

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
            'size': _size(bytes),
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
                  label: Text(t('home.openOutDir')),
                ),
              ),
            if (firstDest != null) const SizedBox(width: 8),
            Expanded(
              child: OutlinedButton.icon(
                onPressed: _resetQueue,
                icon: const Icon(Icons.refresh, size: 18),
                label: Text(t('home.again')),
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
            _dragging ? t('home.dropHint') : t('home.dropTitle'),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            t('home.dropDesc'),
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
                label: Text(t('home.pickFile')),
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
                label: Text(t('home.pickDir'), style: const TextStyle(fontSize: 12)),
              ),
            ],
          ),
          const SizedBox(height: 24),
          Text(
            t('home.anyFormatHint'),
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
        // S10 多候选：一个文件里好几处都像藏着东西
        //
        // 为什么放在**预览页**而不是只在队列里：用户拖单个文件进来走的就是这条路
        // （`_open` → 预览），只看不到队列。真机验时才发现这点 —— 队列那版做完
        // 在真机上一次都碰不到，等于没做。
        if (pv.hasManyCandidates)
          Container(
            key: const ValueKey('multi-candidates'),
            margin: const EdgeInsets.only(bottom: 8),
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
            decoration: BoxDecoration(
              color: cs.primaryContainer.withValues(alpha: 0.35),
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: cs.primary.withValues(alpha: 0.4)),
            ),
            child: Row(
              children: [
                Icon(Icons.travel_explore, size: 16, color: cs.primary),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    t('home.candidateCount2', {'n': '${pv.candidates.length}'}),
                    style: TextStyle(fontSize: 12, color: cs.onSurface),
                  ),
                ),
                TextButton(
                  onPressed: _switching ? null : () => _extractOtherCandidate(),
                  child: Text(_switching ? t('home.switching') : t('home.switchCandidate'),
                      style: const TextStyle(fontSize: 12)),
                ),
              ],
            ),
          ),
        Row(
          children: [
            Chip(
              label: Text(pv.format.toUpperCase()),
              visualDensity: VisualDensity.compact,
            ),
            const SizedBox(width: 8),
            Text(t('home.fileCountSize', {'files': '${pv.fileCount}', 'size': _size(pv.totalSize)})),
            const Spacer(),
            if (pv.encryptedCount > 0)
              Chip(
                avatar: const Icon(Icons.lock_outline, size: 16),
                label: Text(t('home.encryptedCount', {'n': '${pv.encryptedCount}'})),
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
              t('home.indexIncomplete'),
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
                'zipcrypto' => t('home.encZipcrypto'),
                'aes256' => t('home.encAes256'),
                'aes128' || 'aes192' => t('home.encAes'),
                'aes' => t('home.encAesShort'),
                _ => t('home.encUnknown'),
              },
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
          ),
        // 预览阶段就把「解压码」摆出来（2026-10-30 用户真机反馈）：
        // 以前加密包非得点「开始解压」、失败一次，才在错误页见到输入框；
        // 可引擎在预览里早就给了 encryptedCount + enc_kind，没理由不提前问。
        if (pv.encryptedCount > 0) ...[
          const SizedBox(height: 10),
          TextField(
            controller: _pwCtrl,
            obscureText: !_showPw,
            onChanged: (v) {
              final t = v.trim();
              // 不用 setState：这个字段不影响本帧画面
              _password = t.isEmpty ? null : t;
            },
            decoration: InputDecoration(
              labelText: t('home.pwForEncrypted', {'n': '${pv.encryptedCount}'}),
              hintText: t('home.pwEmptyHint'),
              isDense: true,
              border: const OutlineInputBorder(),
              prefixIcon: const Icon(Icons.key, size: 18),
              suffixIcon: IconButton(
                icon: Icon(
                  _showPw ? Icons.visibility_off : Icons.visibility,
                  size: 18,
                ),
                tooltip: _showPw ? t('home.pwHide') : t('home.pwShow'),
                onPressed: () => setState(() => _showPw = !_showPw),
              ),
            ),
          ),
          if (_recentPw.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Wrap(
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
                        _password = pw;
                        setState(() {});
                      },
                    ),
                ],
              ),
            ),
        ],
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
                    t('home.coverLabel', {
                      'size': _size((pv.cover!['size'] as num?)?.toInt() ?? 0),
                      'ext': pv.cover!['ext'] != null ? ' · ${pv.cover!['ext']}' : '',
                    }),
                    style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
                  ),
                  subtitle: Text(
                    t('home.pickMedia'),
                    style: const TextStyle(fontSize: 11),
                  ),
                ),
              CheckboxListTile(
                key: const ValueKey('want-extract'),
                dense: true,
                value: _wantExtract,
                onChanged: (v) => setState(() => _wantExtract = v ?? false),
                secondary: const Icon(Icons.folder_zip_outlined, size: 20),
                title: Text(
                  t('home.contentsTitle', {'files': '${pv.fileCount}', 'size': _size(pv.totalSize)}),
                  style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
                ),
                subtitle: Text(
                  t('home.pickFiles'),
                  style: const TextStyle(fontSize: 11),
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
              t('home.pickedCount', {'picked': '${_picked.length}', 'total': '${pv.fileCount}', 'size': _size(_pickedBytes(pv))}),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            _miniBtn(t('home.selectAll'), () => setState(() {
                  _picked
                    ..clear()
                    ..addAll(pv.entries.where((e) => !e.isDir).map((e) => e.index));
                })),
            _miniBtn(t('home.selectNone'), () => setState(() => _picked.clear())),
            const Spacer(),
            // 树 / 平铺切换：树是默认（看层级 + 按目录勾），平铺留给「只想全选」的老习惯
            _miniBtn(
              _treeView ? t('home.viewFlat') : t('home.viewTree'),
              () => setState(() => _treeView = !_treeView),
              icon: _treeView ? Icons.view_list_outlined : Icons.account_tree_outlined,
            ),
            _miniBtn(
              t('home.advancedSummary', {
                'place': _outDir == null ? t('home.nextToArchive') : t('home.viewCustom'),
                'enc': _encoding == 'auto' ? t('home.encodingAutoName') : _encoding.toUpperCase(),
              }),
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
              label: Text(t('home.cancel')),
            ),
            const SizedBox(width: 8),
            // E4「只检查」：完整解码 + 校验，一个字节都不落盘（协议 verify_only）。
            // 用 TextButton 不用 OutlinedButton：旁边那个才是主入口，
            // 两个描边按钮并排会让用户不知道点哪个（首屏「唯一最强入口」同一条规矩）。
            Tooltip(
              message: t('home.verifyOnlyHint'),
              child: TextButton.icon(
                key: const ValueKey('verify-only'),
                onPressed: () => _extract(verifyOnly: true),
                icon: const Icon(Icons.fact_check_outlined, size: 18),
                label: Text(t('home.verifyOnly')),
              ),
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
                        ? t('home.startPickedCover', {'n': '${_picked.length}'})
                        : t('home.startPicked', {'n': '${_picked.length}'}))
                    : (pv.hasCover && _wantCover ? t('home.extractCoverOnly') : t('home.mustPickSomething')),
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
                child: Text(t('home.colSize'), textAlign: TextAlign.right, style: hdr),
              ),
              if (wide)
                SizedBox(
                  width: 72,
                  child: Text(t('home.colCompressed'), textAlign: TextAlign.right, style: hdr),
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
              n.isEmptyDir ? t('home.emptyDir') : t('home.nFiles', {'n': '${n.fileCount}'}),
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
                tooltip: t('home.onlyThisLevel'),
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
    // 检验落盘阶段单独一屏：这里最要紧的一句话是「请勿拔出设备」
    if (_flushing) return _flushView();
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
            Text(t('home.extracting'), style: Theme.of(context).textTheme.titleMedium),
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
                    _current ?? t('home.preparing'),
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
                  t('home.pwTrying', {'cur': '$_pwTry', 'total': '$_pwTotal'}),
                  style: TextStyle(fontSize: 12, color: cs.primary),
                ),
              ),
            if (_stage == 'LOCATE' && _scanTotal > 0)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: Text(
                  t('home.scanning', {'done': _size(_scanDone), 'total': _size(_scanTotal)}),
                  style: TextStyle(fontSize: 12, color: cs.primary),
                ),
              ),
            // T18-4：暂停中 / 已暂停，都在这一行说清楚「现在是什么状态」
            // （按钮点下去到真停之间有间隔，不说清楚就像卡住了）
            if (_pauseRequested || _paused)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: Text(
                  _paused ? t('home.pausedNote') : t('home.pausing'),
                  style: TextStyle(fontSize: 12, color: cs.primary),
                ),
              ),
            Text(
              _stage == 'LOCATE'
                  ? t('home.step1')
                  : _stage == 'LIST'
                      ? t('home.step2')
                      : t('home.step3'),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 20),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                // T18-4：批量才给「全部暂停」—— 单个文件暂停没意义
                // （它本来就快跑完了，暂停只会让用户以为卡住）
                if (_batchOrder.length > 1) ...[
                  OutlinedButton.icon(
                    onPressed: _pauseRequested ? null : _pauseOrResume,
                    icon: Icon(_paused
                        ? Icons.play_arrow_outlined
                        : Icons.pause_circle_outline),
                    label: Text(_paused ? t('home.resumeQueue') : t('home.pauseQueue')),
                  ),
                  const SizedBox(width: 8),
                ],
                // 批量里跳过当前这一个（组 B 的 skip 语义：不是整批取消）
                if (_batchOrder.length > 1) ...[
                  OutlinedButton.icon(
                    onPressed: _skipCurrent,
                    icon: const Icon(Icons.skip_next_outlined),
                    label: Text(t('home.skipThisItem')),
                  ),
                  const SizedBox(width: 8),
                ],
                OutlinedButton.icon(
                  onPressed: _cancel,
                  icon: const Icon(Icons.stop_circle_outlined),
                  label: Text(_batchOrder.length > 1 ? t('home.cancelBatch') : t('home.cancel')),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  /// 落盘检验视图
  ///
  /// 用户原话：「只有打开了的人给他一个检验的进度条，这样有个东西就不会让他等待了」。
  /// 所以这里的进度条是**真读数**（内核还剩多少没写进盘），不是假转圈；
  /// 数字在降、秒数在涨，用户就能看出「它在干活，不是卡死」。
  Widget _flushView() {
    final cs = Theme.of(context).colorScheme;
    final frac = _flushPeak <= 0
        ? null
        : (1 - (_flushPending / _flushPeak)).clamp(0.0, 1.0);
    return Center(
      key: const ValueKey('flush'),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(t('home.flushing'), style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(
              t('home.flushExplain'),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 16),
            ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: LinearProgressIndicator(value: frac, minHeight: 10),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: Text(
                    t('home.flushRemaining', {'size': _size(_flushPending)}),
                    style: const TextStyle(fontSize: 12),
                  ),
                ),
                Text(
                  t('home.flushWaited', {'secs': '$_flushSecs'}),
                  style: const TextStyle(fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              t('home.flushTips'),
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 20),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                OutlinedButton.icon(
                  onPressed: _cancel,
                  icon: const Icon(Icons.stop_circle_outlined),
                  label: Text(t('home.stopWaiting')),
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
            _verifyMode
                ? (ok ? Icons.verified_outlined : Icons.report_problem_outlined)
                : (ok ? Icons.check_circle_outline : Icons.info_outline),
            size: 72,
            color: ok ? cs.primary : cs.tertiary,
          ),
          const SizedBox(height: 16),
          Text(
            _verifyMode
                // 别写「解开了 N 个文件」：一个字节都没写，目录也不存在
                ? (ok
                    ? t('home.verifyAllGood', {'entries': '${r.entriesDone}'})
                    : t('home.verifySomeBad', {'done': '${r.entriesDone}', 'total': '${r.entriesTotal}'}))
                : (ok
                    ? (onlyCover
                        ? t('home.coverExtracted')
                        : t('home.extractedN', {'entries': '${r.entriesDone}'}))
                    : t('home.extractedPartialN', {'done': '${r.entriesDone}', 'total': '${r.entriesTotal}'})),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            // 校验时 bytesWritten 是「解码过的字节」，不是写进硬盘的量 —— 不能拿来当产物大小
            _verifyMode
                ? t('home.timeNoWrite', {'secs': seconds})
                : t('home.timeWithSize', {'secs': seconds, 'size': _size(r.bytesWritten)}),
            style: TextStyle(color: cs.onSurfaceVariant),
          ),
          if (!_verifyMode && r.dest != null) ...[
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
                    t('home.nestedCount', {'n': '${_nestedArchives.length}'}),
                    style: TextStyle(fontSize: 12, color: cs.onSecondaryContainer),
                  ),
                  TextButton(
                    onPressed: () => _scan(_nestedArchives),
                    child: Text(t('home.continueExtract')),
                  ),
                ],
              ),
            ),
          ],
          // 封面分离结果（DESIGN §17.5）：伪装包里的原版视频/图片
          if (r.cover != null) ...[
            const SizedBox(height: 10),
            Text(
              t('home.coverSeparated', {
                'name': (r.cover!['path'] as String).split('/').last,
                'size': _size((r.cover!['size'] as num?)?.toInt() ?? 0),
              }),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            TextButton.icon(
              onPressed: () => _openFolder(r.cover!['path'] as String),
              icon: const Icon(Icons.play_circle_outline, size: 16),
              label: Text(t('home.openCover'), style: const TextStyle(fontSize: 12)),
            ),
          ],
          // 「接下来点这个」说的是产物里的启动文件 —— 校验模式没有产物，别指路
          if (!_verifyMode)
            if (_entryHint() case final hint?) ...[
            const SizedBox(height: 12),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
              decoration: BoxDecoration(
                color: cs.primaryContainer.withValues(alpha: 0.35),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Text(
                t('home.nextClickThis', {'hint': hint}),
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
              // 校验模式没有产物目录：那个 dest 是引擎顺手回显的路径，
              // 目录根本没建过，摆个「打开目录」只会点了报错。
              if (!_verifyMode && r.dest != null)
                FilledButton.icon(
                  onPressed: () => _openFolder(r.dest),
                  icon: const Icon(Icons.folder_open),
                  label: Text(t('home.openDir')),
                ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: () {
                  // 分享文案（调研 07:952-954）：资源圈的真实动作是「把方法告诉对方」
                  final name = (_file ?? '').split('/').last;
                  final pw = (_password == null || _password!.isEmpty)
                      ? ''
                      : t('home.pwLine', {'pw': '$_password'});
                  Clipboard.setData(ClipboardData(
                    text: t('home.copyHowto', {'name': name, 'pw': pw}),
                  ));
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text(t('home.copiedSend'))),
                  );
                },
                icon: const Icon(Icons.copy_all_outlined, size: 18),
                label: Text(t('home.copyHowtoBtn')),
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
                label: Text(t('home.againOne')),
              ),
            ],
          ),
        ],
      ),
    );
  }

  /// S5：空间不足卡片 —— 三个数 + 三条出路
  ///
  /// 三条出路都必须有**真实后端行为**，不做假开关：
  /// 换位置 = 改 `_outDir` 重解；只解压部分 = 回预览树勾选；继续试试 = 协议 `extract.force`。
  List<Widget> _spaceShortageBlock(ColorScheme cs) {
    return [
      const SizedBox(height: 14),
      Container(
        key: const ValueKey('space-shortage'),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: cs.errorContainer.withValues(alpha: 0.35),
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: cs.error.withValues(alpha: 0.25)),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              t('home.noSpace'),
              style: TextStyle(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: cs.error,
              ),
            ),
            const SizedBox(height: 4),
            Text(_spaceLine(), style: const TextStyle(fontSize: 12, height: 1.5)),
          ],
        ),
      ),
      const SizedBox(height: 10),
      Wrap(
        spacing: 8,
        runSpacing: 8,
        alignment: WrapAlignment.center,
        children: [
          OutlinedButton.icon(
            key: const ValueKey('space-change-dest'),
            onPressed: _changeDestAndRetry,
            icon: const Icon(Icons.folder_open, size: 18),
            label: Text(t('home.changeLocation')),
          ),
          if (_preview != null)
            OutlinedButton.icon(
              key: const ValueKey('space-pick-part'),
              onPressed: () => setState(() => _phase = Phase.preview),
              icon: const Icon(Icons.checklist, size: 18),
              label: Text(t('home.extractPartialBtn')),
            ),
          FilledButton.icon(
            key: const ValueKey('space-force'),
            onPressed: () => _extract(force: true),
            icon: const Icon(Icons.play_arrow, size: 18),
            label: Text(t('home.tryAnyway')),
          ),
        ],
      ),
      const SizedBox(height: 4),
      Text(
        t('home.tryAnywayHint'),
        textAlign: TextAlign.center,
        style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
      ),
    ];
  }

  /// 空间不足的三个人话数字
  ///
  /// 引擎在错误的 `detail` 里给 `need` / `available`（字节数，协议里可能是字符串）。
  /// 拿不到就退成一句通用建议 —— **不给算错的数字**（CLI 侧同一口径，别一边猜一边不猜）。
  String _spaceLine() {
    int? pick(String k) {
      final v = _errorDetail?[k];
      if (v is num) return v.toInt();
      if (v is String) return int.tryParse(v);
      return null;
    }

    final need = pick('need');
    final avail = pick('available');
    if (need == null || avail == null) {
      return t('home.freeSpaceHint');
    }
    final short = need > avail ? need - avail : 0;
    return t('home.noSpaceDetail', {'need': _size(need), 'avail': _size(avail), 'short': _size(short)});
  }

  /// 换个位置再解一次（S5 第一个按钮）
  Future<void> _changeDestAndRetry() async {
    final d = await getDirectoryPath(confirmButtonText: t('home.outHere'));
    if (d == null || !mounted) return;
    setState(() => _outDir = d);
    await _extract();
  }

  Widget _errorView() {
    final cs = Theme.of(context).colorScheme;
    // 分卷中间块本来就没有文件头，探测结论必然是"认不出" —— 说出来只会误导。
    // 空间不足同理：包本身好好的，说一句「认不出的文件」会让用户以为包坏了、
    // 跑去重新下载（2026-10-30 真机截图里就是这么显示的）。
    final hint = (_errorCode == 'MISSING_VOLUME' || _errorCode == 'NOT_ENOUGH_SPACE')
        ? null
        : _typeHint();
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
                          ? t('home.missingPart', {'name': '${_errorDetail!['missing']}'})
                          : t('home.volumesIncomplete'),
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: cs.error,
                      ),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      (_errorDetail?['found'] as String?) != null
                          ? t('home.volumesFound', {'n': '${_errorDetail!['found']}'})
                          : t('home.volumesMissing'),
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
                label: Text(t('home.rescueBtn')),
              ),
              const SizedBox(height: 4),
              Text(
                t('home.rescueDesc'),
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
                label: Text(t('home.diagPack'), style: const TextStyle(fontSize: 12)),
              ),
            if ((_errorNote ?? '').isNotEmpty)
              TextButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: _errorNote!));
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text(t('home.copiedDetail'))),
                  );
                },
                icon: const Icon(Icons.copy, size: 15),
                label: Text(t('home.copyDetail'), style: const TextStyle(fontSize: 12)),
              ),
            // S5：空间不够 —— 三个数 + 三条出路。
            //
            // 以前这里是死路：引擎连 `SPACE_SHORTAGE` 事件都不发（全库只有定义、零发射点），
            // 界面那段处理代码永远收不到；而 `--force` 界面也一个参数都没传。
            // 现在引擎发事件 + 错误 detail 带 need/available，三个按钮各有真实后端行为。
            if (_errorCode == 'NOT_ENOUGH_SPACE') ..._spaceShortageBlock(cs),
            if (_needsPassword) ...[
              const SizedBox(height: 18),
              // 密码本里的常用解压码：点一下直接填入并重试（调研 07:874-878）
              if (_recentPw.isNotEmpty) ...[
                Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    t('home.commonPw'),
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
                  labelText: t('home.pwLabel'),
                  hintText: t('home.pwHint'),
                  border: const OutlineInputBorder(),
                  isDense: true,
                  prefixIcon: const Icon(Icons.key),
                  // 眼睛：点一下看明文，再点回密文（打错一个字符不用整段重输）
                  suffixIcon: IconButton(
                    icon: Icon(
                      _showPw ? Icons.visibility_off : Icons.visibility,
                      size: 18,
                    ),
                    tooltip: _showPw ? t('home.pwHide') : t('home.pwShow'),
                    onPressed: () => setState(() => _showPw = !_showPw),
                  ),
                ),
              ),
              const SizedBox(height: 10),
              FilledButton.icon(
                onPressed: _retryWithPassword,
                icon: const Icon(Icons.lock_open),
                label: Text(t('home.retryWithPw')),
              ),
            ],
            const SizedBox(height: 24),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                FilledButton.icon(
                  onPressed: _pick,
                  icon: const Icon(Icons.folder_open),
                  label: Text(t('home.changeFile')),
                ),
                const SizedBox(width: 12),
                OutlinedButton(
                  onPressed: () => setState(() => _phase = Phase.empty),
                  child: Text(t('home.back')),
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
    if (files.isEmpty) return t('home.noFilesInArchive');
    if (files.length == 1) return t('home.willExtractOne');
    final roots = files.map((e) => e.path.split('/').first).toSet();
    final allNested = files.every((e) => e.path.contains('/'));
    if (roots.length == 1 && allNested) {
      return t('home.willExtractInto', {'root': roots.first});
    }
    return t('home.willCreateFolder', {'n': '${files.length}'});
  }

  /// 读密码本，给密码框做「常用解压码」chips（只读，不回写）
  ///
  /// 路径与引擎一致：`$XDG_CONFIG_HOME/yshjy/passwords.txt`（回退 `~/.config`）。
  Future<void> _loadRecentPasswords() async {
    try {
      // 路径走 platform_paths 的唯一真源（与引擎 config_dir 一致，W9）
      final f = File('$configDir${Platform.pathSeparator}passwords.txt');
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
    // 暂停时不报速度：用户真机验收抓到暂停画面上还挂着「1.04 GB/s · 剩 0 秒」——
    // 停都停了还显示速度，看着像没停住。恢复后下一帧进度事件会把速度算回来。
    if (_paused) return '';
    if (_speed <= 0 || p.totalBytes <= 0) return '';
    final left = p.totalBytes - p.doneBytes;
    if (left <= 0) return '';
    final sec = (left / _speed).round();
    final eta = sec >= 60
        ? t('home.durMinSec', {'min': '${sec ~/ 60}', 'sec': '${sec % 60}'})
        : t('home.durSec', {'sec': '$sec'});
    return t('home.speedEta', {'speed': _size(_speed.round()), 'eta': eta});
  }

  String _size(int bytes) => formatBytes(bytes);

  String _shortPath(String p) {
    final parts = p.split('/').where((s) => s.isNotEmpty).toList();
    if (parts.length <= 2) return p;
    return '…/${parts.sublist(parts.length - 2).join('/')}';
  }
}
