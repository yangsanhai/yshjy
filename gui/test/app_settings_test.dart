import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/app_settings.dart';
import 'package:yshjy_gui/platform_paths.dart';

/// 「解压完自动打开输出目录」这个开关的读取规则（`[general] open_after`）
///
/// 跑法要注意：`configDir` 取 `XDG_CONFIG_HOME`，所以这个测试**必须**在隔离的
/// 配置目录下跑（`tests/all.sh` 会设），否则会去读写用户真实的 config.toml。
void main() {
  File cfg() => File('${configDir}/config.toml');

  setUp(() {
    // 安全闸：万一跑在真实配置目录上，直接跳过而不是动用户的配置
    if (configDir == '$homeDir/.config/yshjy' &&
        Platform.environment['XDG_CONFIG_HOME']?.isNotEmpty != true) {
      fail('这个测试必须在隔离的 XDG_CONFIG_HOME 下跑，拒绝动用户真实配置');
    }
    final f = cfg();
    f.parent.createSync(recursive: true);
    if (f.existsSync()) f.deleteSync();
  });

  tearDown(() {
    final f = cfg();
    if (f.existsSync()) f.deleteSync();
  });

  test('没有配置文件 → 默认关（不弹窗口）', () async {
    expect(await openAfterExtractEnabled(), isFalse);
  });

  test('配置里没写这个键 → 默认关', () async {
    cfg().writeAsStringSync('[general]\ntheme = "auto"\nnotify = "on"\n');
    expect(await openAfterExtractEnabled(), isFalse);
  });

  test('open_after = "on" → 开', () async {
    cfg().writeAsStringSync('[general]\nopen_after = "on"\n');
    expect(await openAfterExtractEnabled(), isTrue);
  });

  test('open_after = "off" → 关', () async {
    cfg().writeAsStringSync('[general]\nopen_after = "off"\n');
    expect(await openAfterExtractEnabled(), isFalse);
  });

  test('键在 [output] 段里不算数（只认 [general]）', () async {
    cfg().writeAsStringSync('[output]\nopen_after = "on"\n');
    expect(await openAfterExtractEnabled(), isFalse);
  });

  test('注释掉的键不算数', () async {
    cfg().writeAsStringSync('[general]\n# open_after = "on"\n');
    expect(await openAfterExtractEnabled(), isFalse);
  });

  test('配置写坏了也不炸（当关着）', () async {
    cfg().writeAsStringSync('这不是 toml\n[general\nopen_after\n');
    expect(await openAfterExtractEnabled(), isFalse);
  });
}
