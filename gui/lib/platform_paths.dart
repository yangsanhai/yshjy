//! 平台路径与「打开目录」——界面侧唯一的一份
//!
//! 为什么要有它：之前 4 个文件各自写 `$HOME/.local/state/yshjy`，等于把界面
//! 钉死在 Linux 上（Windows/macOS/手机直接错）。与引擎侧 `src/paths.rs` 的规则对齐：
//!
//! | 平台 | 状态目录 |
//! |---|---|
//! | Linux / BSD | `$XDG_STATE_HOME/yshjy`，默认 `~/.local/state/yshjy` |
//! | Windows | `%LOCALAPPDATA%\yshjy` |
//! | macOS / iOS | `~/Library/Application Support/yshjy` |
//! | Android | 由 Flutter 传入的沙箱 `filesDir`（见 `setPlatformStateDir`） |
//!
//! iOS/Android 的目录不靠猜：启动时由原生层调 [setPlatformStateDir] 注入。

import 'dart:io';

String? _overrideStateDir;

/// 原生层（iOS/Android）注入状态目录；桌面不需要调
void setPlatformStateDir(String path) {
  _overrideStateDir = path;
}

/// 用户主目录（Windows 没有 HOME）
String get homeDir {
  final env = Platform.environment;
  return env['HOME'] ?? env['USERPROFILE'] ?? '.';
}

/// 状态目录（崩溃日志 / 历史 / 引导标记）—— 目录可能还不存在
String get stateDir {
  final injected = _overrideStateDir;
  if (injected != null && injected.isNotEmpty) return injected;
  if (Platform.isWindows) {
    final base = Platform.environment['LOCALAPPDATA'] ?? '$homeDir\\AppData\\Local';
    return _join(base, 'yshjy');
  }
  if (Platform.isMacOS || Platform.isIOS) {
    return _join(_join(homeDir, 'Library'), 'Application Support/yshjy');
  }
  final xdg = Platform.environment['XDG_STATE_HOME'];
  if (xdg != null && xdg.isNotEmpty && xdg.startsWith('/')) {
    return _join(xdg, 'yshjy');
  }
  return _join(_join(_join(homeDir, '.local'), 'state'), 'yshjy');
}

/// 配置目录（与引擎 `config_dir()` 一致）
String get configDir {
  if (Platform.isWindows) {
    final base = Platform.environment['APPDATA'] ?? '$homeDir\\AppData\\Roaming';
    return _join(base, 'yshjy');
  }
  if (Platform.isMacOS || Platform.isIOS) {
    return _join(_join(homeDir, 'Library'), 'Application Support/yshjy');
  }
  final xdg = Platform.environment['XDG_CONFIG_HOME'];
  if (xdg != null && xdg.isNotEmpty && xdg.startsWith('/')) {
    return _join(xdg, 'yshjy');
  }
  return _join(_join(homeDir, '.config'), 'yshjy');
}

/// 给用户看的紧凑路径（把主目录换成 `~`）
String prettyPath(String p) {
  final h = homeDir;
  if (h.length > 1 && p.startsWith(h)) return '~${p.substring(h.length)}';
  return p;
}

/// 「下载」目录 —— 诊断包、导出这类「给用户拿走的文件」默认落这里。
///
/// 为什么不落引擎的默认位置：`report` 不给 `out_dir` 时引擎落在**进程当前目录**，
/// GUI 从哪启动就在哪（可能是 `/` 或 `~`），用户回头根本找不着（E6 的病根）。
/// 中文系统叫「下载」、其它叫 `Downloads`，都没有就退回家目录。
String get downloadsDir {
  final candidates = Platform.isWindows
      ? ['$homeDir\\Downloads']
      : ['$homeDir/下载', '$homeDir/Downloads'];
  for (final d in candidates) {
    if (Directory(d).existsSync()) return d;
  }
  return homeDir;
}

/// 在系统文件管理器里打开一个目录（跨平台）
Future<bool> openDirectory(String path) async {
  try {
    if (Platform.isWindows) {
      await Process.run('explorer.exe', [path]);
    } else if (Platform.isMacOS) {
      await Process.run('open', [path]);
    } else {
      await Process.run('xdg-open', [path]);
    }
    return true;
  } catch (_) {
    return false;
  }
}

String _join(String a, String b) {
  final sep = Platform.pathSeparator;
  if (a.endsWith(sep)) return '$a$b';
  return '$a$sep$b';
}
