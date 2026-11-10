// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! FFI 传输的真跑测试（**在 Windows 上验，用同一份 C ABI 编的 dll**）
//!
//! 为什么这么验：安卓的 `libyshjy.so` 要在真机上才跑得起来，但**桥接层**可以先在电脑上验通 ——
//! 同一个 `ffi` crate 编成 Windows dll 后，`FfiTransport` 走的路径与手机上完全一样：
//! `DynamicLibrary.open` → `job_submit` + 回调 → 另起 isolate 里 `job_wait` → 结果当一行 JSON 发出去。
//!
//! 这样手机一到就只剩「装得上、打得开」这一件事，桥接的坑提前在电脑上踩掉。
//!
//! 前置：`cargo build --manifest-path ffi\Cargo.toml --release`
//! 没有 dll 时这两个用例会**跳过**（不是失败）—— 不给没装 Rust 的人添红。

@TestOn('windows')
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/transport/ffi_transport.dart';

const _dllPath = r'E:\yshjy-1.0.0-20261108\ffi\target\release\yshjy_ffi.dll';

/// 一个真包（移动硬盘上的探针样本；没有就跳过依赖它的那条用例）
const _sampleZip = r'E:\yshjy-探针-打包-230401.zip';

void main() {
  test('FFI 传输：hello 能拿到 ok 与协议版本（含回调/isolate 等待链路）', () async {
    if (!File(_dllPath).existsSync()) {
      markTestSkipped('没有 dll —— 先 cargo build --manifest-path ffi\\Cargo.toml --release');
      return;
    }
    final t = FfiTransport(library: _dllPath);
    final lines = <String>[];
    final firstLine = Completer<String>();
    t.stdoutLines.listen((l) {
      lines.add(l);
      if (!firstLine.isCompleted) firstLine.complete(l);
    });

    await t.start();
    expect(t.engineVersion, contains('1.0.0'), reason: 'yshjy_version 应带版本号');
    await t.writeLine(jsonEncode({'cmd': 'hello', 'id': 1}));

    final first = await firstLine.future.timeout(const Duration(seconds: 30));
    expect(first, contains('"ok":true'), reason: '引擎返回：$first');
    final decoded = jsonDecode(first) as Map<String, dynamic>;
    expect(decoded['result'], isNotNull, reason: 'hello 的 result 不能是空');
    t.kill();
  }, timeout: const Timeout(Duration(seconds: 60)));

  test('FFI 传输：list 一个真包，能列出包内文件', () async {
    if (!File(_dllPath).existsSync()) {
      markTestSkipped('没有 dll');
      return;
    }
    if (!File(_sampleZip).existsSync()) {
      markTestSkipped('没有样本包 $_sampleZip');
      return;
    }
    final t = FfiTransport(library: _dllPath);
    final done = Completer<String>();
    t.stdoutLines.listen((l) {
      // list 的最终响应：带 result.files 或 ok:true
      if (l.contains('"ok":true') && !done.isCompleted) done.complete(l);
    });
    await t.start();
    await t.writeLine(jsonEncode({'cmd': 'list', 'id': 2, 'path': _sampleZip}));

    final line = await done.future.timeout(const Duration(seconds: 60));
    expect(line, contains('"ok":true'), reason: '实际返回：$line');
    expect(line.toLowerCase(), anyOf(contains('打包输入'), contains('files')),
        reason: 'list 应列出包内条目：$line');
    t.kill();
  }, timeout: const Timeout(Duration(seconds: 90)));
}
