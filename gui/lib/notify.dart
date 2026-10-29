import 'dart:async';
import 'dart:io';

import 'platform_paths.dart';
import 'tray.dart';

/// 桌面通知（Linux 走系统 `notify-send`）。
///
/// 设计原则：**通知不是主流程**。
/// - 没装 `notify-send` → 静默跳过，不报错
/// - 发得慢 / 超时 → 2 秒放弃，不阻塞界面
/// - 非 Linux 平台 → 直接跳过（本批只做 Linux）
/// - 用户关了开关 → 不发
class Notifier {
  /// 通知命令名（Linux 默认 notify-send）。测试时可替换，用来验证「命令不存在」不崩。
  static String command = 'notify-send';

  /// 设置页改动后立刻生效；null = 还没人为指定，按配置文件判断。
  static bool? _override;

  static set enabled(bool v) => _override = v;

  /// 读 `[general] notify = "on" | "off"`；读不到就当开启。
  static bool get enabled {
    if (_override != null) return _override!;
    try {
      final f = File('$configDir/config.toml');
      if (f.existsSync()) {
        final m = RegExp(r'^\s*notify\s*=\s*"?(\w+)"?', multiLine: true)
            .firstMatch(f.readAsStringSync());
        if (m != null) return m.group(1) != 'off';
      }
    } catch (_) {
      // 配置坏了不该影响通知以外的任何事
    }
    return true;
  }

  /// 发一条通知。true = 已交给系统（不代表一定显示出来）。
  static Future<bool> send(String title, String body) async {
    if (Platform.isWindows) {
      // Windows 走托盘气球（W9）：图标是原生的，通知挂同一个图标最省事；
      // 没有原生实现（widget 测试/老壳）时 Tray.notify 自己吞异常并返回 false
      if (!enabled) return false;
      return Tray.notify(title, body);
    }
    if (!Platform.isLinux) return false;
    if (!enabled) return false;
    try {
      final r = await Process.run(
        command,
        ['-a', '杨三海解压', '-i', 'yshjy', '-t', '8000', title, body],
      ).timeout(const Duration(seconds: 2));
      return r.exitCode == 0;
    } on ProcessException {
      return false; // 没装 notify-send
    } on TimeoutException {
      return false;
    } catch (_) {
      return false;
    }
  }
}
