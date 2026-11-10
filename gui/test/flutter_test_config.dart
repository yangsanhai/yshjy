// 所有 flutter 测试的统一初始化（`flutter test` 每跑一个测试文件前都会调它）
//
// 为什么需要：T14 起界面文案走 `l10n.t()` 查表，而表是 `main()` 启动时装进去的。
// 测试不走 `main()` → 表是空的 → `t()` 原样返回 key → 界面显示 `err.NEED_PASSWORD`
// 这种字样，一堆断言具体人话的测试会集体变红（而且红得很迷惑：看起来像文案写错了）。
//
// 在这里统一装**基准语言**（中文），各测试文件不用再各写一遍 setUpAll。
// 要测别的语言，在具体测试里再调 `installTables(table: …)` 覆盖即可。
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/platform_paths.dart';

Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  // 每个测试文件一个**独立的** config / state 目录。
  //
  // 为什么：`flutter test` 默认并发跑多个测试文件（同进程、多 isolate，环境变量共享），
  // 三个设置页测试会抢同一个 config.toml —— 2026-11-01 实际撞过：
  // settings_perf 写的 `jobs = 3` 被 settings_switch 读到，断言红得莫名其妙
  // （单跑全绿、全量偶发红）。这里用时间戳生成唯一目录，各文件互不干扰。
  final stamp = DateTime.now().microsecondsSinceEpoch.toRadixString(36).substring(4);
  final base = Platform.environment['TMPDIR'] ?? '/tmp';
  debugConfigDir = '$base/yt-$stamp/cfg';
  setPlatformStateDir('$base/yt-$stamp/state');

  try {
    final raw = File('assets/l10n/$baseLocale.json').readAsStringSync();
    final parsed = jsonDecode(raw) as Map<String, dynamic>;
    installTables(table: {
      for (final e in parsed.entries)
        if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
    });
  } catch (_) {
    // 语言包读不到：让测试照常跑，红在哪条就是哪条的问题（别在初始化里崩）
  }
  await testMain();
}
