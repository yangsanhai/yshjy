import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/notify.dart';

/// 通知是「锦上添花」：坏掉不许拖垮主流程。
/// 这里只测最关键的一条：**命令不存在时必须静默返回 false，不能抛异常**。
void main() {
  test('notify-send 不存在时不抛异常、返回 false', () async {
    Notifier.command = 'definitely-not-a-real-command-yshjy-test';
    final ok = await Notifier.send('测试', '这条不该弹出来');
    expect(ok, isFalse);
  }, skip: !Platform.isLinux ? '本批只做 Linux 通知' : false);

  test('开关关掉时直接返回 false，根本不调命令', () async {
    Notifier.command = 'definitely-not-a-real-command-yshjy-test';
    Notifier.enabled = false;
    final ok = await Notifier.send('测试', '关掉时不该尝试');
    expect(ok, isFalse);
    Notifier.enabled = true;
  }, skip: !Platform.isLinux ? '本批只做 Linux 通知' : false);
}
