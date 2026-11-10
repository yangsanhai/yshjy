// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机端才有意义的两样东西：**输出位置偏好** 与 **前台服务**
//!
//! 输出位置有两档（方案 A / B，见 docs/ANDROID-整体方案 §5）：
//! - `private`（默认）：解到 App 私有目录，零权限，装上就能用；
//! - `download`：直接解到系统「下载」（`/storage/emulated/0/Download`），
//!   需要用户手动开「所有文件访问」—— 所以是个开关，而且要在设置页说清代价。
//!
//! 前台服务：解大包要几分钟，切后台会被系统杀掉，解压期间挂一个 `dataSync` 服务。

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/platform_paths.dart';

class MobilePrefs {
  static const MethodChannel _storage = MethodChannel('yshjy/storage');
  static const MethodChannel _foreground = MethodChannel('yshjy/foreground');

  // ── config.toml 的 [general] 读写（与桌面设置页同一份文件）────────────
  static Future<String?> readGeneral(String key) async {
    try {
      final f = File('$configDir/config.toml');
      if (!await f.exists()) return null;
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
        if (t.substring(0, i).trim() != key) continue;
        var v = t.substring(i + 1).trim();
        if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) {
          v = v.substring(1, v.length - 1);
        }
        return v;
      }
      return null;
    } catch (_) {
      return null;
    }
  }

  /// 只动 `[general] <key>` 这一行，别的行原样保留
  static Future<void> writeGeneral(String key, String value) async {
    final f = File('$configDir/config.toml');
    try {
      await f.parent.create(recursive: true);
      final lines = (await f.exists()) ? (await f.readAsString()).split('\n') : <String>[];
      var inGeneral = false;
      var found = false;
      var hasGeneral = false;
      for (var i = 0; i < lines.length; i++) {
        final t = lines[i].trim();
        if (t.startsWith('[') && t.endsWith(']')) {
          inGeneral = t == '[general]';
          if (inGeneral) hasGeneral = true;
          continue;
        }
        if (!inGeneral) continue;
        final eq = t.indexOf('=');
        if (eq <= 0) continue;
        if (t.substring(0, eq).trim() == key) {
          lines[i] = '$key = "$value"';
          found = true;
          break;
        }
      }
      if (!found) {
        if (!hasGeneral) lines.add('[general]');
        final idx = lines.indexWhere((l) => l.trim() == '[general]');
        if (idx >= 0) {
          lines.insert(idx + 1, '$key = "$value"');
        } else {
          lines.add('$key = "$value"');
        }
      }
      await f.writeAsString(lines.join('\n'));
    } catch (_) {
      // 写不进去不拦操作：这次的选择照样立刻生效
    }
  }

  // ── 输出位置 ─────────────────────────────────────────────────────
  static const String kOutKey = 'android_out';

  /// 用户**想要**解到系统下载（配置文件里的意愿，不代表权限到位）
  static Future<bool> outToDownloadWanted() async => (await readGeneral(kOutKey)) == 'download';

  static Future<void> setOutToDownload(bool v) => writeGeneral(kOutKey, v ? 'download' : 'private');

  /// 真正生效中的状态（意愿 + 权限都满足）。
  ///
  /// 为什么做成 [ValueNotifier]：首页在 `IndexedStack` 里常驻，切换页签不会重建，
  /// 用户在设置页一开开关，首页必须**立刻**跟着改解压位置 —— 靠 initState 里读一次是不够的。
  static final ValueNotifier<bool> outToDownloadActive = ValueNotifier<bool>(false);

  /// 系统「下载」目录（生效时才非空）
  static String? downloadRootPath;

  /// 重新算一次「意愿 + 权限」，并通知界面
  static Future<void> refreshOut() async {
    final want = await outToDownloadWanted();
    final has = want ? await hasAllFiles() : false;
    final root = has ? await downloadRoot() : null;
    downloadRootPath = root;
    outToDownloadActive.value = has && root != null;
  }

  /// 系统「下载」目录（`/storage/emulated/0/Download`）
  static Future<String?> downloadRoot() async {
    try {
      return await _storage.invokeMethod<String>('downloadPath');
    } catch (_) {
      return null;
    }
  }

  // ── 「所有文件访问」权限 ──────────────────────────────────────────
  static Future<bool> hasAllFiles() async {
    try {
      return await _storage.invokeMethod<bool>('hasAllFiles') ?? false;
    } catch (_) {
      return false;
    }
  }

  /// 跳系统设置页让用户自己勾（这条权限**没法用弹窗申请**，只能跳页）
  static Future<void> requestAllFiles() async {
    try {
      await _storage.invokeMethod('requestAllFiles');
    } catch (_) {}
  }

  // ── 前台服务 ─────────────────────────────────────────────────────
  static Future<void> startForeground({required String title, required String text}) async {
    try {
      await _foreground.invokeMethod('start', {'title': title, 'text': text});
    } catch (_) {}
  }

  static Future<void> stopForeground() async {
    try {
      await _foreground.invokeMethod('stop');
    } catch (_) {}
  }
}
