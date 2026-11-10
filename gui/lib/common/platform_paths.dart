// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
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

/// **测试专用**：把配置目录指到别处（生产代码永远不设它）。
///
/// 为什么需要：`flutter test` 默认**并发**跑多个测试文件（同一进程、多个 isolate，
/// 环境变量是共享的）。三个设置页测试会同时抢同一个 `$XDG_CONFIG_HOME/yshjy/config.toml`，
/// 2026-11-01 真撞过：`settings_perf` 写进去的 `jobs = 3` 被 `settings_switch` 读到，
/// 断言莫名其妙地红（单跑全绿、全量偶发红 —— 最难查的那种）。
/// 现在由 `test/flutter_test_config.dart` 给每个测试文件生成一个独立目录。
String? debugConfigDir;

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
  // 安卓/iOS：原生注入的沙箱目录（见 setPlatformStateDir）就是配置目录 ——
  // 手机上算不出 XDG，硬算会落到只读的根目录，配置根本写不进去。
  final injectedState = _overrideStateDir;
  if (injectedState != null && injectedState.isNotEmpty && (Platform.isAndroid || Platform.isIOS)) {
    return injectedState;
  }
  // 测试覆盖优先（见 [debugConfigDir] 的说明）
  final injected = debugConfigDir;
  if (injected != null && injected.isNotEmpty) return injected;
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

/// 「下载」目录的候选顺序（由近到远）—— 抽成纯函数才能测：
/// getter 里读的是**真机**路径，测试机上验不出 Windows 分支。
///
/// Windows 上「下载」是 Known Folder 的**本地化显示名**：中文系统里用户目录下
/// 真的叫「下载」，不是 `Downloads`。以前只试 `Downloads`，中文 Windows 上必然
/// 落空 → 退回**家目录**，诊断包混在家目录里，用户还是难找。
List<String> downloadDirCandidates(bool isWindows, {String? home}) {
  final h = home ?? homeDir;
  return isWindows
      ? ['$h\\下载', '$h\\Downloads']
      : ['$h/下载', '$h/Downloads'];
}

/// 「下载」目录 —— 诊断包、导出这类「给用户拿走的文件」默认落这里。
///
/// 为什么不落引擎的默认位置：`report` 不给 `out_dir` 时引擎落在**进程当前目录**，
/// GUI 从哪启动就在哪（可能是 `/` 或 `~`），用户回头根本找不着（E6 的病根）。
/// 中文系统叫「下载」、其它叫 `Downloads`，都没有就退回家目录。
String get downloadsDir {
  for (final d in downloadDirCandidates(Platform.isWindows)) {
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
