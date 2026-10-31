import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/l10n.dart';

/// 测试用的读包（不用 rootBundle：这里是纯文件比对，`flutter test` 的 cwd 是 gui/）
Map<String, String> readPack(String code) {
  final raw = File('assets/l10n/$code.json').readAsStringSync();
  final m = jsonDecode(raw) as Map<String, dynamic>;
  return {
    for (final e in m.entries)
      if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
  };
}

/// 一条文案里的占位符集合：`"共 {n} 个"` → `{n}`
Set<String> placeholders(String s) =>
    RegExp(r'\{([a-zA-Z_][a-zA-Z0-9_]*)\}').allMatches(s).map((m) => m.group(0)!).toSet();

/// 注意：Dart 标识符只能 ASCII，这里一律英文（只有 reason 里的提示是中文）
void main() {
  group('resolveLocale 语言解析', () {
    test('设置成具体语言就直接用', () {
      expect(resolveLocale('en-US', 'zh_CN.UTF-8'), 'en-US');
      expect(resolveLocale('ja-JP', 'zh_CN.UTF-8'), 'ja-JP');
    });

    test('设置写坏了 → 基准语言（不能变成未知状态）', () {
      expect(resolveLocale('klingon', 'en_US'), baseLocale);
    });

    test('跟随系统：zh_CN.UTF-8 → 简体', () {
      expect(resolveLocale('system', 'zh_CN.UTF-8'), 'zh-CN');
    });

    test('跟随系统：zh-TW / zh_HK / zh-Hant → 繁体', () {
      expect(resolveLocale('system', 'zh_TW.UTF-8'), 'zh-TW');
      expect(resolveLocale('system', 'zh-HK'), 'zh-TW');
      expect(resolveLocale('system', 'zh-Hant-CN'), 'zh-TW');
    });

    test('跟随系统：zh-Hans-CN → 简体（别被地区带偏成繁体）', () {
      expect(resolveLocale('system', 'zh-Hans-CN'), 'zh-CN');
    });

    test('跟随系统：en_US / ja_JP / ru_RU 各归各位', () {
      expect(resolveLocale('system', 'en_US.UTF-8'), 'en-US');
      expect(resolveLocale('system', 'ja_JP'), 'ja-JP');
      expect(resolveLocale('system', 'ru_RU.UTF-8'), 'ru-RU');
    });

    test('跟随系统：不认识的语言 → 基准', () {
      expect(resolveLocale('system', 'fr_FR.UTF-8'), baseLocale);
      expect(resolveLocale('system', ''), baseLocale);
    });
  });

  group('t() 取文案', () {
    setUp(() {
      installTables(
        table: {'a.b': '共 {n} 个文件', 'only.en': 'English only'},
        base: {'a.b': '共 {n} 个文件', 'a.c': '只有中文有', 'only.en': 'English only'},
        locale: 'en-US',
      );
    });

    test('占位符替换', () {
      expect(t('a.b', {'n': '3'}), '共 3 个文件');
    });

    test('同一个占位符出现两次都要换', () {
      installTables(table: {'x': '{a}-{a}'});
      expect(t('x', {'a': 'z'}), 'z-z');
    });

    test('参数没给 → 占位符原样留着（宁可露出来，不要静默变空串）', () {
      expect(t('a.b'), '共 {n} 个文件');
    });

    test('译文缺这条 → 回退基准语言', () {
      expect(t('a.c'), '只有中文有');
    });

    test('两边都没有 → 原样返回 key（界面里一眼看出漏翻）', () {
      expect(t('没有.这个'), '没有.这个');
    });
  });

  group('语言包完整性（漏翻的护栏）', () {
    final files = Directory('assets/l10n')
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.json'))
        .toList();

    test('至少有基准语言和英文两份', () {
      final codes = files.map((f) => f.uri.pathSegments.last.replaceAll('.json', '')).toSet();
      expect(codes.contains('zh-CN'), isTrue, reason: '缺基准语言包');
      expect(codes.contains('en-US'), isTrue, reason: '缺英文包');
    });

    test('每个语种的 key 与基准一一对应（缺/多/空都算错）', () {
      final base = readPack('zh-CN');
      final problems = <String>[];
      for (final f in files) {
        final code = f.uri.pathSegments.last.replaceAll('.json', '');
        if (code == 'zh-CN') continue;
        final pack = readPack(code);
        final missing = base.keys.where((k) => !pack.containsKey(k)).toList();
        final empty = base.keys.where((k) => pack.containsKey(k) && pack[k]!.trim().isEmpty).toList();
        final extra = pack.keys.where((k) => !base.containsKey(k)).toList();
        if (missing.isNotEmpty) problems.add('$code 缺 ${missing.length} 条: ${missing.join(", ")}');
        if (empty.isNotEmpty) problems.add('$code 空 ${empty.length} 条: ${empty.join(", ")}');
        if (extra.isNotEmpty) problems.add('$code 多 ${extra.length} 条: ${extra.join(", ")}');
      }
      expect(problems, isEmpty, reason: problems.join('\n'));
    });

    test('译文里的占位符与基准完全一致（翻译时弄丢 {version} 会当场红）', () {
      final base = readPack('zh-CN');
      final problems = <String>[];
      for (final f in files) {
        final code = f.uri.pathSegments.last.replaceAll('.json', '');
        if (code == 'zh-CN') continue;
        final pack = readPack(code);
        for (final key in base.keys) {
          final text = pack[key];
          if (text == null) continue; // 缺 key 由上面那条报，这里不重复
          final expected = placeholders(base[key]!);
          final actual = placeholders(text);
          if (expected.length != actual.length || !expected.containsAll(actual)) {
            problems.add('$code 的 $key：基准 $expected / 译文 $actual');
          }
        }
      }
      expect(problems, isEmpty, reason: problems.join('\n'));
    });
  });

  test('支持的语言与语言名一一对应（设置页下拉不能出现无名项）', () {
    for (final code in supportedLocales) {
      expect(localeNames.containsKey(code), isTrue, reason: '$code 没有本地名');
    }
    expect(localeNames.length, supportedLocales.length);
  });

  test('可用的语言（有语言包的）只能是支持语言的子集，且每种都真有语言包文件', () {
    expect(availableLocales, contains(baseLocale), reason: '基准语言必须有语言包');
    for (final code in availableLocales) {
      expect(supportedLocales, contains(code), reason: '$code 不在支持列表里，下拉会出现引擎不认的语言');
      expect(File('assets/l10n/$code.json').existsSync(), isTrue,
          reason: '$code 在 availableLocales 里但没有 assets/l10n/$code.json —— '
              '用户选它就只会回落到中文，等于骗人');
    }
    // 反向：有语言包却没进 availableLocales，同样会让「翻完了却选不到」
    for (final code in supportedLocales) {
      final hasPack = File('assets/l10n/$code.json').existsSync();
      expect(hasPack, availableLocales.contains(code),
          reason: '$code 语言包存在=$hasPack，但 availableLocales 里=${availableLocales.contains(code)} —— '
              '两边必须一致，翻完一种就要加进 availableLocales');
    }
  });
}
