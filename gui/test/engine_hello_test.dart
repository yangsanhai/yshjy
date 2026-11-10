// `EngineClient.hello()` 的「只问一次 + 失败不缓存」回归（2026-11-01 真机撞的）
//
// 现象：用户装了 0.9.46 打开设置页，那块小字写「本机磁盘类型没探到」，
// 而底部状态栏和格式表都正常（一个说引擎 0.9.46、一个列出 12 种格式）。
// 查出来是应用一启动就**三个人同时要 hello**：AppShell（要引擎版本）、
// 设置页（要 hello.disk）、设置页的格式表（要 hello.formats）。
// 三个请求一起打向一个刚刚 fork、还没预热的引擎进程，真机上其中最靠前的那次失败
// —— 失败的那个正好是磁盘建议。
//
// 修法：hello 在进程内**合并成一次**（Single-flight + 缓存），失败**不缓存**。
// 这里把两件事分别钉住，避免以后又被"多加一个 hello 调用"改回去。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';

import 'fake_engine.dart';

void main() {
  test('并发三次 hello 只向引擎发一次命令（能力表一次运行内不变）', () async {
    final fake = FakeEngine();
    installFakeEngine(fake);

    final rs = await Future.wait([
      EngineClient.instance.hello(),
      EngineClient.instance.hello(),
      EngineClient.instance.hello(),
    ]);

    expect(rs.map((r) => r['engine']).toSet(), {'test'});
    expect(
      fake.sent.where((m) => m['cmd'] == 'hello').length,
      1,
      reason: '三个并发调用必须合并成一次（真机上就是这么打的，别退回去）',
    );

    // 再问一次走缓存，仍然只有一次
    await EngineClient.instance.hello();
    expect(fake.sent.where((m) => m['cmd'] == 'hello').length, 1);

    // 显式 refresh 才重问
    await EngineClient.instance.hello(refresh: true);
    expect(fake.sent.where((m) => m['cmd'] == 'hello').length, 2);

    fake.dispose();
  });

  test('第一次 hello 失败 → 失败不缓存，下一次能成功（冷启动自愈）', () async {
    final fake = FakeEngine()..failHelloOnce = true;
    installFakeEngine(fake);

    await expectLater(EngineClient.instance.hello(), throwsA(anything));

    final r = await EngineClient.instance.hello();
    expect(r['engine'], 'test');
    expect(r['disk'], 'rotational');
    expect(fake.sent.where((m) => m['cmd'] == 'hello').length, 2);

    fake.dispose();
  });
}
