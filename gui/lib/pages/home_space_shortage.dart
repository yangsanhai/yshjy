// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 「空间不够」那张卡 + 三条出路 —— 从 `home_page.dart` 抽出来的第 8 块 A 段。
///
/// 这一块的历史值得记：**以前这里是死路**。引擎连 `SPACE_SHORTAGE` 事件都不发
/// （全库只有定义、零发射点），界面这段处理代码永远收不到；而 `--force` 界面也一个参数没传。
/// 现在引擎发事件 + 错误 detail 带 `need` / `available`，三个按钮各有真实后端行为：
/// 换位置重解 / 只挑一部分解 / 硬着头皮试。
///
/// 返回 `List<Widget>` 而不是一个 Widget：它是在页面 Column 的 children 里 spread 进去的，
/// 中间自带三个间距（14 / 10 / 4），包成 Column 反而会让间距语义变模糊。
library;

import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n.dart';

/// 空间不足的三个人话数字。
///
/// 引擎在错误的 `detail` 里给 `need` / `available`（字节数，协议里可能是字符串）。
/// 拿不到就退成一句通用建议 —— **不给算错的数字**（CLI 侧同一口径，别一边猜一边不猜）。
String spaceLine(Map<String, dynamic>? detail) {
  int? pick(String k) {
    final v = detail?[k];
    if (v is num) return v.toInt();
    if (v is String) return int.tryParse(v);
    return null;
  }

  final need = pick('need');
  final avail = pick('available');
  if (need == null || avail == null) {
    return t('home.freeSpaceHint');
  }
  final short = need > avail ? need - avail : 0;
  return t('home.noSpaceDetail',
      {'need': formatBytes(need), 'avail': formatBytes(avail), 'short': formatBytes(short)});
}

List<Widget> spaceShortageBlock(
  ColorScheme cs, {
  required String line,
  required bool hasPreview,
  required VoidCallback onChangeDest,
  required VoidCallback onPickPart,
  required VoidCallback onTryAnyway,
}) {
  return [
    const SizedBox(height: 14),
    Container(
      key: const ValueKey('space-shortage'),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: cs.errorContainer.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: cs.error.withValues(alpha: 0.25)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            t('home.noSpace'),
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: cs.error,
            ),
          ),
          const SizedBox(height: 4),
          Text(line, style: const TextStyle(fontSize: 12, height: 1.5)),
        ],
      ),
    ),
    const SizedBox(height: 10),
    Wrap(
      spacing: 8,
      runSpacing: 8,
      alignment: WrapAlignment.center,
      children: [
        OutlinedButton.icon(
          key: const ValueKey('space-change-dest'),
          onPressed: onChangeDest,
          icon: const Icon(Icons.folder_open, size: 18),
          label: Text(t('home.changeLocation')),
        ),
        // 没有预览就没有「挑一部分」可言（用户根本没看到清单）
        if (hasPreview)
          OutlinedButton.icon(
            key: const ValueKey('space-pick-part'),
            onPressed: onPickPart,
            icon: const Icon(Icons.checklist, size: 18),
            label: Text(t('home.extractPartialBtn')),
          ),
        FilledButton.icon(
          key: const ValueKey('space-force'),
          onPressed: onTryAnyway,
          icon: const Icon(Icons.play_arrow, size: 18),
          label: Text(t('home.tryAnyway')),
        ),
      ],
    ),
    const SizedBox(height: 4),
    Text(
      t('home.tryAnywayHint'),
      textAlign: TextAlign.center,
      style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
    ),
  ];
}
