// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 界面文案的多语言（T14）
///
/// # 设计取舍（写清楚，下一轮别推翻重来）
///
/// **不引 `flutter_localizations` / `intl`**。那是官方 gen_l10n 路线：要 ARB 文件 +
/// 代码生成，占位符语法（`{count, plural, ...}`）跟引擎语言包（`{name}`）是两套，
/// 翻译的人得学两种写法。本项目引擎已经自带 7 个 `locales/*.toml`，界面再开一套
/// 等于同一句文案维护两处。所以：文案放 `assets/l10n/<code>.json`，**段名与键名
/// 跟引擎对齐**（`段.键` + `{占位符}`），贡献文档只需讲一次「复制文件、改 _meta、
/// 翻右半边」。
///
/// 代价：缺 key **不会在编译期报错**。用 `gui/test/l10n_test.dart` 的全量比对补回来
/// —— zh-CN 是基准，其它语种缺一条、多一条、空一条、占位符少一个都当场红。
///
/// 运行时缺 key 的行为：先回退 zh-CN，再查不到就把 key 原样显示（界面里出现
/// `about.intro` 这种字样就是漏翻了，比显示空白好排查，也不会把裸 key 当文案发给用户
/// —— 这条和 `humanize.dart` 的「查不到的码不显示原始码」是两种权衡：那里是给用户的
/// 报错文案，这里是开发者该看出来的漏翻）。
///
/// ⚠️ Dart 的标识符**只能是 ASCII**（同 Rust/bash 比它最严），所以本文件里
/// 函数名与变量名一律英文，只有注释和字符串是中文。
library;

import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart' show rootBundle;

/// 基准语言：任何一条文案，它必须有
const String baseLocale = 'zh-CN';

/// 支持的语言，与引擎 `locales/*.toml` 一一对应
const List<String> supportedLocales = [
  'zh-CN',
  'zh-TW',
  'en-US',
  'ja-JP',
  'ko-KR',
  'es-ES',
  'ru-RU',
];

/// **界面文案已经翻完**的语言（`assets/l10n/<code>.json` 真正存在的那些）。
///
/// 跟 [supportedLocales] 不是一回事：那是引擎能输出的语言数，这里是界面自己的进度。
/// 没翻完的语言**一律不能出现在下拉里**，也不能留在配置里 —— 否则会出现两种让用户
/// 以为软件坏了的自相矛盾状态：
///   · 选了「日本語」-> 找不到语言包 -> 界面停在英文（「怎么还是英文」）
///   · 选了「日本語」-> 回落到中文 -> 下拉写着日本語、界面是中文
/// 翻完一种就往这里加一个，并在 [localeNames] 里它已经有了名字。
const List<String> availableLocales = [
  'zh-CN',
  'zh-TW',
  'en-US',
  'ja-JP',
  'ko-KR',
  'es-ES',
  'ru-RU',
];

/// 每种语言在**它自己语言里**的名字（设置页下拉用，永远不翻译）
const Map<String, String> localeNames = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  'en-US': 'English',
  'ja-JP': '日本語',
  'ko-KR': '한국어',
  'es-ES': 'Español',
  'ru-RU': 'Русский',
};

/// 语言设置：`system`（跟随系统）或 [supportedLocales] 里的某个 code。
///
/// 改它会让界面重建（`main.dart` 里像 `themeMode` 一样监听）。
final ValueNotifier<String> uiLocaleSetting = ValueNotifier<String>('system');

/// 生效中的语言（`uiLocaleSetting` 解析后的结果，例如 `system` → `zh-CN`）
String currentLocale = baseLocale;

Map<String, String> _table = {};
Map<String, String> _base = {};

/// 「跟随系统」解析成一个真实语种
///
/// 系统语言的写法五花八门：`zh_CN.UTF-8`、`zh-Hans-CN`、`en_US`、`ja_JP`。
/// 中文额外分繁简：TW / HK / MO / Hant 走繁体，其余简体（这是**唯一**一处要按地区
/// 分开的地方，别的语言按主语言配对就够）。
String resolveLocale(String setting, String systemLocale) {
  if (setting != 'system') {
    return supportedLocales.contains(setting) ? setting : baseLocale;
  }
  // 去掉 `zh_CN.UTF-8` 末尾的编码：不剥掉的话地区会变成 `CN.UTF-8`，
  // 繁简判断永远走不到 —— `zh_TW.UTF-8` 会被当成简体
  final normalized = systemLocale.trim().replaceAll('_', '-').split('.').first;
  if (normalized.isEmpty) return baseLocale;
  for (final code in supportedLocales) {
    if (normalized.toLowerCase() == code.toLowerCase()) return code;
  }
  final parts = normalized.split('-');
  final primary = parts.first.toLowerCase();
  if (primary == 'zh') {
    final region = (parts.length > 1 ? parts[1] : '').toUpperCase();
    if (region == 'TW' || region == 'HK' || region == 'MO' || region.startsWith('HANT')) {
      return 'zh-TW';
    }
    return 'zh-CN';
  }
  for (final code in supportedLocales) {
    if (code.split('-').first.toLowerCase() == primary) return code;
  }
  return baseLocale;
}

/// 某一份语言包（扁平化：`_meta` 这类下划线开头的元数据不进文案表）
Future<Map<String, String>> loadPack(String code) async {
  final raw = await rootBundle.loadString('assets/l10n/$code.json');
  final parsed = jsonDecode(raw) as Map<String, dynamic>;
  return {
    for (final e in parsed.entries)
      if (!e.key.startsWith('_') && e.value is String) e.key: e.value as String,
  };
}

/// 启动时调一次：读基准包 + 目标语言的包
///
/// 目标语言的包读坏了**不让界面变一堆 key**：退回基准语言继续启动
/// （`currentLocale` 也跟着改成基准，否则设置页会显示「英文」而界面是中文）。
/// 基准包自己读不出来才让异常冒出来 —— 那是安装包坏了，没什么可救的。
Future<void> initLocale(String setting) async {
  _base = await loadPack(baseLocale);
  var target = resolveLocale(setting, Platform.localeName);
  var effective = setting;
  if (!availableLocales.contains(target)) {
    // 配置里存着一个还没翻完的语言（手改的、或旧版本留下的）：界面用基准语言，
    // 并把这个值当成「跟随系统」，免得设置页写着「日本語」而界面是中文。
    target = baseLocale;
    effective = 'system';
  }
  if (target == baseLocale) {
    _table = _base;
    currentLocale = baseLocale;
  } else {
    try {
      _table = await loadPack(target);
      currentLocale = target;
    } catch (_) {
      _table = _base;
      currentLocale = baseLocale;
      effective = 'system';
    }
  }
  uiLocaleSetting.value = effective;
}

/// 设置页切语言：先换表，再通知界面重建
///
/// 目标语言**没有语言包**（T14 还没翻到它）或包读坏了，一律回落到基准语言，
/// 并且把 `uiLocaleSetting` 也改成基准 —— 绝不能「保持现状」：那样用户点一下
/// 「日本語」，界面停在原来的语言上（之前就是这样：从英文点日本語，界面一直是英文，
/// 看起来像切换失灵）。宁可明确变成中文，也不要静止不动。
Future<void> switchLocale(String setting) async {
  var target = resolveLocale(setting, Platform.localeName);
  var effective = setting;
  if (!availableLocales.contains(target)) {
    target = baseLocale;
    effective = baseLocale;
  }
  Map<String, String> next;
  if (target == baseLocale) {
    next = _base;
  } else {
    try {
      next = await loadPack(target);
    } catch (_) {
      // 语言包文件坏了（不是没做）：退回基准，别把界面切成一堆 key
      target = baseLocale;
      effective = baseLocale;
      next = _base;
    }
  }
  _table = next;
  currentLocale = target;
  uiLocaleSetting.value = effective;
}

/// 取一条文案：`t('about.report.make')`，带占位符时
/// `t('about.report.path', {'path': '/x/y.zip'})`。
///
/// 占位符写法与引擎语言包一致：`{name}`。参数里没给的同名占位符**原样留着**
/// （宁可界面上露出 `{path}`，也不要静默变成空串 —— 那种 bug 最难发现）。
String t(String key, [Map<String, String>? args]) {
  final text = _table[key] ?? _base[key];
  if (text == null) return key;
  if (args == null || args.isEmpty) return text;
  var result = text;
  for (final e in args.entries) {
    result = result.replaceAll('{${e.key}}', e.value);
  }
  return result;
}

/// 测试用：直接装两张表，不碰 `rootBundle`
/// （`flutter test` 里读 asset 要另配，纯逻辑测试不该被它拖住）
@visibleForTesting
void installTables({
  required Map<String, String> table,
  Map<String, String>? base,
  String locale = 'zh-CN',
}) {
  _table = table;
  _base = base ?? table;
  currentLocale = locale;
}
