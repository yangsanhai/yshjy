import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/notify.dart';

/// 通知是「锦上添花」：坏掉不许拖垮主流程。
/// PRD 要求「不崩、不阻塞」——两条都要真测到（第二方复核指出原先只测了第一条）。
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

  test('命令卡住（sleep 5）时 2 秒超时返回 false，不阻塞主流程', () async {
    // 造一个真实会卡住的假命令，覆盖「超时分支」
    final dir = await Directory.systemTemp.createTemp('yshjy-notify-');
    final fake = File('${dir.path}/slow-notify');
    await fake.writeAsString('#!/bin/sh\nsleep 5\n');
    await Process.run('chmod', ['+x', fake.path]);

    Notifier.command = fake.path;
    final sw = Stopwatch()..start();
    final ok = await Notifier.send('测试', '这个命令会卡 5 秒');
    sw.stop();

    expect(ok, isFalse, reason: '超时应当返回 false');
    expect(sw.elapsed.inSeconds, lessThan(4),
        reason: '应在 2 秒超时后返回，而不是等满 5 秒（实测 ${sw.elapsed.inMilliseconds}ms）');
  }, skip: !Platform.isLinux ? '本批只做 Linux 通知' : false);
}
