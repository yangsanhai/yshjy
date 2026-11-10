// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! FFI 传输：**进程内**跑引擎（安卓用）
//!
//! 为什么需要它：安卓不允许 `fork/exec` 我们自己的二进制，桌面那条
//! `Process.start('yshjy --serve')` 在手机上直接不通（见 docs/ANDROID-整体方案-2026-11-10.md §4.2）。
//!
//! 与 `ProcessTransport` 的**行为契约完全一致**：只搬「整行 JSON」。区别只在底下：
//!
//! ```text
//! writeLine(一行 JSON)
//!    → yshjy_job_submit(json, cb, NULL)       // 后台线程跑，立即返回句柄
//!    → cb 每来一条事件 → 直接塞进 stdoutLines（一行一条）
//!    → 另起 Isolate 里阻塞调 yshjy_job_wait    // 绝不能卡住 UI isolate
//!    → 取回结果串 → 补最后一行 → yshjy_job_free
//! ```
//!
//! 三个实现细节（都是踩过坑才这么写的）：
//!
//! 1. **`job_wait` 必须放到别的 isolate**：它是阻塞调用，在主 isolate 上会冻界面。
//!    `Pointer` 不能跨 isolate 传递，所以传地址（`int`），在那边 `fromAddress` 重建。
//! 2. **事件回调用 `NativeCallable.listener`**：引擎在**自己的工作线程**上回调，
//!    `listener` 会把调用投递回 Dart 事件循环，天然线程安全。
//! 3. **结果最后发**：`job_wait` 返回时，引擎已经把事件都发完了，但事件是**异步投递**
//!    回来的消息，可能还排在队列里。所以发结果前让出一轮事件循环，保证「事件在前、
//!    结果在后」——与管道模式的顺序一致（`EngineClient` 靠这个顺序收尾）。

import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

import 'engine_transport.dart';

/// C 侧回调签名：`void cb(const char *json, size_t len, void *user)`
typedef _EventCb = Void Function(Pointer<Char> json, IntPtr len, Pointer<Void> user);

/// `YshjyJob *yshjy_job_submit(const char *json, yshjy_event_cb cb, void *user)`
typedef _JobSubmitNative = Pointer<Void> Function(
    Pointer<Utf8> json, Pointer<NativeFunction<_EventCb>> cb, Pointer<Void> user);
typedef _JobSubmitDart = Pointer<Void> Function(
    Pointer<Utf8> json, Pointer<NativeFunction<_EventCb>> cb, Pointer<Void> user);

/// 一个引擎实例的 C 函数表
class _Api {
  _Api(DynamicLibrary lib)
      : abiVersion = lib.lookupFunction<Uint32 Function(), int Function()>('yshjy_abi_version'),
        version =
            lib.lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>('yshjy_version'),
        jobSubmit = lib.lookupFunction<_JobSubmitNative, _JobSubmitDart>('yshjy_job_submit'),
        jobCancel =
            lib.lookupFunction<Int32 Function(Pointer<Void>), int Function(Pointer<Void>)>(
                'yshjy_job_cancel'),
        jobWait = lib.lookupFunction<Pointer<Utf8> Function(Pointer<Void>),
            Pointer<Utf8> Function(Pointer<Void>)>('yshjy_job_wait'),
        jobFree = lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>(
            'yshjy_job_free'),
        stringFree = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>(
            'yshjy_string_free');

  final int Function() abiVersion;
  final Pointer<Utf8> Function() version;
  final _JobSubmitDart jobSubmit;
  final int Function(Pointer<Void>) jobCancel;
  final Pointer<Utf8> Function(Pointer<Void>) jobWait;
  final void Function(Pointer<Void>) jobFree;
  final void Function(Pointer<Utf8>) stringFree;
}

/// 在**另一个 isolate** 里阻塞等结果（顶层函数：`Isolate.run` 要求可发送）
String _waitJobResultIsolate(String libraryName, int jobAddress) {
  final lib = DynamicLibrary.open(libraryName);
  final wait = lib.lookupFunction<Pointer<Utf8> Function(Pointer<Void>),
      Pointer<Utf8> Function(Pointer<Void>)>('yshjy_job_wait');
  final free = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>(
      'yshjy_string_free');
  final job = Pointer<Void>.fromAddress(jobAddress);
  final p = wait(job);
  if (p == nullptr) {
    return jsonEncode({
      'ok': false,
      'error': {'code': 'NULL_RESULT', 'message': '引擎返回了空指针'},
    });
  }
  try {
    return p.toDartString();
  } finally {
    free(p);
  }
}

/// 安卓（以及将来 iOS）用的进程内传输
class FfiTransport implements EngineTransport {
  FfiTransport({String library = 'libyshjy.so'}) : _libraryName = library;

  final String _libraryName;
  final _out = StreamController<String>.broadcast();
  final _err = StreamController<String>.broadcast();
  final _exit = Completer<int>();

  _Api? _api;
  NativeCallable<_EventCb>? _cb;
  final Set<Pointer<Void>> _live = {};
  bool _closed = false;

  /// 引擎版本串（`--version` 那句），排错时打印用
  String get engineVersion {
    final api = _api;
    if (api == null) return '(未启动)';
    final p = api.version();
    if (p == nullptr) return '(空)';
    try {
      return p.toDartString();
    } finally {
      api.stringFree(p);
    }
  }

  @override
  Future<void> start() async {
    if (_api != null) return;
    final lib = DynamicLibrary.open(_libraryName);
    final api = _Api(lib);
    final abi = api.abiVersion();
    if (abi != 1) {
      throw StateError('引擎 C ABI 版本不符：库是 $abi，界面要 1');
    }
    _cb = NativeCallable<_EventCb>.listener(_onEvent);
    _api = api;
  }

  void _onEvent(Pointer<Char> json, int len, Pointer<Void> _) {
    if (_closed) return;
    try {
      final bytes = json.cast<Uint8>().asTypedList(len);
      _out.add(utf8.decode(bytes));
    } catch (e) {
      // 回调里不能抛（会穿到引擎线程），只记不炸
      _err.add('事件解码失败: $e');
    }
  }

  @override
  Stream<String> get stdoutLines => _out.stream;

  @override
  Stream<String> get stderrLines => _err.stream;

  @override
  Future<int> get exitCode => _exit.future;

  @override
  Future<void> writeLine(String line) async {
    final api = _api;
    if (api == null) throw StateError('FfiTransport 还没 start()');
    final json = line.toNativeUtf8();
    try {
      final job = api.jobSubmit(json, _cb!.nativeFunction, nullptr);
      if (job == nullptr) {
        _out.add(jsonEncode({
          'ok': false,
          'error': {'code': 'JOB_ALLOC', 'message': '引擎分配任务句柄失败'},
        }));
        return;
      }
      _live.add(job);
      unawaited(_awaitResult(job));
    } finally {
      malloc.free(json);
    }
  }

  Future<void> _awaitResult(Pointer<Void> job) async {
    final api = _api!;
    String result;
    try {
      // 只把「库名 + 地址」带过去：闭包里若碰 	his（或 Pointer），
      // Isolate.run 会试图发送整个 FfiTransport（带 Completer）→
      // Illegal argument in isolate message: object is unsendable。实测踩过。
      final libraryName = _libraryName;
      final address = job.address;
      result = await Isolate.run(() => _waitJobResultIsolate(libraryName, address));
    } catch (e) {
      result = jsonEncode({
        'ok': false,
        'error': {'code': 'WAIT_FAILED', 'message': '$e'},
      });
    }
    // 让还没投递到的事件先落地（见文件头「实现细节 3」）
    await Future<void>.delayed(Duration.zero);
    if (!_closed) _out.add(result);
    _live.remove(job);
    api.jobFree(job);
  }

  @override
  Future<void> closeInput() async {
    // 没有「输入端」可关：进程内调用，收尾在 kill()
  }

  @override
  void kill() {
    if (_closed) return;
    _closed = true;
    final api = _api;
    if (api != null) {
      for (final job in _live.toList()) {
        api.jobCancel(job);
      }
    }
    _live.clear();
    if (!_exit.isCompleted) _exit.complete(0);
    _out.close();
    _err.close();
  }
}
