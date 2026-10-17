//! 引擎客户端 —— 与 `yshjy --serve` 说话的那一层
//!
//! 设计原则（见仓库 docs/PROTOCOL.md）：
//! - 界面**只认协议**，不认引擎实现。引擎怎么改，只要协议不动，这里就不用动。
//! - 事件按 `kind` 分支；**不认识的 kind 一律忽略**（后端加新事件不会弄坏旧界面）。
//! - 错误只拿 `code` 查文案；`note` 只用于日志/反馈。
//!
//! 这个文件同时是「Flutter 怎么接 Rust 引擎」的参考实现：
//! `Process.start` + 逐行 JSON，没有 FFI、没有平台插件、Windows/macOS/Android 同样可用。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

// ───────────────────────── 数据模型 ─────────────────────────

/// 归档里的一个条目（对应 Rust 的 `EntryView`）
class EntryItem {
  final int index;
  final String path;
  final int size;
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
    this.modified,
  });

  factory EntryItem.fromJson(Map<String, dynamic> j) => EntryItem(
        index: (j['index'] as num?)?.toInt() ?? 0,
        path: j['path'] as String? ?? '',
        size: (j['size'] as num?)?.toInt() ?? 0,
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
  final List<EntryItem> entries;
  final List<String> hints;

  Preview({
    required this.format,
    required this.complete,
    required this.totalSize,
    required this.encryptedCount,
    required this.entries,
    required this.hints,
  });

  factory Preview.fromJson(Map<String, dynamic> j) => Preview(
        format: j['format'] as String? ?? '?',
        complete: j['complete'] != false,
        totalSize: (j['total_size'] as num?)?.toInt() ?? 0,
        encryptedCount: (j['encrypted'] as num?)?.toInt() ?? 0,
        entries: (j['entries'] as List? ?? [])
            .map((e) => EntryItem.fromJson((e as Map).cast<String, dynamic>()))
            .toList(),
        hints: (j['hints'] as List? ?? []).map((e) => e.toString()).toList(),
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

  ExtractResult({
    required this.outcome,
    required this.entriesDone,
    required this.entriesTotal,
    required this.bytesWritten,
    required this.dest,
    required this.millis,
  });

  factory ExtractResult.fromJson(Map<String, dynamic> j) => ExtractResult(
        outcome: j['outcome']?.toString() ?? 'Failed',
        entriesDone: (j['entries_done'] as num?)?.toInt() ?? 0,
        entriesTotal: (j['entries_total'] as num?)?.toInt() ?? 0,
        bytesWritten: (j['bytes_written'] as num?)?.toInt() ?? 0,
        dest: j['dest'] as String?,
        millis: (j['millis'] as num?)?.toInt() ?? 0,
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
  Process? _proc;
  final _events = StreamController<EngineEvent>.broadcast();
  final Map<int, Completer<Map<String, dynamic>>> _pending = {};
  int _nextId = 1;
  bool _started = false;

  /// 引擎路径（按优先级查找）
  ///
  /// 1. 环境变量 `YSHJY_BIN`（开发时指到 target/release/yshjy）
  /// 2. **跟界面放在同一个目录的 `yshjy`** —— 发布包就是这样摆的，
  ///    用户双击就能用，不需要装 PATH
  /// 3. PATH 里的 yshjy（系统安装版）
  static String get executable {
    final env = Platform.environment['YSHJY_BIN'];
    if (env != null && env.isNotEmpty) return env;
    try {
      final beside = File(Platform.resolvedExecutable).parent.path + Platform.pathSeparator + 'yshjy';
      if (File(beside).existsSync()) return beside;
    } catch (_) {}
    return 'yshjy';
  }

  Stream<EngineEvent> get events => _events.stream;

  Future<void> start() async {
    if (_started) return;
    _proc = await Process.start(executable, ['--serve']);
    _proc!.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(_onLine, onDone: _onClosed);
    // 引擎的 stderr 只有崩溃/调试信息，不打扰用户，但留着便于排查
    _proc!.stderr
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((l) => debugPrint('[engine] $l'));
    _started = true;
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
    _proc!.stdin.writeln(payload);
    await _proc!.stdin.flush();
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
  }) async {
    final r = await _call({
      'cmd': 'extract',
      'path': path,
      if (dest != null) 'dest': dest,
      if (password != null) 'password': password,
    });
    return ExtractResult.fromJson(_unwrap(r));
  }

  /// 生成诊断包（报障用）。默认**路径脱敏**，方便直接贴到群里。
  Future<Map<String, dynamic>> report({String? path}) async {
    final r = await _call({
      'cmd': 'report',
      if (path != null) 'path': path,
      'no_paths': true,
    }, timeout: const Duration(minutes: 2));
    return _unwrap(r);
  }

  Future<void> cancel() async {
    try {
      await _call({'cmd': 'cancel'}, timeout: const Duration(seconds: 10));
    } catch (_) {
      // 取消失败不该再抛错给用户 —— 它本来就是"尽量停下"
    }
  }

  void dispose() {
    _proc?.stdin.close();
    _proc?.kill();
    _events.close();
  }
}
