// 设置页 / 历史页测试用的**最小假引擎** —— 只回答 `hello`，其余命令不回。
//
// 为什么需要它（2026-11-01，T17-1 带出来的）：
//   格式表以前要用户点一下才问引擎，所以设置页的测试从来没碰过 EngineClient。
//   T17-1 改成"进页面就自动显示"之后，`initState` 里就会 `hello()` ——
//   不注入假传输的话，测试会去真的 spawn 子进程，轻则慢、重则挂在假时钟里
//   （`Process.start` 的定时器在 FakeAsync 里永远不完成）。
//
// 用法（照 pack_*_widget_test 的老写法）：
//   late FakeEngine fake;
//   setUp(() { fake?.dispose(); fake = FakeEngine(); EngineClient.transportFactory = (_, __) => fake!; });
//   tearDownAll(() => EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args));
//
// ⚠️ `EngineClient` 是单例，它**只起一次**传输层：所以每个用例都要给一个新的
//    fake（旧的 dispose 掉），否则第二个用例会对着已经关掉的流等回包。

import 'dart:async';
import 'dart:convert';

import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/transport/engine_transport.dart';

/// 换上一个新的假引擎，**并把 `EngineClient` 的内部状态清干净**。
///
/// 为什么不能只设置 `transportFactory`：`EngineClient` 是全局单例，`_started`
/// 一旦为 true 就不会再调用工厂 —— 同一个测试文件里的第二个用例会对着**已经
/// 关掉的旧假流**发命令，永远等不到回包（症状：单跑绿、全量并发跑偶发红）。
/// 这个坑是 2026-11-01 在 T17-1/T17-2 上撞的，别再手写 factory 赋值。
void installFakeEngine(FakeEngine fake) {
  EngineClient.transportFactory = (_, __) => fake;
  EngineClient.instance.resetForTest();
}

/// 收尾：恢复真实子进程传输（`tearDownAll` 里调）
void restoreRealTransport() {
  EngineClient.instance.resetForTest();
  EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args);
}

class FakeEngine implements EngineTransport {
  final _out = StreamController<String>.broadcast();
  final _err = StreamController<String>.broadcast();

  /// 收到了哪些命令（断言「界面到底发没发」用）
  final List<Map<String, dynamic>> sent = [];

  /// 让**下一次** hello 失败一次（模拟「引擎刚 fork、还没预热」的真机冷启动）
  bool failHelloOnce = false;

  /// `hello` 的返回值。默认给一份有 `formats` + `disk` 的最小表：
  /// 格式表要能出两行、磁盘建议要能说「机械硬盘」。
  Map<String, dynamic> helloResult = {
    'engine': 'test',
    'protocol': 1,
    'disk': 'rotational',
    'formats': [
      {
        'format': 'zip',
        'shape': 'single',
        'encrypted': true,
        'can_create': true,
      },
      {
        'format': '7z',
        'shape': 'single',
        'encrypted': false,
        'can_create': false,
      },
    ],
  };

  @override
  Future<void> start() async {}

  @override
  Stream<String> get stdoutLines => _out.stream;

  @override
  Stream<String> get stderrLines => _err.stream;

  @override
  Future<int> get exitCode async => 0;

  @override
  Future<void> closeInput() async {}

  @override
  void kill() {}

  void dispose() {
    _out.close();
    _err.close();
  }

  Map<String, dynamic>? lastCmd(String name) {
    for (final m in sent.reversed) {
      if (m['cmd'] == name) return m;
    }
    return null;
  }

  @override
  Future<void> writeLine(String line) async {
    final m = (jsonDecode(line) as Map).cast<String, dynamic>();
    sent.add(m);
    if (m['cmd'] == 'hello') {
      if (failHelloOnce) {
        failHelloOnce = false;
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': false,
          'error': {'code': 'ENGINE_EXITED', 'note': '模拟冷启动失败'},
        }));
        return;
      }
      _out.add(jsonEncode({'id': m['id'], 'ok': true, 'result': helloResult}));
    }
  }
}
