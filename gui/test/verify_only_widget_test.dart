// E4「只检查」的界面回归（T7 第 1 项）
//
// 三件事必须钉住，任一条漏了都是「看着像做了、其实没做」：
//   ① 点「只检查」→ 协议发出去的 extract 里 `verify_only == true`
//      （引擎侧早就支持，界面不传就等于功能不存在）；
//   ② 同一请求里 `cover == false` 且**不带** `dest` —— 切封面是另一条真写盘的路径，
//      留着它「一个字节都不写」就是假话；带 dest 则引擎可能顺手把目录建出来；
//   ③ 普通「开始解压」**不许**带 `verify_only`。反向验证：默认值要是写成恒真，
//      每次解压都变成空跑，用户以为解完了、硬盘里什么都没有。
//
// 结果页另有一条：不能出现「解开了 N 个文件」和「打开目录」。
// 夹具里的数字是照真机 `/usr/bin/yshjy --test --json` 的原样输出抄的 ——
// 它照样报 done=2 / bytes=13440，还回显一个并不存在的 dest。
// 界面照普通成功页渲染就会把用户带到一个不存在的目录去。

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/notify.dart';
import 'package:yshjy_gui/desktop/pages/home_page.dart';
import 'package:yshjy_gui/common/transport/engine_transport.dart';

/// 带伪装封面的预览。
///
/// 故意给一份 hasCover 的样本：没有封面就验不出「只检查时封面有没有被关掉」，
/// 那条恰恰是最容易漏的（漏了用户会发现包旁边莫名多出一个视频）。
Preview sample() => Preview(
      format: 'zip',
      complete: true,
      totalSize: 13440,
      encryptedCount: 0,
      encKind: null,
      cover: const {'size': 24000, 'kind': 'mp4', 'ext': 'mp4'},
      entries: [
        EntryItem(
            index: 1,
            path: '甲.txt',
            size: 5000,
            packed: 800,
            isDir: false,
            encrypted: false,
            risky: false),
        EntryItem(
            index: 2,
            path: 'dir/乙.bin',
            size: 8440,
            packed: 3000,
            isDir: false,
            encrypted: false,
            risky: false),
      ],
      hints: const [],
    );

Future<void> pumpPreview(WidgetTester tester) async {
  // 视口给大：预览页有勾选卡 + 状态栏 + 列表，默认 800x600 下底部按钮行会被挤掉
  tester.view.physicalSize = const Size(1100, 1700);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    home: Scaffold(body: HomePage(autoStart: false, debugPreview: sample())),
  ));
  await tester.pumpAndSettle();
}

/// 定次 pump，不用 pumpAndSettle。
///
/// 为什么：普通解压那条路会接着走 `_maybeOpenAfterDone` / `HistoryStore.add`
/// 这类**真文件 I/O**，而 `testWidgets` 是假时钟 —— 这类 Future 永远不完成，
/// `pumpAndSettle` 就一路等到超时（看起来像界面卡死，其实是环境）。
/// 「只检查」该验的是「发出去的命令对不对」，把帧推到响应处理完就够了。
Future<void> pumpFrames(WidgetTester tester) async {
  for (var i = 0; i < 25; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

void main() {
  _FakeEngine? fake;

  setUp(() async {
    // 每条测试都要一个干净的假引擎，并且让单例**允许重起**：
    // EngineClient 是单例、引擎只起一次（`_started`），第一条测试启动后，
    // 后面的测试 stub 不到新行为 —— 实测表现是「点了按钮但命令一条都没发」，
    // 看起来像功能坏了，其实是测试环境复用了上一条的进程。
    // 关掉上一条的 stdout 会触发 `_onClosed` → `_started = false`，下一条重新 start。
    fake?.dispose();
    await Future<void>.delayed(Duration.zero);
    fake = _FakeEngine();
    EngineClient.transportFactory = (_, __) => fake!;
    // 别在跑测试时真弹桌面通知（用户明确反感弹窗/提示音）。
    // 必须用 enabled=false：走 command 那条路会真的 Process.run + 2 秒 timeout，
    // 在 widget 测试的假时钟里留下一个 pending Timer，直接判测试失败。
    Notifier.enabled = false;
  });

  tearDownAll(() =>
      EngineClient.transportFactory = (exe, args) => ProcessTransport(exe, args));

  testWidgets('点「只检查」→ 协议收到 verify_only，封面关掉、不带 dest', (tester) async {
    await pumpPreview(tester);

    await tester.tap(find.byKey(const ValueKey('verify-only')));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('extract');
    expect(cmd, isNotNull, reason: '点了按钮必须真发一条 extract');
    expect(cmd!['verify_only'], isTrue,
        reason: 'verify_only 没传 = 引擎照样写盘，这个功能等于没做');
    expect(cmd['cover'], isFalse,
        reason: '切封面是另一条写盘路径，开着它「不写任何文件」就是假话');
    expect(cmd.containsKey('dest'), isFalse,
        reason: '不给 dest，免得引擎按它建出目录、或产物撒在别处');
    expect(cmd.containsKey('select'), isFalse,
        reason: '校验的是整包，不跟着界面上的勾选走');
    expect(cmd['extract'], isTrue);
  });

  testWidgets('普通「开始解压」不许带 verify_only（反向验证）', (tester) async {
    await pumpPreview(tester);

    await tester.tap(find.textContaining('开始解压'));
    await pumpFrames(tester);

    final cmd = fake!.lastCmd('extract');
    expect(cmd, isNotNull);
    expect(cmd!.containsKey('verify_only'), isFalse,
        reason: '默认值写歪成恒真，每次解压都会空跑、用户以为解完了');
    expect(cmd['cover'], isTrue, reason: '普通解压照旧带封面（这轮只动「只检查」那条路）');
  });

  testWidgets('结果页说「校验通过」、不摆「打开目录」、不显示那个不存在的 dest',
      (tester) async {
    await pumpPreview(tester);

    await tester.tap(find.byKey(const ValueKey('verify-only')));
    await pumpFrames(tester);

    expect(find.textContaining('校验通过'), findsOneWidget);
    expect(find.textContaining('解开了'), findsNothing,
        reason: '一个字节都没写，说「解开了」就是假成功');
    expect(find.textContaining('/home/me/下载/样本'), findsNothing,
        reason: '引擎回显的 dest 在校验模式下目录并不存在，显示它就是把用户指向空处');
    expect(find.text('打开目录'), findsNothing,
        reason: '点它会报「目录不存在」，等于给个假按钮');
    expect(find.textContaining('没写任何文件'), findsOneWidget);
  });
}

/// 假引擎传输：记录界面发来的每条命令，并按命令回一条**真机原样**的响应
class _FakeEngine implements EngineTransport {
  final _out = StreamController<String>.broadcast();
  final _err = StreamController<String>.broadcast();
  final List<Map<String, dynamic>> sent = [];

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

  /// 关掉两条流：EngineClient 的 `_onClosed` 会把「已启动」复位，
  /// 下一条测试才能重新 start 一个新假引擎。
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
    switch (m['cmd']) {
      case 'hello':
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {'engine': 'test', 'protocol': 1},
        }));
      case 'extract':
        // 照 `/usr/bin/yshjy --test --json` 的真实回包抄：
        // verify_only 下 done / bytes 跟真解一样（它确实解码+校验了），
        // dest 也照样回显 —— 而目录根本不存在。界面不能被骗。
        _out.add(jsonEncode({
          'id': m['id'],
          'ok': true,
          'result': {
            'outcome': 'Success',
            'entries_done': 2,
            'entries_total': 2,
            'bytes_written': 13440,
            'dest': '/home/me/下载/样本',
            'millis': 42,
            'failed': <dynamic>[],
          },
        }));
    }
  }
}
