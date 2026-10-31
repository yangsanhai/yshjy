import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/platform_paths.dart';

/// 「下载」目录要跨平台找得到（T7 / E6 / P1-3）
///
/// 以前 Windows 分支只试 `Downloads`，中文 Windows 上用户目录里叫「下载」，
/// 必然落空 → 退回**家目录**，诊断包混在家目录里，用户还是难找。
/// 这里断言的是**候选顺序**（纯函数），真机存在与否由 `downloadsDir` 判断。
///
/// 注意：变量名必须 ASCII —— Dart 不接受中文标识符（会报 Illegal character）。
void main() {
  test('Windows：先中文「下载」，再 Downloads', () {
    expect(
      downloadDirCandidates(true, home: r'C:\Users\someone'),
      [r'C:\Users\someone\下载', r'C:\Users\someone\Downloads'],
    );
  });

  test('Windows 用反斜杠拼路径（正斜杠在资源管理器地址栏里不通用）', () {
    final candidates = downloadDirCandidates(true, home: r'C:\Users\someone');
    for (final d in candidates) {
      expect(d.contains('/'), isFalse, reason: 'Windows 候选里不该出现正斜杠: $d');
    }
  });

  test('Linux/macOS：先中文「下载」，再 Downloads，用正斜杠', () {
    expect(
      downloadDirCandidates(false, home: '/home/someone'),
      ['/home/someone/下载', '/home/someone/Downloads'],
    );
  });

  test('真机上 downloadsDir 一定返回一个存在的目录（兜底是家目录）', () {
    final d = downloadsDir;
    expect(Directory(d).existsSync(), isTrue,
        reason: '找不到「下载」时应当退回存在的家目录，而不是给一个不存在的路径: $d');
  });
}
