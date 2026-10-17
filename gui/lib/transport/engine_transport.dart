//! 引擎传输层：界面与引擎「怎么说话」的抽象
//!
//! 为什么要有这一层（DESIGN §18.1）：现在的实现是 `Process.start` 起一个子进程，
//! 但 **iOS 物理上禁止 fork/exec 任意可执行文件**，Android 也可以选择同进程 FFI。
//! 如果界面直接 `Process.start`，这些平台永远跑不了 —— 所以把「传输」抽出来：
//!
//! ```text
//! EngineClient（协议/事件，全平台一份）
//!        ↓ 只依赖这个接口
//! EngineTransport
//!        ├── ProcessTransport   Linux/Windows/macOS/Android（子进程 + NDJSON）
//!        └── FfiTransport        iOS（静态库 + C ABI，见 docs/FFI.md）
//! ```
//!
//! 约定：传输层只搬**整行 JSON**，不理解命令；解析与状态都在 `EngineClient`。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// 一条引擎连接
abstract class EngineTransport {
  /// 启动（幂等由上层保证）
  Future<void> start();

  /// 引擎 stdout 的**整行**流（每行一条 JSON）
  Stream<String> get stdoutLines;

  /// 引擎 stderr 的整行流（只用于排查，不进用户界面）
  Stream<String> get stderrLines;

  /// 进程退出码（没有进程概念的实现可以永不完成或返回 0）
  Future<int> get exitCode;

  /// 写一行（自动补换行并 flush）
  Future<void> writeLine(String line);

  /// 优雅关闭输入（引擎读到 EOF 会自己收尾）
  Future<void> closeInput();

  /// 硬杀（只在关闭窗口/异常时用）
  void kill();
}

/// 传输工厂：`EngineClient` 启动时用它，测试/其它平台可替换
typedef EngineTransportFactory = EngineTransport Function(
  String executable,
  List<String> args,
);

/// 子进程实现（当前所有桌面平台 + Android 用这个）
class ProcessTransport implements EngineTransport {
  ProcessTransport(this.executable, this.args);

  final String executable;
  final List<String> args;
  Process? _p;

  Process get _proc {
    final p = _p;
    if (p == null) {
      throw StateError('ProcessTransport 还没 start()');
    }
    return p;
  }

  @override
  Future<void> start() async {
    if (_p != null) return;
    _p = await Process.start(executable, args);
  }

  @override
  Stream<String> get stdoutLines =>
      _proc.stdout.transform(utf8.decoder).transform(const LineSplitter());

  @override
  Stream<String> get stderrLines =>
      _proc.stderr.transform(utf8.decoder).transform(const LineSplitter());

  @override
  Future<int> get exitCode => _proc.exitCode;

  @override
  Future<void> writeLine(String line) async {
    _proc.stdin.writeln(line);
    await _proc.stdin.flush();
  }

  @override
  Future<void> closeInput() => _proc.stdin.close();

  @override
  void kill() => _p?.kill();
}
