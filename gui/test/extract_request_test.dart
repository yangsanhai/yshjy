// 批 7b：`buildExtractRequest` 纯函数单测
//
// 以前这些判断长在 `_HomePageState._extract` 里，28 处条件**一条测试都没有** ——
// 要测只能起假引擎走整条 NDJSON 链路（`test/fake_engine.dart`），那验的是协议链路，
// 判断写错了它也可能是绿的。抽成纯函数后，这里每一条都是对**行为契约**的断言。
//
// 反向验证（改坏看红，2026-11-01 实测）：
//   把 `cover: verifyOnly ? false : wantCover` 改成 `cover: wantCover` → 第 2 条立刻红
//   （「只检查」会把封面切出来，磁盘上莫名多一个视频 —— 而界面说「没写任何文件」）。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/pages/home_extract_request.dart';

/// 4 个文件 + 1 个目录（目录不该计入「是不是全选」）
Preview sample({Map<String, dynamic>? cover}) => Preview(
      format: 'zip',
      complete: true,
      totalSize: 13440,
      encryptedCount: 0,
      encKind: null,
      cover: cover,
      entries: [
        EntryItem(
            index: 1,
            path: '甲.txt',
            size: 10,
            packed: 5,
            isDir: false,
            encrypted: false,
            risky: false),
        EntryItem(
            index: 2,
            path: '乙.txt',
            size: 10,
            packed: 5,
            isDir: false,
            encrypted: false,
            risky: false),
        EntryItem(
            index: 3,
            path: '丙.txt',
            size: 10,
            packed: 5,
            isDir: false,
            encrypted: false,
            risky: false),
        EntryItem(
            index: 4,
            path: '丁.txt',
            size: 10,
            packed: 5,
            isDir: false,
            encrypted: false,
            risky: false),
        EntryItem(
            index: 5,
            path: '目录/',
            size: 0,
            packed: 0,
            isDir: true,
            encrypted: false,
            risky: false),
      ],
      hints: const [],
    );

/// 默认都在「普通解压」这一档，各用例只改自己关心的那几项
ExtractRequest build({
  Preview? preview,
  Set<int>? picked,
  bool wantCover = true,
  bool wantExtract = true,
  String? outDir,
  bool verifyOnly = false,
  int? candidate,
  bool force = false,
  bool? smartDir = true,
}) {
  final pv = preview ?? sample();
  return buildExtractRequest(
    path: '/tmp/样本.mp4',
    preview: pv,
    picked: picked ?? {1, 2, 3, 4},
    wantCover: wantCover,
    wantExtract: wantExtract,
    password: 'PW',
    outDir: outDir,
    encoding: 'gbk',
    smartDir: smartDir,
    force: force,
    candidate: candidate,
    verifyOnly: verifyOnly,
  );
}

void main() {
  test('普通解压 + 全选：不传 select，其余字段照界面透传', () {
    final r = build(outDir: '/home/me/下载', smartDir: false);

    expect(r.path, '/tmp/样本.mp4');
    expect(r.password, 'PW');
    expect(r.encoding, 'gbk');
    expect(r.dest, '/home/me/下载');
    // 引擎默认全解；多传一个数组不仅没用，序号对不上还会漏解
    expect(r.select, isNull, reason: '全选就该不传 select');
    expect(r.cover, isTrue);
    expect(r.wantExtract, isTrue);
    expect(r.verifyOnly, isFalse);
    expect(r.candidate, isNull);
    expect(r.force, isFalse);
    expect(r.smartDir, isFalse, reason: 'E5 开关要原样传下去');
  });

  test('「只检查」：封面强制关、dest 不带、不跟界面勾选', () {
    final r = build(
      // 界面此刻：封面勾着、只解部分、还选了输出目录 —— 三条都必须被覆盖掉
      picked: {1},
      wantCover: true,
      wantExtract: false,
      outDir: '/home/me/下载',
      verifyOnly: true,
    );

    expect(r.verifyOnly, isTrue);
    expect(r.cover, isFalse,
        reason: '切封面是另一条真写盘的路径，开着它「一个字节都不写」就是假话');
    expect(r.dest, isNull,
        reason: '给了 dest 引擎可能顺手把目录建出来（CLI --test 实测零字节）');
    expect(r.wantExtract, isTrue,
        reason: '校验的是整包完不完好，不跟「只解清单」的勾选走');
    expect(r.select, isNull, reason: '校验整包，不传条目序号');
  });

  test('只提封面（没勾「解压内容」）：不传 select，封面照勾', () {
    final r = build(picked: {1}, wantExtract: false, wantCover: true);

    expect(r.select, isNull, reason: '不解内容时传 select 没有意义');
    expect(r.wantExtract, isFalse);
    expect(r.cover, isTrue);
  });

  test('部分勾选：只把这几个序号传下去', () {
    final r = build(picked: {1, 3}, wantExtract: true);

    expect(r.select, [1, 3], reason: '勾了哪几个就解哪几个（P23-1 文件级挑选）');
  });

  test('换候选解压：不传旧序号（A 包的清单不能拿去挑 B 包的文件）', () {
    final r = build(picked: {1}, candidate: 4096);

    expect(r.candidate, 4096);
    expect(r.select, isNull,
        reason: '另一段里的条目跟当前预览完全可能不同，带旧序号会漏解或报错');
  });

  test('目录不算「全选」的分母：4 个文件勾满就是全选', () {
    // 条目里还有 1 个目录（index 5）。若分母把目录算进去，永远凑不满 5 个，
    // 于是每次「开始解压」都会多传一个 select —— 这就是「列表和实际解出来的不一样」的来源。
    final r = build(picked: {1, 2, 3, 4});
    expect(r.select, isNull);
  });

  test('还没有清单（preview 为 null）：按全选处理，绝不传空列表', () {
    final r = buildExtractRequest(
      path: '/tmp/x.zip',
      preview: null,
      picked: {},
      wantCover: false,
      wantExtract: true,
      password: null,
      outDir: null,
      encoding: null,
      smartDir: null,
    );

    expect(r.select, isNull, reason: '传空列表等于「一个都不解」，比全解还糟');
    expect(r.smartDir, isNull, reason: '不传 = 引擎默认 true（老行为）');
  });

  test('「继续试试」把 force 传下去（S5 跳过空间预检）', () {
    expect(build(force: true).force, isTrue);
  });

  test('send() 交给引擎的参数与字段一一对应（防手抄漏项）', () async {
    final api = _RecordingApi();
    await build(picked: {2}, outDir: '/tmp/out').send(api);

    final args = api.extractArgs!;
    expect(args['path'], '/tmp/样本.mp4');
    expect(args['dest'], '/tmp/out');
    expect(args['password'], 'PW');
    expect(args['cover'], isTrue);
    expect(args['wantExtract'], isTrue);
    expect(args['select'], [2]);
    expect(args['encoding'], 'gbk');
    expect(args['force'], isFalse);
    expect(args['smartDir'], isTrue);
    expect(args['verifyOnly'], isFalse);
  });
}

/// 只收 extract 的极简假引擎（其余成员本文件用不到）
class _RecordingApi implements EngineApi {
  Map<String, dynamic>? extractArgs;

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
    extractArgs = {
      'path': path,
      'dest': dest,
      'password': password,
      'cover': cover,
      'wantExtract': wantExtract,
      'select': select,
      'encoding': encoding,
      'force': force,
      'candidate': candidate,
      'verifyOnly': verifyOnly,
      'smartDir': smartDir,
    };
    return ExtractResult(
      outcome: 'Success',
      entriesDone: 1,
      entriesTotal: 1,
      bytesWritten: 10,
      dest: null,
      millis: 1,
    );
  }

  @override
  noSuchMethod(Invocation invocation) => throw UnimplementedError(
      '本文件只验 extract，用到了 ${invocation.memberName} 说明测试写歪了');
}
