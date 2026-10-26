// 托盘通道的测试
//
// 托盘是「锦上添花」：坏掉不许拖垮主流程。所以这里的重点不是「能不能藏窗口」，
// 而是：
// 1. 没有原生实现时（widget 测试、非 Linux 平台）**不许抛异常**
// 2. 真的发出去了的时候，方法名和参数必须跟 C++ 侧对得上
//    （gui/linux/runner/my_application.cc 里的 "yshjy/tray"）
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/tray.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('没有原生实现时 hideWindow 不抛异常', () async {
    // 通道不存在 → MissingPluginException。Tray 必须自己吃掉。
    await expectLater(Tray.hideWindow(), completes);
  });

  test('没有原生实现时 setStatus 不抛异常', () async {
    await expectLater(Tray.setStatus('测试'), completes);
  });

  test('setStatus 把 {"text": ...} 发到 yshjy/tray 通道', () async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('yshjy/tray'),
            (MethodCall call) async {
      calls.add(call);
      return null;
    });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(const MethodChannel('yshjy/tray'), null);
    });

    await Tray.setStatus('正在解压 42%');

    expect(calls, hasLength(1), reason: '应该正好发一次调用');
    expect(calls.single.method, 'set_status',
        reason: '方法名必须和 C++ 侧 my_application.cc 里的分支一致');
    final args = calls.single.arguments;
    expect(args, isA<Map<Object?, Object?>>(),
        reason: '标准 codec 解出来的 map 键值类型是 Object?');
    expect((args as Map<Object?, Object?>)['text'], '正在解压 42%',
        reason: '参数键名必须是 "text"，C++ 侧按 "text" 取值');
  });

  test('hideWindow 发的方法名是 hide_window', () async {
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('yshjy/tray'),
            (MethodCall call) async {
      calls.add(call);
      return null;
    });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(const MethodChannel('yshjy/tray'), null);
    });

    await Tray.hideWindow();

    expect(calls, hasLength(1));
    expect(calls.single.method, 'hide_window',
        reason: 'C++ 侧按 "hide_window" 分支处理藏窗口');
  });
}
