// 界面测试的「别动用户真配置」保险丝 —— **唯一实现**，别在每个测试里各写一份。
//
// 为什么收成一份：2026-10-31 实测翻过一次车 ——
//   · `app_settings_test.dart` 判的是「configDir 就是 ~/.config/yshjy 且没设 XDG_CONFIG_HOME」
//     （合理：只要被指到临时目录就放行）
//   · `settings_switch_widget_test.dart` 判的是「路径里必须含字符串 `yshjy-test`」
//     （过严：`tests/all.sh` 用的是 `yshjy-all-cfg`，于是整组 setUpAll 直接炸）
// 两套判断不一样，等于**同一个规则手抄了两份**，跑全量回归才撞出来。
// 现在统一走这里：只关心「是不是用户真实配置目录」，不关心临时目录叫什么名字。

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/platform_paths.dart';

/// 在会读写 `config.toml` 的测试的 `setUp` / `setUpAll` 里调一次。
///
/// 判定：`XDG_CONFIG_HOME` 没设（或为空）**并且** `configDir` 就落在用户真实配置目录上
/// → 直接 fail。只要被指到任何一个临时目录就放行（临时目录叫什么都行）。
void requireIsolatedConfigDir(String what) {
  final xdg = Platform.environment['XDG_CONFIG_HOME'];
  // ★ Dart 不认中文标识符（踩第 N 次）：变量名一律英文，中文只放字符串里
  final realConfig = '$homeDir${Platform.pathSeparator}.config${Platform.pathSeparator}yshjy';
  if ((xdg == null || xdg.isEmpty) && configDir == realConfig) {
    fail(
      '$what 会读写 config.toml，必须在隔离的 XDG_CONFIG_HOME 下跑'
      '（拒绝动用户真实配置：$configDir）\n'
      '  例：XDG_CONFIG_HOME=/tmp/yshjy-test-config flutter test',
    );
  }
}

/// 会读写 `stateDir`（历史记录 GUI 历史 / 崩溃日志）的测试在 `setUp` 里调一次。
///
/// 为什么单独一条：历史页测试要**造**几条记录才能验排序，而 `HistoryStore` 的
/// 落点是 `stateDir/gui-history.json` —— 指到用户真实目录就是直接改他的历史。
/// 和 config 那条同一个思路：只要不是真实目录（临时目录叫什么都行）就放行。
void requireIsolatedStateDir(String what) {
  final xdg = Platform.environment['XDG_STATE_HOME'];
  final realState = '$homeDir${Platform.pathSeparator}.local'
      '${Platform.pathSeparator}state${Platform.pathSeparator}yshjy';
  if ((xdg == null || xdg.isEmpty) && stateDir == realState) {
    fail(
      '$what 会读写 stateDir，必须在隔离的 XDG_STATE_HOME 下跑'
      '（拒绝动用户真实历史记录：$stateDir）\n'
      '  例：XDG_STATE_HOME=/tmp/yshjy-test-state flutter test',
    );
  }
}
