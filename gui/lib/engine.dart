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
  });

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
          'error': {'code': 'ENGINE_EXITED', 'note': '引擎进程已退出'},
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
        'error': {'code': 'TIMEOUT', 'note': '引擎太久没回应'},
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

  Future<Map<String, dynamic>> hello() async =>
      _unwrap(await _call({'cmd': 'hello'}));

  Future<Preview> list(String path, {String? password}) async {
    final r = await _call({
      'cmd': 'list',
      'path': path,
      if (password != null) 'password': password,
    });
    return Preview.fromJson(_unwrap(r));
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
      // 声明「我能处理 ASK_USER」—— 不写的话引擎遇到限额直接失败（协议设计如此）
      if (ask) 'ask': true,
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
    String? cover,
    /// 自解压种类（P23-5）：'windows'（exe）/ 'linux'（.sh）；null = 不自解压
    String? sfxKind,
    /// 压缩级别（P23-4）：0 = 仅存储最快 / 6 = 标准 / 9 = 最小体积
    int? level,
  }) async {
    final r = await _call({
      'cmd': 'pack',
      'inputs': inputs,
      'out': out,
      'mode': mode,
      if (password != null) 'password': password,
      if (cover != null) 'cover': cover,
      if (sfxKind != null) 'sfx': sfxKind,
      if (splitBytes != null) 'split': splitBytes,
      if (level != null) 'level': level,
      'checksum': checksum,
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
  Future<Map<String, dynamic>> report({String? path}) async {
    final r = await _call({
      'cmd': 'report',
      if (path != null) 'path': path,
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

  void dispose() {
    _proc?.closeInput();
    _proc?.kill();
    _events.close();
  }
}
