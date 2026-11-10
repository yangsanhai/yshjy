// 主页四个纯函数 + AppBar 的回归 —— 按块拆分的第 9、10 块。
//
// 这四个函数原来都是 State 的私有方法（读 `_preview` / `_error`），
// 「边界对不对」只能靠 pump 整个页面 + 造引擎状态来间接验证。
// 抽成纯函数之后，一条 test() 就能钉一个边界，成本从「造一屏」降到「造一个对象」。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_app_bar.dart';
import 'package:yshjy_gui/desktop/pages/home_hints.dart';

EntryItem f(String path, int index, {bool isDir = false}) => EntryItem(
      index: index,
      path: path,
      size: isDir ? 0 : 10,
      packed: isDir ? 0 : 10,
      isDir: isDir,
      encrypted: false,
      risky: false,
    );

Preview pv(List<EntryItem> entries) => Preview(
      format: 'zip',
      complete: true,
      totalSize: 10,
      encryptedCount: 0,
      encKind: null,
      entries: entries,
      hints: const [],
    );

void main() {
  group('kindName', () {
    test('认识的格式给人话', () {
      expect(kindName('mp4'), contains('MP4'));
      expect(kindName('zip'), contains('ZIP'));
    });

    test('不认识的说「认不出的文件」，不许空白', () {
      expect(kindName('qqqq'), isNotEmpty);
      expect(kindName('qqqq'), contains('认不出'));
    });
  });

  group('shortPath', () {
    test('三级以上只留最后两级', () {
      expect(shortPath('/home/a/b/c/d.zip'), '…/c/d.zip');
    });

    test('两级以内原样返回', () {
      expect(shortPath('c/d.zip'), 'c/d.zip');
      expect(shortPath('d.zip'), 'd.zip');
    });

    test('★ Windows 反斜杠路径也要能缩短（原来只认 / ，整条原样显示）', () {
      final s = shortPath(r'C:\Users\me\下载\游戏.zip');
      expect(s, contains('…'), reason: '不缩短的话 AppBar 只能靠省略号硬截，「哪个文件夹」这个信息就没了');
      expect(s, contains('游戏.zip'));
    });
  });

  group('entryHint', () {
    test('没有预览时不给提示', () {
      expect(entryHint(null), isNull);
    });

    test('找到 setup.exe 就指它', () {
      expect(
        entryHint(pv([f('Setup.exe', 1), f('readme.txt', 2)])),
        'Setup.exe',
      );
    });

    test('目录不算启动文件', () {
      expect(
        entryHint(pv([f('setup', 1, isDir: true)])),
        isNull,
        reason: '指到一个目录上，用户点进去还是得自己找',
      );
    });

    test('一个像样的都没有就返回 null（宁可不指路，不瞎指）', () {
      expect(entryHint(pv([f('a.txt', 1), f('b.jpg', 2)])), isNull);
    });
  });

  group('typeHint（失败时那句「这文件其实是…」）', () {
    test('没有错误信息时不说', () {
      expect(typeHint(null), isNull);
      expect(typeHint(EngineError('BAD', 'x')), isNull,
          reason: 'detail 空时什么都不知道，瞎猜比不说更糟');
    });

    test('0 字节 → 空文件，且不是警告色', () {
      final h = typeHint(EngineError('BAD', 'x', {'size': '0'}));
      expect(h!.text, contains('空文件'));
      expect(h.warn, isFalse);
    });

    test('★ 名字是 mp4、内容是 zip → 说「很可能就是伪装包」，但不是警告', () {
      final h = typeHint(EngineError('BAD', 'x', {
        'looks_like': 'zip',
        'ext': 'mp4',
        'ext_mismatch': 'true',
        // ★ 必须给 looks_kind: 'archive' —— 不给的话走的是「可能被改过名」那条
        //   （中性说法），只有认出来是压缩包才敢说「很可能就是伪装包」
        'looks_kind': 'archive',
        'size': '1024',
      }));
      expect(h!.text, contains('伪装包'));
      expect(h.warn, isFalse, reason: '伪装包是好消息（能解），染红会吓人');
    });

    test('★ 名字是 mp4、内容是 ELF 程序 → 必须警告', () {
      final h = typeHint(EngineError('BAD', 'x', {
        'looks_like': 'elf',
        'ext': 'mp4',
        'ext_mismatch': 'true',
        'looks_kind': 'program',
      }));
      expect(h!.warn, isTrue, reason: '来路不明的程序要提醒别运行，这是唯一该红的情况');
      expect(h.text, contains('别运行'));
    });

    test('认不出的文件也要说清可能原因', () {
      final h = typeHint(EngineError('BAD', 'x', {'looks_like': 'unknown'}));
      expect(h!.text, contains('认不出'));
      expect(h.text, contains('加密'), reason: '「可能被加密过」是用户最容易自己解决的那条');
    });
  });

  group('HomeAppBar', () {
    Future<void> pumpBar(
      WidgetTester tester, {
      String? file,
      bool canPick = true,
      VoidCallback? onPick,
    }) async {
      tester.view.physicalSize = const Size(1200, 200);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          appBar: HomeAppBar(
            file: file,
            canPick: canPick,
            onPick: onPick ?? () {},
          ),
        ),
      ));
      await tester.pump();
    }

    testWidgets('有文件时标题旁边显示压短后的路径', (tester) async {
      await pumpBar(tester, file: '/home/a/b/c/游戏.zip');
      expect(find.text('杨三海解压'), findsOneWidget);
      expect(find.textContaining('游戏.zip'), findsOneWidget);
      expect(find.textContaining('…/'), findsOneWidget);
    });

    testWidgets('没文件时不显示路径那一截', (tester) async {
      await pumpBar(tester);
      expect(find.textContaining('…/'), findsNothing);
    });

    testWidgets('解压中不许再选文件（按钮禁用）', (tester) async {
      await pumpBar(tester, canPick: false);
      final btn = tester.widget<IconButton>(find.byType(IconButton));
      expect(btn.onPressed, isNull,
          reason: '解压中途换文件会让状态机错乱，只能等这一轮结束');
    });

    testWidgets('能选时点了报父层', (tester) async {
      var picks = 0;
      await pumpBar(tester, onPick: () => picks++);
      await tester.tap(find.byType(IconButton));
      await tester.pump();
      expect(picks, 1);
    });
  });
}
