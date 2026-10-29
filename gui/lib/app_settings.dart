import 'dart:io';

import 'platform_paths.dart';

/// 界面侧的少量偏好，借 `config.toml` 的 `[general]` 段一起存
/// —— 不另起一份 state 文件，用户在设置页看到的就是唯一真源。
///
/// 为什么这些键归界面而不是引擎：它们是**界面行为**（要不要替你弹窗口），
/// 引擎不该做、也不该在无界面/脚本场景里做。引擎读配置对未知字段是宽容的
/// （逐字段解析），所以多一个 `open_after` 不会让任何版本报错。
///
/// 当前只有一个键：`[general] open_after = "on"/"off"`（默认 off）。
/// 场景说明写在设置页的副标题里：批量解压时自动开窗口是灾难，所以默认关。
Future<bool> openAfterExtractEnabled() async {
  try {
    final f = File('$configDir/config.toml');
    if (!await f.exists()) return false;
    var inGeneral = false;
    for (final line in (await f.readAsString()).split('\n')) {
      final t = line.trim();
      if (t.startsWith('[') && t.endsWith(']')) {
        inGeneral = t == '[general]';
        continue;
      }
      if (!inGeneral || t.isEmpty || t.startsWith('#')) continue;
      final i = t.indexOf('=');
      if (i <= 0) continue;
      if (t.substring(0, i).trim() != 'open_after') continue;
      var v = t.substring(i + 1).trim();
      if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) {
        v = v.substring(1, v.length - 1);
      }
      return v == 'on';
    }
    return false;
  } catch (_) {
    // 配置读不到/写坏 → 当作「关着」：宁可不弹窗口，也不要因为配置异常炸界面
    return false;
  }
}
