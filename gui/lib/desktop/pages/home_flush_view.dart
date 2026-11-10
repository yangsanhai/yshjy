// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 「正在等系统把临时文件落盘」那一屏 —— 从 `home_page.dart` 抽出来的第 6 块 A 段。
///
/// 这一屏只在**解压完、但磁盘还在慢慢写**时出现（机械盘上尤其明显，用户报过
/// 「解压完了但文件打不开」，其实是页缓存还没落盘）。
///
/// 进度条用 `value: frac`（不是无限转圈）—— 用户在等一件「有尽头的事」，
/// 转圈会让人以为卡死了。`peak <= 0` 时退回成不确定进度条（那时真算不出比例）。
library;

import 'package:flutter/material.dart';

import 'package:yshjy_gui/common/format.dart';
import 'package:yshjy_gui/desktop/key_action_scope.dart';
import 'package:yshjy_gui/desktop/keyboard.dart';
import 'package:yshjy_gui/common/l10n.dart';

class FlushView extends StatelessWidget {
  const FlushView({
    super.key,
    required this.peak,
    required this.pending,
    required this.secs,
    required this.onCancel,
  });

  /// 开始落盘时有多少待写字节（用来算比例，`<= 0` 表示算不出来）
  final int peak;

  /// 现在还剩多少没写完
  final int pending;

  /// 已经等了多久（秒）
  final int secs;

  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final frac =
        peak <= 0 ? null : (1 - (pending / peak)).clamp(0.0, 1.0);
    final content = Center(
      key: const ValueKey('flush'),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(t('home.flushing'),
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(
              t('home.flushExplain'),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 16),
            ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: LinearProgressIndicator(
                value: frac,
                minHeight: 10,
                // T11：同上 —— 念出「在等落盘」+ 具体百分比
                semanticsLabel: t('home.flushing'),
                semanticsValue:
                    frac == null ? null : '${(frac * 100).floor()}%',
              ),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: Text(
                    t('home.flushRemaining', {'size': formatBytes(pending)}),
                    style: const TextStyle(fontSize: 12),
                  ),
                ),
                Text(
                  t('home.flushWaited', {'secs': '$secs'}),
                  style: const TextStyle(fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              t('home.flushTips'),
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 20),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                OutlinedButton.icon(
                  onPressed: onCancel,
                  icon: const Icon(Icons.stop_circle_outlined),
                  label: Text(t('home.stopWaiting')),
                ),
              ],
            ),
          ],
        ),
      ),
    );
    // Esc = 取消当前操作（UI-SPEC §4b.6）。这一屏只有「停止等待」一个动作，
    // 所以 Esc 就给它 —— 用户在等落盘时最想干的事就是「别等了」。
    return KeyActionScope(
      handlers: {KeyAction.cancel: onCancel},
      child: content,
    );
  }
}
