// 批 7a：「引擎客户端可注入」的地基验收
//
// 这一批只做一件事 —— 让页面依赖 `EngineApi` 接口而不是 `EngineClient.instance`。
// 为什么值得单独一条测试（而不是靠现有 337 条兜着）：
// 现有 widget 测试全部靠替换 `EngineClient.transportFactory`，走的是**协议链路**
// （假引擎收 JSON、回 JSON）。那条链路验不到「页面拿到结果之后怎么变界面」，
// 也验不到「参数拼得对不对」之外的编排细节 —— 而且一旦页面**没**用注入的对象，
// 那些测试照样是绿的（它们看的是真单例）。
//
// 所以这里的断言必须做到「注入对象是唯一的引擎来源」：
//   ① 走预览路径：页面必须调用**注入对象的** list()，并把它的结果渲染出来；
//   ② 走解压路径：页面必须把**界面状态**（勾选、封面开关）翻译成 extract 的参数；
//   ③ 页面销毁时会 dispose 注入对象（与单例行为完全一致，别留一个"生产会关、测试不会关"的分叉）。
//
// 反向验证（改坏看红，2026-11-01 实测）：把 `_client` 改回
// `EngineClient.instance`，本文件 ① ② 两条立刻变红（假引擎一条命令都收不到）。

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/notify.dart';
import 'package:yshjy_gui/pages/home_page.dart';

/// 带伪装封面 + 两个文件的最小预览（与 verify_only_widget_test 同一份样本口径：
/// 有封面才验得出「普通解压带 cover、只检查不带」）。
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

/// 定次 pump（理由同 verify_only_widget_test：解压成功后这段路会碰真文件 I/O，
/// `pumpAndSettle` 在假时钟里会一路等到超时）。
Future<void> pumpFrames(WidgetTester tester, {int frames = 25}) async {
  for (var i = 0; i < frames; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

void main() {
  setUp(() {
    // 用户明确反感弹窗/提示音；而且 command 那条路会起真进程 + 定时器
    Notifier.enabled = false;
  });

  testWidgets('预览路径：页面用的是**注入对象**的 list()，并渲染它的结果', (tester) async {
    final fake = _FakeApi();
    tester.view.physicalSize = const Size(1100, 1700);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    // ⚠️ 必须 `runAsync`：`_open` 第一行是 `await Directory(path).exists()` ——
    // **真实文件系统 IO**。`testWidgets` 默认是假时钟，真实 IO 的 Future 永远不完成，
    // `_open` 就卡在第一行（症状：假引擎一条命令都收不到，看着像注入没生效）。
    // 这批第一次就踩到了，别再改成裸 pump。
    await tester.runAsync(() async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          // 路径不需要真存在：`_open` 先看是不是目录（不是）→ 直接 list
          body: HomePage(engine: fake, autoStart: false, initialFile: '/tmp/样本伪装.mp4'),
        ),
      ));
      for (var i = 0; i < 5; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 20));
        await tester.pump();
      }
    });
    await pumpFrames(tester, frames: 3);

    expect(fake.calls, contains('list'),
        reason: '注入的对象没被调用 —— 页面还在用 EngineClient.instance');
    expect(fake.lastArgs('list')?['path'], '/tmp/样本伪装.mp4');
    // 注入结果的渲染证据：条目名出现在文件列表里
    expect(find.textContaining('甲.txt'), findsOneWidget);
    expect(find.textContaining('乙.bin'), findsOneWidget);
  });

  testWidgets('解压路径：界面状态被翻译成 extract 参数（封面开、全选不带 select）',
      (tester) async {
    final fake = _FakeApi();
    tester.view.physicalSize = const Size(1100, 1700);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: HomePage(
          engine: fake,
          autoStart: false,
          // 直接进预览页（与 `_open` 的初始化同一套字段，见 `debugPreview` 注释）
          debugPreview: sample(),
        ),
      ),
    ));
    await tester.pumpAndSettle();

    await tester.tap(find.textContaining('开始解压'));
    await pumpFrames(tester);

    final args = fake.lastArgs('extract');
    expect(args, isNotNull, reason: '点了「开始解压」必须真的调注入对象的 extract');
    expect(args!['cover'], isTrue, reason: '伪装包的封面默认要勾上（与 _open 里 wantCover=hasCover 一致）');
    expect(args['wantExtract'], isTrue);
    // 两个文件默认全选 → 按设计**不传** select（引擎默认全解，与老行为一致）
    expect(args.containsKey('select'), isFalse,
        reason: '全选时传 select 不仅多余，还会在条目序号不一致时漏解');
  });

  testWidgets('页面销毁时 dispose 注入对象（与单例行为一致）', (tester) async {
    final fake = _FakeApi();
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(body: HomePage(engine: fake, autoStart: false, debugPreview: sample())),
    ));
    await tester.pumpAndSettle();
    expect(fake.disposed, 0);

    await tester.pumpWidget(const MaterialApp(home: SizedBox()));
    await tester.pumpAndSettle();
    expect(fake.disposed, 1,
        reason: '生产会 dispose 单例，测试路径也必须一样 —— 否则真机上有资源没收');
  });
}

/// 普通 Dart 对象实现的假引擎（**不是** EngineTransport，不走 JSON）。
///
/// 每个用到的命令记一条 `calls` + 参数快照；没实现的方法抛异常 ——
/// 万一哪天页面偷偷调了新命令，测试会立刻红，而不是静悄悄走默认值。
class _FakeApi implements EngineApi {
  final List<String> calls = [];
  final Map<String, List<Map<String, dynamic>>> _args = {};
  final _events = StreamController<EngineEvent>.broadcast();
  int disposed = 0;

  Map<String, dynamic>? lastArgs(String cmd) {
    final list = _args[cmd];
    return (list == null || list.isEmpty) ? null : list.last;
  }

  void _record(String cmd, Map<String, dynamic> args) {
    calls.add(cmd);
    _args.putIfAbsent(cmd, () => []).add(args);
  }

  @override
  Stream<EngineEvent> get events => _events.stream;

  @override
  bool get busy => false;

  @override
  Future<void> start() async {}

  @override
  Future<Map<String, dynamic>> hello({bool refresh = false}) async =>
      {'engine': 'fake-api', 'protocol': 1};

  @override
  Future<Preview> list(String path, {String? password}) async {
    _record('list', {'path': path, 'password': password});
    return sample();
  }

  @override
  Future<ExtractResult> extract(
    String path, {
    String? dest,
    String? password,
    bool cover = true,
    bool wantExtract = true,
    bool drill = false,
    bool ask = true,
    List<int>? select,
    String? encoding,
    bool force = false,
    int? candidate,
    bool verifyOnly = false,
    bool? smartDir,
  }) async {
    _record('extract', {
      'path': path,
      'dest': dest,
      'password': password,
      'cover': cover,
      'wantExtract': wantExtract,
      if (select != null) 'select': select,
      'encoding': encoding,
      'force': force,
      'candidate': candidate,
      'verifyOnly': verifyOnly,
      'smartDir': smartDir,
    });
    // dest 给 null：解压成功后 `_maybeOpenAfterDone` 不会去碰文件系统
    return ExtractResult(
      outcome: 'Success',
      entriesDone: 2,
      entriesTotal: 2,
      bytesWritten: 13440,
      dest: null,
      millis: 7,
    );
  }

  @override
  void dispose() {
    disposed++;
    // ⚠️ 故意**不**关 _events：同一条测试里页面可能重建，
    // 关了就成了「第二次挂载时事件流已死」的假故障（与 fake_engine.dart 的坑同源）。
  }

  // ── 本文件用不到的命令：显式抛错，别静默 ──
  @override
  Future<Map<String, dynamic>> pack({
    required List<String> inputs,
    required String out,
    String? password,
    int? splitBytes,
    bool checksum = true,
    bool readme = true,
    String mode = 'disguise',
    String? format,
    String? cover,
    String? sfxKind,
    int? level,
    int? decoy,
    int? decoySize,
    bool decoySignatures = false,
    bool tweakTime = false,
    String? layout,
    String? second,
  }) =>
      throw UnimplementedError('测试没用到 pack');

  @override
  Future<Map<String, dynamic>> scan(List<String> paths) =>
      throw UnimplementedError('测试没用到 scan');

  @override
  Future<Map<String, dynamic>> batch({
    required List<String> paths,
    String? dest,
    String? password,
    String? encoding,
  }) =>
      throw UnimplementedError('测试没用到 batch');

  @override
  Future<Map<String, dynamic>> answer(String value) =>
      throw UnimplementedError('测试没用到 answer');

  @override
  Future<Map<String, dynamic>> skipCurrent() =>
      throw UnimplementedError('测试没用到 skip');

  @override
  Future<Map<String, dynamic>> report({String? path, String? outDir}) =>
      throw UnimplementedError('测试没用到 report');

  @override
  Future<Map<String, dynamic>> rescue(String path,
          {String? dest, bool listOnly = false, String? encoding}) =>
      throw UnimplementedError('测试没用到 rescue');

  @override
  Future<void> cancel() async {}

  @override
  Future<Map<String, dynamic>> cancelJob(int job) async => const {};

  @override
  Future<Map<String, dynamic>> pauseQueue() async => const {};

  @override
  Future<Map<String, dynamic>> resumeQueue() async => const {};
}
