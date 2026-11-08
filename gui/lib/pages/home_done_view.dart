// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 单个文件解压完的完成屏 —— 从 `home_page.dart` 抽出来的第 7 块。
///
/// **别和第 2 块的 `BatchDoneView` 搞混**：那个是「整批跑完」的总账（横向列表、逐项成败），
/// 这个是**单个文件**解完的一张卡（大图标 + 一句话 + 产物路径 + 三个按钮）。
///
/// 三个「这个屏千万别写错」的点（都来自真机反馈，改动前先读）：
/// 1. **校验模式（`verifyMode`）没有产物**：`bytesWritten` 是「解码过的字节」不是写进硬盘的量，
///    不能当产物大小显示；`dest` 是引擎顺手回显的路径、目录根本没建过，所以「打开目录」不能给。
/// 2. **只勾了「伪装封面」时解出 0 个文件是正常的**：说「解开了 0 个文件」会吓到用户。
/// 3. **嵌套包不自动解**：只在产物里发现了压缩包时问一句，用户点多一次才继续。
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import '../format.dart';
import '../key_action_scope.dart';
import '../keyboard.dart';
import '../l10n.dart';

class DoneView extends StatelessWidget {
  const DoneView({
    super.key,
    required this.result,
    required this.verifyMode,
    required this.nestedArchives,
    required this.notes,
    required this.fileName,
    required this.password,
    required this.entryHint,
    required this.onScanNested,
    required this.onOpenFolder,
    required this.onReset,
  });

  final ExtractResult result;

  /// 校验模式（只读不写）
  final bool verifyMode;

  /// 产物里还套着的压缩包（非空 → 显示「继续解压」入口）
  final List<String> nestedArchives;

  /// 引擎给的提示行
  final List<String> notes;

  /// 原文件名（复制分享文案用）
  final String? fileName;

  /// 用户填过的解压码（非空就一起写进分享文案）
  final String? password;

  /// 「接下来点这个」的目标文件（`null` = 不指路）
  final String? entryHint;

  final void Function(List<String> paths) onScanNested;
  final void Function(String? dir) onOpenFolder;

  /// 「再来一个」= 回到空态
  final VoidCallback onReset;

  @override
  Widget build(BuildContext context) {
    final r = result;
    final cs = Theme.of(context).colorScheme;
    final ok = r.ok;
    // 只勾了「伪装封面」那一项：解出 0 个文件是正常的，别吓用户
    final onlyCover = r.entriesDone == 0 && r.cover != null;
    final seconds = (r.millis / 1000).toStringAsFixed(1);
    final content = Center(
      key: const ValueKey('done'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(
            verifyMode
                ? (ok ? Icons.verified_outlined : Icons.report_problem_outlined)
                : (ok ? Icons.check_circle_outline : Icons.info_outline),
            size: 72,
            color: ok ? cs.primary : cs.tertiary,
          ),
          const SizedBox(height: 16),
          Text(
            verifyMode
                // 别写「解开了 N 个文件」：一个字节都没写，目录也不存在
                ? (ok
                    ? t('home.verifyAllGood', {'entries': '${r.entriesDone}'})
                    : t('home.verifySomeBad', {
                        'done': '${r.entriesDone}',
                        'total': '${r.entriesTotal}'
                      }))
                : (ok
                    ? (onlyCover
                        ? t('home.coverExtracted')
                        : t('home.extractedN', {'entries': '${r.entriesDone}'}))
                    : t('home.extractedPartialN', {
                        'done': '${r.entriesDone}',
                        'total': '${r.entriesTotal}'
                      })),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(
            // 校验时 bytesWritten 是「解码过的字节」，不是写进硬盘的量 —— 不能拿来当产物大小
            verifyMode
                ? t('home.timeNoWrite', {'secs': seconds})
                : t('home.timeWithSize',
                    {'secs': seconds, 'size': formatBytes(r.bytesWritten)}),
            style: TextStyle(color: cs.onSurfaceVariant),
          ),
          if (!verifyMode && r.dest != null) ...[
            const SizedBox(height: 6),
            Text(
              r.dest!,
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 12),
            ),
          ],
          // 嵌套包（产物里还套着压缩包）：问一句，不自动解
          if (nestedArchives.isNotEmpty) ...[
            const SizedBox(height: 10),
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
              decoration: BoxDecoration(
                color: cs.secondaryContainer,
                borderRadius: BorderRadius.circular(10),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.inventory_2_outlined,
                      size: 16, color: cs.onSecondaryContainer),
                  const SizedBox(width: 8),
                  Text(
                    t('home.nestedCount', {'n': '${nestedArchives.length}'}),
                    style:
                        TextStyle(fontSize: 12, color: cs.onSecondaryContainer),
                  ),
                  TextButton(
                    onPressed: () => onScanNested(nestedArchives),
                    child: Text(t('home.continueExtract')),
                  ),
                ],
              ),
            ),
          ],
          // 封面分离结果（DESIGN §17.5）：伪装包里的原版视频/图片
          if (r.cover != null) ...[
            const SizedBox(height: 10),
            Text(
              t('home.coverSeparated', {
                'name': (r.cover!['path'] as String).split('/').last,
                'size': formatBytes((r.cover!['size'] as num?)?.toInt() ?? 0),
              }),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            TextButton.icon(
              onPressed: () => onOpenFolder(r.cover!['path'] as String),
              icon: const Icon(Icons.play_circle_outline, size: 16),
              label:
                  Text(t('home.openCover'), style: const TextStyle(fontSize: 12)),
            ),
          ],
          // 「接下来点这个」说的是产物里的启动文件 —— 校验模式没有产物，别指路
          if (!verifyMode)
            if (entryHint case final hint?) ...[
              const SizedBox(height: 12),
              Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                decoration: BoxDecoration(
                  color: cs.primaryContainer.withValues(alpha: 0.35),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Text(
                  t('home.nextClickThis', {'hint': hint}),
                  style: const TextStyle(fontSize: 13),
                ),
              ),
            ],
          if (notes.isNotEmpty) ...[
            const SizedBox(height: 14),
            for (final n in notes)
              Padding(
                padding: const EdgeInsets.only(bottom: 4),
                child: Text('· $n', style: const TextStyle(fontSize: 12)),
              ),
          ],
          const SizedBox(height: 26),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              // 校验模式没有产物目录：那个 dest 是引擎顺手回显的路径，
              // 目录根本没建过，摆个「打开目录」只会点了报错。
              if (!verifyMode && r.dest != null)
                // 键位就地提示（T11）：Enter = 打开目录（本屏主操作）
                Tooltip(
                  message: 'Enter',
                  child: FilledButton.icon(
                    onPressed: () => onOpenFolder(r.dest),
                    icon: const Icon(Icons.folder_open),
                    label: Text(t('home.openDir')),
                  ),
                ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: () {
                  // 分享文案（调研 07:952-954）：资源圈的真实动作是「把方法告诉对方」
                  final name = (fileName ?? '').split('/').last;
                  final pw = (password == null || password!.isEmpty)
                      ? ''
                      : t('home.pwLine', {'pw': '$password'});
                  Clipboard.setData(ClipboardData(
                    text: t('home.copyHowto', {'name': name, 'pw': pw}),
                  ));
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text(t('home.copiedSend'))),
                  );
                },
                icon: const Icon(Icons.copy_all_outlined, size: 18),
                label: Text(t('home.copyHowtoBtn')),
              ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: onReset,
                icon: const Icon(Icons.add),
                label: Text(t('home.againOne')),
              ),
            ],
          ),
        ],
      ),
    );
    // Enter = 本屏主操作（UI-SPEC §4b.6）= 「打开目录」。
    // ★ 校验模式和「只提封面」两种情况没有可打开的产物目录，那时不接
    //   —— 界面上的按钮也是这时才隐藏（`!verifyMode && r.dest != null`），
    //     键盘必须和按钮同一口径，不能凭空给出一个点了就报错的入口。
    // Esc = 取消/返回 → 「再来一个」，和点那个按钮同一条路。
    return KeyActionScope(
      handlers: {
        KeyAction.primary:
            (!verifyMode && r.dest != null) ? () => onOpenFolder(r.dest) : null,
        KeyAction.cancel: onReset,
      },
      child: content,
    );
  }
}
