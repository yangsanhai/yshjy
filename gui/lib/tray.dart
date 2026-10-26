import 'package:flutter/services.dart';

/// 托盘通道：和 gui/linux/runner/my_application.cc 里的 "yshjy/tray" 对应。
///
/// 干什么用的：托盘图标本身是 C++（GTK + libayatana-appindicator）画的，
/// Flutter 够不着 GTK 的 widget，所以开了条 MethodChannel 让 Dart 指挥原生：
/// - [hideWindow]：把主窗口藏起来，进程和引擎照跑（解压继续）
/// - [setStatus]：把底部状态栏的文字同步到托盘菜单的状态行
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
}
