//! 引擎客户端 —— 与 `yshjy --serve` 说话的那一层
//!
//! 设计原则（见仓库 docs/PROTOCOL.md）：
//! - 界面**只认协议**，不认引擎实现。引擎怎么改，只要协议不动，这里就不用动。
//! - 事件按 `kind` 分支；**不认识的 kind 一律忽略**（后端加新事件不会弄坏旧界面）。
//! - 错误只拿 `code` 查文案；`note` 只用于日志/反馈。
//!
//! 这个文件同时是「Flutter 怎么接 Rust 引擎」的参考实现：
//! 默认传输是 `Process.start` + 逐行 JSON（桌面/Android）；
//! iOS 换成 `FfiTransport`（同一个 `EngineTransport` 接口，见 docs/FFI.md）。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'transport/engine_transport.dart';
import 'l10n.dart';

// ───────────────────────── 数据模型 ─────────────────────────

/// 归档里的一个条目（对应 Rust 的 `EntryView`）
class EntryItem {
  final int index;
  final String path;
  final int size;

  /// 压缩后大小（引擎 `EntryView.packed`；未知则 null）。
  /// 树视图里用它标「压不动」的文件（PeaZip 默认展示这一列，调研 07:632）。
  final int? packed;

  final bool isDir;
  final bool encrypted;
  final bool risky;
  final int? modified;

  EntryItem({
    required this.index,
    required this.path,
    required this.size,
    required this.isDir,
    required this.encrypted,
    required this.risky,
    this.packed,
    this.modified,
  });

  factory EntryItem.fromJson(Map<String, dynamic> j) => EntryItem(
        index: (j['index'] as num?)?.toInt() ?? 0,
        path: j['path'] as String? ?? '',
        size: (j['size'] as num?)?.toInt() ?? 0,
        packed: (j['packed'] as num?)?.toInt(),
        isDir: j['is_dir'] == true,
        encrypted: j['encrypted'] == true,
        risky: j['is_risky'] == true,
        modified: (j['modified'] as num?)?.toInt(),
      );

  /// 只显示最后一段（列表里没必要把整条路径铺满）
  String get name => path.split('/').where((s) => s.isNotEmpty).last;
}

/// 预览结果
class Preview {
  final String format;
  final bool complete;
  final int totalSize;
  final int encryptedCount;
  /// 加密方式：zipcrypto / aes / aes128 / aes192 / aes256 / unknown（null = 没加密）
  final String? encKind;
  final List<EntryItem> entries;
  final List<String> hints;

  /// S10 多候选：这个文件里像是有几处藏着东西（`list` 里 `candidates`；正常包为空）
  ///
  /// 预览页看到的是「引擎默认那处」的内容 —— 用户发现解错了，就让他在这些候选中换一处。
  final List<ArchiveCandidate> candidates;

  /// 伪装封面（`list` 结果里的 `cover`）：`{size, kind, ext}`；普通压缩包为 null
  ///
  /// 用户在验收里要求「封面和压缩包内容上下并列、各自带勾选」（GUI-PLAN §5.2①），
  /// 界面要靠这个字段决定要不要显示「伪装封面」那一行。
  final Map<String, dynamic>? cover;

  Preview({
    required this.format,
    required this.complete,
    required this.totalSize,
    required this.encryptedCount,
    required this.encKind,
    required this.entries,
    required this.hints,
    this.cover,
    this.candidates = const [],
  });

  /// 一处文件里有好几处藏着东西（才值得给用户看选择入口）
  bool get hasManyCandidates => candidates.length >= 2;

  /// 有封面区（= 这是个伪装包）
  bool get hasCover => cover != null;

  factory Preview.fromJson(Map<String, dynamic> j) => Preview(
        format: j['format'] as String? ?? '?',
        complete: j['complete'] != false,
        totalSize: (j['total_size'] as num?)?.toInt() ?? 0,
        encryptedCount: (j['encrypted'] as num?)?.toInt() ?? 0,
        encKind: j['enc_kind'] as String?,
        entries: (j['entries'] as List? ?? [])
            .map((e) => EntryItem.fromJson((e as Map).cast<String, dynamic>()))
            .toList(),
        hints: (j['hints'] as List? ?? []).map((e) => e.toString()).toList(),
        cover: (j['cover'] as Map?)?.cast<String, dynamic>(),
        candidates: (j['candidates'] is List)
            ? (j['candidates'] as List)
                .whereType<Map>()
                .map((m) => ArchiveCandidate.fromJson(m.cast<String, dynamic>()))
                .toList()
            : const [],
      );

  int get fileCount => entries.where((e) => !e.isDir).length;
}

/// 进度（对应 Rust 的 `ProgressView`；total 为 0 表示"还不知道"）
class Progress {
  final int doneBytes;
  final int totalBytes;
  final int doneFiles;
  final int totalFiles;

  Progress({
    required this.doneBytes,
    required this.totalBytes,
    required this.doneFiles,
    required this.totalFiles,
  });

  factory Progress.fromJson(Map<String, dynamic> j) => Progress(
        doneBytes: (j['done_bytes'] as num?)?.toInt() ?? 0,
        totalBytes: (j['total_bytes'] as num?)?.toInt() ?? 0,
        doneFiles: (j['done_files'] as num?)?.toInt() ?? 0,
        totalFiles: (j['total_files'] as num?)?.toInt() ?? 0,
      );

  double? get fraction =>
      totalBytes == 0 ? null : (doneBytes / totalBytes).clamp(0.0, 1.0);
}

/// 引擎事件（kind 是稳定标识，data 是内部结构）
class EngineEvent {
  final String kind;
  final Map<String, dynamic> data;

  EngineEvent(this.kind, this.data);

  factory EngineEvent.fromJson(Map<String, dynamic> j) {
    final ev = (j['event'] as Map?)?.cast<String, dynamic>() ?? const {};
    // Rust 的 Event 是 externally tagged：{"ExtractProgress": {...}}
    // 有些变体（如 StageStarted { stage: "Locate" }）的值是字符串，那就整个当 data
    final first = ev.isEmpty ? null : ev.values.first;
    final data = first is Map
        ? first.cast<String, dynamic>()
        : ev;
    return EngineEvent(j['kind'] as String? ?? '', data);
  }

  Progress? get progress => data['progress'] is Map
      ? Progress.fromJson((data['progress'] as Map).cast<String, dynamic>())
      : null;

  String? get current => data['current'] as String?;
}

/// 一次解压/打包的结果
class ExtractResult {
  final String outcome;
  final int entriesDone;
  final int entriesTotal;
  final int bytesWritten;
  final String? dest;
  final int millis;
  /// 分离出来的封面（DESIGN §17.5）：{path, size, kind}；没有则为 null
  final Map<String, dynamic>? cover;

  ExtractResult({
    required this.outcome,
    required this.entriesDone,
    required this.entriesTotal,
    required this.bytesWritten,
    required this.dest,
    required this.millis,
    this.cover,
  });

  factory ExtractResult.fromJson(Map<String, dynamic> j) => ExtractResult(
        outcome: j['outcome']?.toString() ?? 'Failed',
        entriesDone: (j['entries_done'] as num?)?.toInt() ?? 0,
        entriesTotal: (j['entries_total'] as num?)?.toInt() ?? 0,
        bytesWritten: (j['bytes_written'] as num?)?.toInt() ?? 0,
        dest: j['dest'] as String?,
        millis: (j['millis'] as num?)?.toInt() ?? 0,
        cover: (j['cover'] as Map?)?.cast<String, dynamic>(),
      );

  bool get ok => outcome.startsWith('Success');
}

/// 协议错误（只有码 + 技术备注；文案由界面查语言包）
class EngineError implements Exception {
  final String code;
  final String note;

  /// 引擎附带的结构化参数（`looks_like` / `ext_mismatch` / `size` …）
  ///
  /// 有了它界面才能说"你给的是个 MP4 视频"，而不是干巴巴一句"出错"。
  /// 这是协议 v1 的兼容扩展：老引擎不给这个字段，界面按空表处理即可。
  final Map<String, dynamic> detail;

  EngineError(this.code, this.note, [Map<String, dynamic>? detail])
      : detail = detail ?? const {};

  /// 引擎探测到的真实类型（`mp4` / `png` / `elf` / `zip` …）
  String? get looksLike => detail['looks_like'] as String?;

  /// 文件扩展名（小写）
  String? get ext => detail['ext'] as String?;

  /// 名字和内容对不上？（伪装包的典型特征）
  bool get extMismatch => detail['ext_mismatch'] == 'true';

  /// 文件大小（字节）
  int? get size => int.tryParse(detail['size']?.toString() ?? '');

  /// 真实内容的大类：archive / media / document / program / unknown
  String get looksKind => (detail['looks_kind'] as String?) ?? 'unknown';

  @override
  String toString() => 'EngineError($code): $note';
}

// ───────────────────────── 客户端 ─────────────────────────

class EngineClient {
  /// 全局单例：整个应用只有**一个**引擎进程（多页面共用，事件流广播给所有订阅者）
  static final EngineClient instance = EngineClient();

  EngineTransport? _proc;
  final _events = StreamController<EngineEvent>.broadcast();

  /// 传输工厂：默认起子进程。iOS / 测试可以换成别的实现（DESIGN §18.1）。
  static EngineTransportFactory transportFactory =
      (exe, args) => ProcessTransport(exe, args);
  final Map<int, Completer<Map<String, dynamic>>> _pending = {};
  int _nextId = 1;
  bool _started = false;

  /// `hello` 的进程内缓存（引擎的能力表在一次运行里不会变；见 `hello()` 的注释）
  Map<String, dynamic>? _helloCache;

  /// 正在飞的那次 `hello` —— 三个并发调用共享它，避免同时打向还没预热的引擎
  Future<Map<String, dynamic>>? _helloInflight;

  /// 正在启动中的那次 `start()`。
  ///
  /// 为什么需要它：`await Process.start` 挂起期间，第二个调用看到的 `_started`
  /// 还是 false，于是会**再起一个引擎进程**（实测界面启动后同时存在两个
  /// `yshjy --serve`，各持一条管道）。缓存 Future 让并发调用共用同一次启动。
  Future<void>? _starting;

  /// 引擎路径（按优先级查找）
  ///
  /// 1. 环境变量 `YSHJY_BIN`（开发时指到 target/release/yshjy）
  /// 2. **跟界面放在同一个目录的 `yshjy`** —— 发布包就是这样摆的，
  ///    用户双击就能用，不需要装 PATH
  /// 3. PATH 里的 yshjy（系统安装版）
  static String get executable {
    final env = Platform.environment['YSHJY_BIN'];
    if (env != null && env.isNotEmpty) return env;
    // Windows 的引擎是 yshjy.exe（发布包里引擎跟界面放同一目录，W9）
    final name = Platform.isWindows ? 'yshjy.exe' : 'yshjy';
    try {
      final beside = File(Platform.resolvedExecutable).parent.path + Platform.pathSeparator + name;
      if (File(beside).existsSync()) return beside;
    } catch (_) {}
    return name;
  }

  Stream<EngineEvent> get events => _events.stream;

  /// 是否有正在跑的请求（关闭窗口时用来决定要不要先问一句）
  bool get busy => _pending.isNotEmpty;

  /// 起引擎（幂等）。并发的多次调用共用同一个 Future —— 只会起一个进程；
  /// 启动失败时清掉缓存，允许下次重试。
  Future<void> start() => _starting ??= _doStart();

  Future<void> _doStart() async {
    if (_started) return;
    try {
      final t = transportFactory(executable, const ['--serve']);
      _proc = t;
      await t.start();
      t.stdoutLines.listen(_onLine, onDone: _onClosed);
      // 引擎的 stderr 只有崩溃/调试信息，不打扰用户，但留着便于排查
      t.stderrLines.listen((l) => debugPrint('[engine] $l'));
      _started = true;
    } catch (_) {
      _proc = null;
      _starting = null;
      rethrow;
    }
  }

  void _onLine(String line) {
    if (line.trim().isEmpty) return;
    Map<String, dynamic> v;
    try {
      v = (jsonDecode(line) as Map).cast<String, dynamic>();
    } catch (_) {
      return; // 半行/坏行：忽略，绝不因此崩界面
    }
    if (v['type'] == 'event') {
      _events.add(EngineEvent.fromJson(v));
      return;
    }
    final id = v['id'];
    if (id is int) {
      _pending.remove(id)?.complete(v);
    }
  }

  void _onClosed() {
    // 引擎没了（崩溃/被关）：把所有等待中的请求一次性失败掉，别让界面永远转圈
    for (final c in _pending.values) {
      if (!c.isCompleted) {
        c.complete({
          'ok': false,
          'error': {'code': 'ENGINE_EXITED', 'note': t('engine.noteExited')},
        });
      }
    }
    _pending.clear();
    _started = false;
    _starting = null; // 否则下次 start() 会拿到「已完成的旧 Future」，永远重起不了
  }

  /// 发一条命令，等它的响应。
  ///
  /// `timeout` 只防"永远不返回"；解压大包可以很久，所以默认给很长。
  Future<Map<String, dynamic>> _call(
    Map<String, dynamic> cmd, {
    Duration timeout = const Duration(hours: 6),
  }) async {
    await start();
    final id = _nextId++;
    final c = Completer<Map<String, dynamic>>();
    _pending[id] = c;
    final payload = jsonEncode({...cmd, 'id': id});
    await _proc!.writeLine(payload);
    return c.future.timeout(
      timeout,
      onTimeout: () => {
        'ok': false,
        'error': {'code': 'TIMEOUT', 'note': t('engine.noteTimeout')},
      },
    );
  }

  /// 把「失败响应」统一抛成 EngineError，调用方只处理成功分支
  static Map<String, dynamic> _unwrap(Map<String, dynamic> r) {
    if (r['ok'] == true) return (r['result'] as Map).cast<String, dynamic>();
    final e = (r['error'] as Map?)?.cast<String, dynamic>() ?? const {};
    throw EngineError(
      e['code'] as String? ?? 'UNKNOWN',
      e['note'] as String? ?? '',
      (e['detail'] as Map?)?.cast<String, dynamic>(),
    );
  }

  /// 握手（`hello`）。**同一个进程里只发一次**，并发调用共享同一个 Future。
  ///
  /// 为什么要合并（2026-11-01 真机撞的）：
  /// 应用一启动，`AppShell`、`SettingsPage`、设置页的格式表**各自**都会要 hello，
  /// 三个请求同时打向一个**刚刚 fork 出来、还没预热**的引擎进程。
  /// 真机上（release AOT + 机械盘冷启动）它们中间的一个会失败 —— 症状就是
  /// `hello.disk` 拿到空，设置页显示「本机磁盘类型没探到」（用户在截图里点出来的），
  /// 而状态栏和格式表却是好的（它们重试成功了）。
  /// 而且引擎侧的能力表在**一次运行里不会变**，本来就没必要问三遍。
  ///
  /// 失败**不缓存**：下次调用会重新发一次（这样"第一次冷启动失败"能自愈）。
  /// 要强制重问（引擎升级后刷新能力表）就传 `refresh: true`。
  Future<Map<String, dynamic>> hello({bool refresh = false}) {
    if (!refresh && _helloCache != null) return Future.value(_helloCache!);
    final inflight = _helloInflight;
    if (inflight != null) return inflight;
    final f = _doHello();
    _helloInflight = f;
    return f;
  }

  Future<Map<String, dynamic>> _doHello() async {
    try {
      final r = _unwrap(await _call({'cmd': 'hello'}));
      _helloCache = r;
      return r;
    } finally {
      // 失败也要清掉，否则后面所有调用都会拿到这个已失败的 Future
      _helloInflight = null;
    }
  }

  Future<Preview> list(String path, {String? password}) async {
    final r = await _call({
      'cmd': 'list',
      'path': path,
      if (password != null) 'password': password,
    });
    return Preview.fromJson(_unwrap(r));
  }

  /// S10 多候选：一个文件里「像归档」的一处（`scan` 的 `candidates[]`）
  ///
  /// 只有 `length >= 2` 时界面才该显示选择界面；正常包永远只有一条（`chosen`）。
  static List<ArchiveCandidate> candidatesOf(Map<String, dynamic> item) {
    final raw = item['candidates'];
    if (raw is! List) return const [];
    return raw
        .whereType<Map>()
        .map((m) => ArchiveCandidate.fromJson(m.cast<String, dynamic>()))
        .toList();
  }

  Future<ExtractResult> extract(
    String path, {
    String? dest,
    String? password,
    bool cover = true,
    bool wantExtract = true,
    bool drill = false,
    bool ask = true,
    /// 文件级挑选（P23-1）：只解这些序号（来自 list 的 entries.index）；null/空 = 全解
    List<int>? select,
    /// 文件名编码（P23-3）：gbk / big5 / sjis / utf-8；null 或 'auto' = 引擎自动链
    String? encoding,
    /// 跳过磁盘空间预检（协议 `extract.force`，CLI 的 `--force`）
    ///
    /// 引擎和协议**早就支持**了，界面以前一个参数都没传 —— 于是「空间差一点」在界面上
    /// 是死路（S5 的第三个按钮「继续试试」就是它）。
    bool force = false,
    /// S10 多候选：只解**这段偏移**处的那一份（值来自 `scan` 的 `candidates[].offset`）
    ///
    /// 用户点「换个地方试」时才会传。引擎侧 `locate_at_base` 精确切那一段；
    /// 传了但那段解不开，引擎报 `NOT_ARCHIVE`（不会偷偷回头解默认那处）。
    int? candidate,
    /// 只检查不落盘（协议 `extract.verify_only`，CLI 的 `--test`）
    ///
    /// 引擎侧**早就支持**：批处理和三套解压器每条落盘路径都判了 `verify_only`，
    /// 连 `PartFile::create_with` 都带着它，`--test` 走的就是这条路 —— 只是界面
    /// 以前一个字都没传，于是「这包坏没坏」只能真解一遍、把硬盘占满（T7 / E4）。
    ///
    /// ⚠️ 用它的调用方必须同时把 `cover` 关掉：提取封面是真写盘的另一条路径，
    /// 一起开着就与「一个字节都不写」矛盾（实测 CLI `--test` 是零字节）。
    bool verifyOnly = false,
    /// 智能解压（协议 `extract.smart_dir`，CLI 默认开 / `--no-smart-dir` 关）
    ///
    /// `true`：包里只有一个顶层目录 / 只有一个文件时不额外套层，一堆散文件才建
    /// 同名文件夹；`false`：**永远**多套一层「包名」文件夹。
    ///
    /// ⚠️ 只有在**不给 dest** 时才看得出区别 —— 给了 dest 两种模式都把产物直接
    /// 铺在 dest 下（历史行为）。界面里选了输出目录时这个开关必须禁用并说明。
    bool? smartDir,
  }) async {
    final r = await _call({
      'cmd': 'extract',
      'path': path,
      if (dest != null) 'dest': dest,
      if (password != null) 'password': password,
      // GUI-PLAN §5.2①：封面和内容是两件事，各自带勾选
      'cover': cover,
      'extract': wantExtract,
      if (drill) 'drill': true,
      // 文件级挑选（P23-1）：协议认正整数数组，序号与列表里的 index 一致
      if (select != null && select.isNotEmpty) 'select': select,
      // 文件名编码（P23-3）：「自动」不传，交给引擎的自动链
      if (encoding != null && encoding.isNotEmpty && encoding != 'auto')
        'encoding': encoding,
      // 空间不够但用户要硬试（S5「继续试试」）
      if (force) 'force': true,
      // 指定候选那一段（S10「换个地方试」）
      if (candidate != null) 'candidate': candidate,
      // 声明「我能处理 ASK_USER」—— 不写的话引擎遇到限额直接失败（协议设计如此）
      if (ask) 'ask': true,
      // 只检查不落盘（E4）：引擎走 verify_only，解码 + 校验但一个字节都不写
      if (verifyOnly) 'verify_only': true,
      // 智能解压（E5）：不传 = 引擎默认 true；只有「没给 dest」时才真有区别
      if (smartDir != null) 'smart_dir': smartDir,
    });
    return ExtractResult.fromJson(_unwrap(r));
  }

  /// 打包（`mode: plain` = 普通 ZIP；`disguise` = 伪装成用户选的文件）
  ///
  /// 普通压缩是默认语义（GUI-PLAN §5.2②）；伪装是特殊功能，要显式选。
  /// `cover` = 用户挑的外壳文件路径（任意类型）。伪装模式下必填；
  /// 引擎会按外壳的文件头决定布局（MP4 走 free 原子，其他走尾部追加）。
  Future<Map<String, dynamic>> pack({
    required List<String> inputs,
    required String out,
    String? password,
    int? splitBytes,
    bool checksum = true,
    bool readme = true,
    String mode = 'disguise',
    /// 归档格式（T17-4 / 协议 `pack.format`）：'zip' / 'tar.gz' / 'tar.zst'
    ///
    /// 只在**普通压缩**（`mode: 'plain'`）下才传 —— 伪装包只能是 zip（引擎会拒，
    /// 见 `pack::PackFormat` 的探针注释）。null = 不传，引擎默认 zip。
    String? format,
    String? cover,
    /// 自解压种类（P23-5）：'windows'（exe）/ 'linux'（.sh）；null = 不自解压
    String? sfxKind,
    /// 压缩级别（P23-4）：0 = 仅存储最快 / 6 = 标准 / 9 = 最小体积
    int? level,
    /// 干扰数据（E1 / 协议 `pack.decoy`）：往里撒 N 段乱码，让网盘/在线扫描
    /// 以为这是普通视频；null 或 0 = 不打。
    ///
    /// ⚠️ 与 `mode:"plain"`（普通 ZIP）和自解压互斥 —— 引擎会回 `BAD_PARAM`，
    /// 不是静默忽略。所以调用方只在伪装模式且非自解压时才传。
    int? decoy,
    /// 每段字节数（协议 `pack.decoy_size`，引擎允许 4K–1M，默认 16K）
    int? decoySize,
    /// 每段里夹一枚假特征码（协议 `pack.decoy_signatures`，默认关）
    bool decoySignatures = false,
    /// 防炸链（T18-3 / 协议 `pack.tweak_time`）：抖条目时间戳，让网盘判不成同一个文件
    bool tweakTime = false,
    /// 数据藏在哪（T18-1 · 协议 `pack.layout`，也认老写法 `pack.tail`）
    ///
    /// - `null`：老行为，交给引擎按封面文件头决定（MP4 走 free 原子，图片/文本尾部追加）
    /// - `'free'`：藏进封面的 `free` 原子（推荐：视频能播，8 种工具全通）
    /// - `'tail'`：封面一个字节都不改，数据直接接在文件末尾（最保险）
    /// - `'mid'`：插进视频数据中间（L4）—— 视频照常能播，包藏得比尾巴深；
    ///   代价是 EOCD 离文件尾远，别的解压软件多半认不出（7-Zip 和本工具一定行）
    /// - `'head'`：放进文件头部（L3）—— 代价比 mid 更狠，unzip / 资源管理器全找不到
    /// - `'two'`：一个文件里放**两个包**（L7）—— 封面 + 本次输入 + `second` 指定的现成 zip；
    ///   通用解压器只看得见其中一个（7-Zip 看第一个、unzip 看最后一个）
    String? layout,
    /// L7 的第二个包（协议 `pack.second`）：一个现成 ZIP 的绝对路径。
    /// 只有 `layout == 'two'` 才有意义 —— 给别的布局会被引擎拒（那边有人话说明）。
    String? second,
  }) async {
    final r = await _call({
      'cmd': 'pack',
      'inputs': inputs,
      'out': out,
      'mode': mode,
      if (format != null) 'format': format,
      if (password != null) 'password': password,
      if (cover != null) 'cover': cover,
      if (sfxKind != null) 'sfx': sfxKind,
      if (splitBytes != null) 'split': splitBytes,
      if (level != null) 'level': level,
      // 干扰数据（E1）：段数 > 0 才传，0 等于不打
      if (decoy != null && decoy > 0) 'decoy': decoy,
      if (decoySize != null) 'decoy_size': decoySize,
      if (decoySignatures) 'decoy_signatures': true,
      // 数据藏在哪（E2 的 tail 是这里的一个档；T18-1 又加了 mid / head / two）
      if (layout != null) 'layout': layout,
      // L7 的第二个包：只在选了「两个包」时传
      if (second != null && second.isNotEmpty) 'second': second,
      'checksum': checksum,
      // 防炸链（T18-3）：默认关，开着才传
      if (tweakTime) 'tweak_time': true,
      'readme': readme,
    }, timeout: const Duration(hours: 2));
    return _unwrap(r);
  }

  /// 扫一批路径（目录会自动展开、分卷自动归并）——只读文件头，不解压
  Future<Map<String, dynamic>> scan(List<String> paths) async => _unwrap(
        await _call({'cmd': 'scan', 'paths': paths},
            timeout: const Duration(minutes: 3)),
      );

  /// 批量解压：引擎内部逐个跑，一个坏了不影响其余（S13）
  ///
  /// `ask:true` 让限额命中时走 ASK_USER 询问（否则整批直接失败）；
  /// 批量里选「跳过」只跳过当前这一项（组 B 的 `skip` 语义）。
  Future<Map<String, dynamic>> batch({
    required List<String> paths,
    String? dest,
    String? password,
    /// 文件名编码（P23-3）：整批用同一个代码页
    String? encoding,
  }) async =>
      _unwrap(await _call({
        'cmd': 'batch',
        'paths': paths,
        if (dest != null) 'dest': dest,
        if (password != null) 'password': password,
        if (encoding != null && encoding.isNotEmpty && encoding != 'auto')
          'encoding': encoding,
        'ask': true,
      }, timeout: const Duration(hours: 6)));

  /// 回答引擎的 ASK_USER：continue / skip / cancel
  Future<Map<String, dynamic>> answer(String value) async =>
      _unwrap(await _call({'cmd': 'answer', 'value': value},
          timeout: const Duration(seconds: 10)));

  /// 批量里只跳过当前这一项，下一项继续（不是整批取消）
  Future<Map<String, dynamic>> skipCurrent() async =>
      _unwrap(await _call({'cmd': 'skip'}, timeout: const Duration(seconds: 10)));

  /// 生成诊断包（报障用）。默认**路径脱敏**，方便直接贴到群里。
  ///
  /// `outDir`（E6 / 协议 `report.out_dir`）：存到哪个目录。**不传时引擎落在进程当前
  /// 目录** —— 对 GUI 来说就是「从哪启动就落在哪」（可能是 `/` 或 `~`），用户回头
  /// 根本找不着。所以界面一律传一个好找的地方（见 `platform_paths.downloadsDir`）。
  Future<Map<String, dynamic>> report({String? path, String? outDir}) async {
    final r = await _call({
      'cmd': 'report',
      if (path != null) 'path': path,
      if (outDir != null) 'out_dir': outDir,
      'no_paths': true,
    }, timeout: const Duration(minutes: 2));
    return _unwrap(r);
  }

  /// 救援：索引（中央目录）坏了时按文件头抢救（协议 `rescue`，与 CLI `--rescue` 同一条路）
  ///
  /// - `listOnly: true` 只重建清单不落盘 —— 想先说「能救回 N 个」再动手就用它
  /// - 不给 `dest` 时引擎落到 `<包名>.rescue`（与 CLI 一致）
  ///
  /// 超时给到 30 分钟：救援要扫全文件，网盘挂载点上慢得多。
  Future<Map<String, dynamic>> rescue(
    String path, {
    String? dest,
    bool listOnly = false,
    String? encoding,
  }) async =>
      _unwrap(await _call({
        'cmd': 'rescue',
        'path': path,
        if (dest != null) 'dest': dest,
        'list_only': listOnly,
        if (encoding != null && encoding.isNotEmpty && encoding != 'auto')
          'encoding': encoding,
      }, timeout: const Duration(minutes: 30)));

  Future<void> cancel() async {
    try {
      await _call({'cmd': 'cancel'}, timeout: const Duration(seconds: 10));
    } catch (_) {
      // 取消失败不该再抛错给用户 —— 它本来就是"尽量停下"
    }
  }

  /// 只取消队列里的某一行（D5）：其余任务照跑
  ///
  /// 返回 `{"cancelled_job": n, "running": bool}`：
  /// - `running: true` —— 它正在跑，已经真的被打断
  /// - `running: false` —— 它还在排队，引擎轮到它时会直接标记跳过
  Future<Map<String, dynamic>> cancelJob(int job) async {
    try {
      return _unwrap(await _call(
        {'cmd': 'cancel', 'job': job},
        timeout: const Duration(seconds: 10),
      ));
    } catch (_) {
      return const {'cancelled_job': 0, 'running': false};
    }
  }

  /// 队列「全部暂停」（T18-4）
  ///
  /// 返回 `{"paused": bool, "delivered": bool}`：
  /// - `delivered: false` —— 当时没有任务在跑（界面该把按钮复位，别假装暂停成功）
  /// - `delivered: true` **只代表请求送到了**：引擎要等当前这个文件解完才真的停，
  ///   那时会发 `PAUSED` 事件 —— 界面必须按事件翻按钮，否则就是「点了没反应」
  Future<Map<String, dynamic>> pauseQueue() async {
    try {
      return _unwrap(await _call({'cmd': 'pause'}, timeout: const Duration(seconds: 10)));
    } catch (_) {
      return const {'paused': false, 'delivered': false};
    }
  }

  /// 队列「继续」（T18-4）：从暂停处接着跑，不重解已经完成的条目
  Future<Map<String, dynamic>> resumeQueue() async {
    try {
      return _unwrap(await _call({'cmd': 'resume'}, timeout: const Duration(seconds: 10)));
    } catch (_) {
      return const {'paused': false, 'delivered': false};
    }
  }

  void dispose() {
    _proc?.closeInput();
    _proc?.kill();
    _events.close();
  }

  /// **只在测试里用**：把「起没起过」的内部状态清干净。
  ///
  /// 为什么需要它（2026-11-01 撞的）：`EngineClient` 是全局单例，`_started` 一旦
  /// 变成 true 就不会再调用 `transportFactory`。widget 测试一个文件里跑好几个用例，
  /// 每个用例都换一个新的假传输 —— 不清状态的话，第二个用例的 `hello` 会被写进
  /// **第一个用例已经关掉的那条假流**里，永远等不到回包。
  /// 症状特别坑：单独跑这个文件全绿，全量并发跑偶发红（看着像 flaky，其实是状态没清）。
  ///
  /// 生产代码永远不会调它（没有 UI 入口），所以不影响任何真实行为。
  @visibleForTesting
  void resetForTest() {
    _proc?.kill();
    _proc = null;
    _started = false;
    _starting = null;
    _pending.clear();
    _helloCache = null;
    _helloInflight = null;
  }
}

/// S10 多候选：一个文件里「像归档」的一处
///
/// 字段全是引擎给的**机器标识**，句子由界面查文案渲染（与错误码同一条原则）。
class ArchiveCandidate {
  /// 格式标识（`zip` / `7z` / `rar` …）
  final String format;

  /// 归档数据的起点（字节偏移）
  final int offset;

  /// 置信度：`high` / `medium` / `low`
  final String confidence;

  /// 是不是引擎默认会用的那一处（永远恰有一条）
  final bool chosen;

  const ArchiveCandidate({
    required this.format,
    required this.offset,
    required this.confidence,
    required this.chosen,
  });

  factory ArchiveCandidate.fromJson(Map<String, dynamic> j) => ArchiveCandidate(
        format: (j['format'] as String?) ?? 'unknown',
        offset: (j['offset'] as num?)?.toInt() ?? 0,
        confidence: (j['confidence'] as String?) ?? 'low',
        chosen: j['chosen'] == true,
      );

  /// 可信度的人话（S10 稿：「可信度高」/「可能是误报」）
  String get confidenceText => switch (confidence) {
        'high' => t('engine.confHigh'),
        'medium' => t('engine.confMedium'),
        _ => t('engine.confLow'),
      };

  /// 位置的人话：字节偏移对新手没意义，换成「从文件开头算第 X MB 处」
  String get offsetText {
    if (offset <= 0) return t('engine.atStart');
    const mb = 1024 * 1024;
    if (offset >= mb) {
      return t('engine.atMb', {'mb': (offset / mb).toStringAsFixed(1)});
    }
    final kb = offset / 1024;
    return t('engine.atKb', {'kb': kb.toStringAsFixed(0)});
  }
}
