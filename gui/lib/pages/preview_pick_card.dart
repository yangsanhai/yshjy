// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 预览页的「封面 / 内容」两张勾选卡 —— 从 `home_page.dart` 抽出来的第 5d-2 第 2 段。
///
/// **为什么是「上下两张卡各带勾选」而不是两个按钮**（GUI-PLAN §5.2①，用户验收原话）：
/// 伪装包里装着两样东西 —— 一个假封面（视频/图片）和真正的压缩包内容。用户要能分别勾：
/// 只勾封面 → 只提取封面不解压；只勾内容 → 照常解压不提封面；都勾 → 两件都做；都不勾 → 主按钮置灰。
/// 做成「独立按钮」或「解压完再在完成卡里附带」，都会被反问「我不是说了要和压缩包一块显示、各带勾选吗」。
///
/// 没有封面时不显示封面那行（普通压缩包只有内容一行）。
library;

import 'package:flutter/material.dart';

import '../engine.dart';
import '../format.dart';
import '../l10n.dart';

class PreviewPickCard extends StatelessWidget {
  const PreviewPickCard({
    super.key,
    required this.preview,
    required this.wantCover,
    required this.wantExtract,
    required this.onWantCoverChanged,
    required this.onWantExtractChanged,
  });

  final Preview preview;

  /// 勾了「提取伪装封面」
  final bool wantCover;

  /// 勾了「解压压缩包内容」
  final bool wantExtract;

  final void Function(bool v) onWantCoverChanged;
  final void Function(bool v) onWantExtractChanged;

  @override
  Widget build(BuildContext context) {
    final pv = preview;
    return Card(
      margin: EdgeInsets.zero,
      clipBehavior: Clip.antiAlias,
      child: Column(
        children: [
          if (pv.hasCover)
            CheckboxListTile(
              key: const ValueKey('want-cover'),
              dense: true,
              value: wantCover,
              onChanged: (v) => onWantCoverChanged(v ?? false),
              secondary: const Icon(Icons.movie_outlined, size: 20),
              title: Text(
                t('home.coverLabel', {
                  'size': formatBytes((pv.cover!['size'] as num?)?.toInt() ?? 0),
                  'ext':
                      pv.cover!['ext'] != null ? ' · ${pv.cover!['ext']}' : '',
                }),
                style: const TextStyle(
                    fontSize: 13, fontWeight: FontWeight.w500),
              ),
              subtitle: Text(
                t('home.pickMedia'),
                style: const TextStyle(fontSize: 11),
              ),
            ),
          CheckboxListTile(
            key: const ValueKey('want-extract'),
            dense: true,
            value: wantExtract,
            onChanged: (v) => onWantExtractChanged(v ?? false),
            secondary: const Icon(Icons.folder_zip_outlined, size: 20),
            title: Text(
              t('home.contentsTitle',
                  {
                    'files': '${pv.fileCount}',
                    'size': pv.hasSizeSuspect
                        ? t('home.sizeSuspect')
                        : formatBytes(pv.totalSize),
                  }),
              style:
                  const TextStyle(fontSize: 13, fontWeight: FontWeight.w500),
            ),
            subtitle: Text(
              t('home.pickFiles'),
              style: const TextStyle(fontSize: 11),
            ),
          ),
        ],
      ),
    );
  }
}
