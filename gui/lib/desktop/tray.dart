// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'package:flutter/services.dart';

/// 托盘通道：和 gui/linux/runner/my_application.cc 里的 "yshjy/tray" 对应。
///
/// 干什么用的：托盘图标本身是 C++（GTK + libayatana-appindicator）画的，
/// Flutter 够不着 GTK 的 widget，所以开了条 MethodChannel 让 Dart 指挥原生：
/// - [hideWindow]：把主窗口藏起来，进程和引擎照跑（解压继续）
/// - [setStatus]：把底部状态栏的文字同步到托盘菜单的状态行
/// - [onQuitRequest]：接住原生侧的「用户点了退出」，由 Dart 决定何时真退
/// - [quitNow]：Dart 收拾好了，让原生真的退出进程
///
/// 为什么异常必须全吃掉：
/// 1. widget 测试里没有原生实现，invokeMethod 会抛 MissingPluginException；
/// 2. 非 Linux 平台（或者将来换壳）根本没有这个通道；
/// 3. 托盘只是「锦上添花」，它坏掉绝不能拖垮主流程 —— 底部状态栏才是
///    真正给用户看的，托盘状态行只是同一份信息的第二个出口。
/// 所以这里一律静默吞掉，让调用方（app_shell）可以放心地 fire-and-forget。
class Tray {
  Tray._();

  static const MethodChannel _ch = MethodChannel('yshjy/tray');

  /// 把窗口藏起来（进程和引擎继续跑）。
  ///
  /// 调用方一般不 await 结果 —— 藏没藏成功都不影响解压本身。
  static Future<void> hideWindow() async {
    try {
      await _ch.invokeMethod<void>('hide_window');
    } catch (_) {
      // 没有原生实现 / 通道断了：忽略，见上面的说明
    }
  }

  /// 把底部状态栏的文字同步到托盘菜单的状态行。
  static Future<void> setStatus(String text) async {
    try {
      await _ch.invokeMethod<void>('set_status', <String, dynamic>{
        'text': text,
      });
    } catch (_) {
      // 同上
    }
  }

  /// 接住原生侧的「用户点了退出」，交给 [cb] 决定什么时候真退。
  ///
  /// 为什么要有这一层：引擎是**子进程**，进程一没它读 stdin 就撞 EOF、
  /// 立刻自己退，正在解的那一项被硬中断。所以退出不能由原生单方面决定，
  /// 得先绕回 Dart：让 Dart 取消引擎、给它时间在数据块边界收好尾，
  /// 完了再由 Dart 调 [quitNow] 才真退。
  ///
  /// 传 null 表示撤掉处理器（比如界面被拆了，别让回调打到废对象上）。
  static void onQuitRequest(void Function()? cb) {
    if (cb == null) {
      _ch.setMethodCallHandler(null);
      return;
    }
    _ch.setMethodCallHandler((MethodCall call) async {
      if (call.method == 'request_quit') {
        cb();
      }
      // 原生侧不要回包（它传的 callback 是 nullptr），回什么它都不看
      return null;
    });
  }

  /// 托盘气球通知（Windows 走 Shell_NotifyIcon 的 NIF_INFO；Linux 的通知
  /// 走 notify-send，见 notify.dart —— 两边出口不同，这里只放 Windows 这条）。
  static Future<bool> notify(String title, String body) async {
    try {
      await _ch.invokeMethod<void>('notify', <String, dynamic>{
        'title': title,
        'body': body,
      });
      return true;
    } catch (_) {
      return false;
    }
  }

  /// 让原生真的退出进程（走 GApplication 的正常退出流程）。
  ///
  /// 异常照样吃掉：退不出去也不能因此崩界面 —— 顶多进程还留着。
  static Future<void> quitNow() async {
    try {
      await _ch.invokeMethod<void>('quit_now');
    } catch (_) {
      // 同上
    }
  }

  /// 把 Windows 窗口标题栏刷成和界面同一个深浅色（Linux 没有这条，静默忽略）。
  ///
  /// 为什么要在 Dart 侧刷：标题栏是系统画的，界面切「深色」时它不会自己变，
  /// 黑内容配白标题栏很割裂。原生侧只做 DwmSetWindowAttribute，时机由界面决定。
  static Future<void> setTitleBarDark(bool dark) async {
    try {
      await _ch.invokeMethod<void>('set_titlebar_dark', <String, dynamic>{
        'dark': dark,
      });
    } catch (_) {
      // 同上：非 Windows 壳 / 测试环境没有原生实现
    }
  }
}
