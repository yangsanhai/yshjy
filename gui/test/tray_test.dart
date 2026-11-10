// 托盘通道的测试
//
// 托盘是「锦上添花」：坏掉不许拖垮主流程。所以这里的重点不是「能不能藏窗口」，
// 而是：
// 1. 没有原生实现时（widget 测试、非 Linux 平台）**不许抛异常**
// 2. 真的发出去了的时候，方法名和参数必须跟 C++ 侧对得上
//    （gui/linux/runner/my_application.cc 里的 "yshjy/tray"）
import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/desktop/app_shell.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/tray.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('没有原生实现时 hideWindow 不抛异常', () async {
    // 通道不存在 → MissingPluginException。Tray 必须自己吃掉。
    await expectLater(Tray.hideWindow(), completes);
  });

  test('没有原生实现时 setStatus 不抛异常', () async {
    await expectLater(Tray.setStatus('测试'), completes);
  });

  test('setStatus 把 {"text": ...} 发到 yshjy/tray 通道', () async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('yshjy/tray'),
            (MethodCall call) async {
      calls.add(call);
      return null;
    });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(const MethodChannel('yshjy/tray'), null);
    });

    await Tray.setStatus('正在解压 42%');

    expect(calls, hasLength(1), reason: '应该正好发一次调用');
    expect(calls.single.method, 'set_status',
        reason: '方法名必须和 C++ 侧 my_application.cc 里的分支一致');
    final args = calls.single.arguments;
    expect(args, isA<Map<Object?, Object?>>(),
        reason: '标准 codec 解出来的 map 键值类型是 Object?');
    expect((args as Map<Object?, Object?>)['text'], '正在解压 42%',
        reason: '参数键名必须是 "text"，C++ 侧按 "text" 取值');
  });

  test('hideWindow 发的方法名是 hide_window', () async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('yshjy/tray'),
            (MethodCall call) async {
      calls.add(call);
      return null;
    });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(const MethodChannel('yshjy/tray'), null);
    });

    await Tray.hideWindow();

    expect(calls, hasLength(1));
    expect(calls.single.method, 'hide_window',
        reason: 'C++ 侧按 "hide_window" 分支处理藏窗口');
  });

  // ───────────────────── 托盘状态行：真实解压进度 ─────────────────────
  //
  // 之前托盘状态行只是抄底部状态栏的碎文字（「找到 3 个可解包」之类），
  // 窗口藏起来之后用户从托盘看到的既不是进度也不是百分比。
  // 现在改成：托盘只显示引擎事件算出来的**真实进度**（正在解压 42%），
  // 规则集中在 app_shell.dart 的 trayStatusForEvent（唯一真源）里。
  //
  // 这里测两层：
  // ① 纯函数（规则本身：节流、100% 必发、任务结束回空闲）
  // ② 通道（外壳订阅引擎事件后，托盘真的收到了 set_status）

  /// 造一条进度事件（形状跟 Rust 侧 serde 序列化出来的一致）
  EngineEvent progressEvent(String kind, double fraction) {
    const total = 1000;
    return EngineEvent.fromJson({
      'type': 'event',
      'kind': kind,
      'event': {
        'ExtractProgress': {
          'job': 1,
          'progress': {
            'done_bytes': (total * fraction).round(),
            'total_bytes': total,
            'done_files': 1,
            'total_files': 10,
          },
          'current': null,
        }
      },
    });
  }

  test('规则：进度涨够 2% 才发托盘，100% 一定发', () {
    // 第一次：还没有基准（-1），一定发
    final r1 = trayStatusForEvent(progressEvent('EXTRACT_PROGRESS', 0.10), -1);
    expect(r1.text, '正在解压 10%');
    expect(r1.lastPct, 10);

    // 10% → 12%：差 2，够步长 → 发
    final r2 = trayStatusForEvent(
        progressEvent('EXTRACT_PROGRESS', 0.12), r1.lastPct);
    expect(r2.text, '正在解压 12%');
    expect(r2.lastPct, 12);

    // 12% → 12.5%：floor 后还是 12%，没涨 → 不发（密集进度事件不许刷通道）
    final r3 = trayStatusForEvent(
        progressEvent('EXTRACT_PROGRESS', 0.125), r2.lastPct);
    expect(r3.text, isNull, reason: '涨不到 2% 就不该再过通道');
    expect(r3.lastPct, 12, reason: '没发出去就不能推进节流基准');

    // 12% → 100%：一定发（用户最想看到「解完了」）
    final r4 = trayStatusForEvent(
        progressEvent('EXTRACT_PROGRESS', 1.0), r3.lastPct);
    expect(r4.text, '正在解压 100%');
    expect(r4.lastPct, 100);
  });

  test('规则：打包进度说的是「正在打包」，任务结束回「空闲」', () {
    final r = trayStatusForEvent(progressEvent('PACK_PROGRESS', 0.42), -1);
    expect(r.text, '正在打包 42%');

    // 任务结束 → 空闲，并且把节流基准清零（下一单从头算）
    final done = trayStatusForEvent(
        EngineEvent.fromJson({
          'type': 'event',
          'kind': 'JOB_FINISHED',
          'event': {
            'JobFinished': {'job': 1, 'outcome': 'Success'}
          }
        }),
        42);
    expect(done.text, '空闲');
    expect(done.lastPct, -1, reason: '任务结束必须重置节流基准');
  });

  test('规则：别的事件不碰托盘，总字节未知时也没有百分比', () {
    final other = trayStatusForEvent(
        EngineEvent.fromJson({
          'type': 'event',
          'kind': 'STAGE_STARTED',
          'event': {
            'StageStarted': {'job': 1, 'stage': 'Locate'}
          }
        }),
        -1);
    expect(other.text, isNull, reason: '阶段事件不该写托盘（会把进度行冲掉）');

    // total_bytes == 0 → fraction 是 null → 没有百分比可言
    final unknown = trayStatusForEvent(
        EngineEvent.fromJson({
          'type': 'event',
          'kind': 'EXTRACT_PROGRESS',
          'event': {
            'ExtractProgress': {
              'job': 1,
              'progress': {
                'done_bytes': 0,
                'total_bytes': 0,
                'done_files': 0,
                'total_files': 0,
              },
              'current': null,
            }
          }
        }),
        -1);
    expect(unknown.text, isNull);
  });

  testWidgets('外壳订阅引擎事件：进度和「空闲」都真的发到托盘通道', (tester) async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('yshjy/tray'),
            (MethodCall call) async {
      calls.add(call);
      return null;
    });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(const MethodChannel('yshjy/tray'), null);
    });

    // 假引擎：只回 hello，其余靠测试自己往 stdout 塞事件
    final fake = _FakeEngine();
    final oldFactory = EngineClient.transportFactory;
    EngineClient.transportFactory = (_, __) => fake;
    addTearDown(() => EngineClient.transportFactory = oldFactory);

    await tester.pumpWidget(const MaterialApp(home: AppShell()));
    await tester.pump();

    // 10% → 12% → 12.5%（不涨，不该发）→ 结束
    fake.emitProgress('EXTRACT_PROGRESS', 0.10);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 20));
    fake.emitProgress('EXTRACT_PROGRESS', 0.12);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 20));
    fake.emitProgress('EXTRACT_PROGRESS', 0.125);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 20));
    fake.emitJobFinished();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 20));

    final texts = calls
        .where((c) => c.method == 'set_status')
        .map((c) => (c.arguments as Map)['text'] as String)
        .toList();

    // 只看进度那条线：「正在处理…」是「引擎忙但还没百分比」时的临时状态，
    // 跟这里要验的进度行不是一回事，所以单独滤出来。
    final progressTexts = texts.where((t) => t.startsWith('正在解压')).toList();
    expect(progressTexts, ['正在解压 10%', '正在解压 12%'],
        reason: '10% 和 12% 各发一次；12.5% 没涨够 2% 不该发');
    expect(texts.where((t) => t == '空闲'), hasLength(1),
        reason: '任务结束托盘要回到「空闲」');
  });
}

/// 假引擎传输：把测试塞进来的事件当成引擎 stdout 的 NDJSON 喂出去
class _FakeEngine implements EngineTransport {
  final _out = StreamController<String>.broadcast();
  final _err = StreamController<String>.broadcast();

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

  @override
  Future<void> writeLine(String line) async {
    // 只回 hello（外壳启动时会问版本），其余命令测试里用不到
    final m = jsonDecode(line) as Map<String, dynamic>;
    if (m['cmd'] == 'hello') {
      _out.add(jsonEncode({
        'id': m['id'],
        'ok': true,
        'result': {'engine': 'test', 'protocol': 1},
      }));
    }
  }

  /// 塞一条进度事件
  void emitProgress(String kind, double fraction) {
    const total = 1000;
    _out.add(jsonEncode({
      'type': 'event',
      'kind': kind,
      'event': {
        'ExtractProgress': {
          'job': 1,
          'progress': {
            'done_bytes': (total * fraction).round(),
            'total_bytes': total,
            'done_files': 1,
            'total_files': 10,
          },
          'current': null,
        }
      },
    }));
  }

  /// 塞一条任务结束事件
  void emitJobFinished() {
    _out.add(jsonEncode({
      'type': 'event',
      'kind': 'JOB_FINISHED',
      'event': {
        'JobFinished': {'job': 1, 'outcome': 'Success'}
      },
    }));
  }
}
