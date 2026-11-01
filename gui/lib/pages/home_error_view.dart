/// 失败页的两段 —— 从 `home_page.dart` 抽出来的第 8 块 B/C 段。
///
/// 失败页是这个项目最要紧的一屏：用户撞上它的次数远多于成功页（包坏了、密码错了、
/// 空间不够、缺分卷都落这儿）。所以每一句「为什么失败 + 下一步做什么」都是真机上一轮轮磨出来的，
/// 注释里带着来源，别随手改文案。
///
/// 两块分工：
/// - `ErrorHead`：图标 / 一句话诊断 / 类型提示 / 缺分卷细节 / 救援按钮 / 两个详情按钮
/// - `ErrorPasswordRetry`：密码错了的重试区（常用码**点一下就直接重试**，和预览页只填入不一样）
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../humanize.dart';
import '../l10n.dart';

/// 头部：图标 + 诊断 + 类型提示 + 缺分卷 + 救援 + 详情
class ErrorHead extends StatelessWidget {
  const ErrorHead({
    super.key,
    required this.errorCode,
    required this.errorNote,
    required this.errorDetail,
    required this.canRescue,
    required this.hint,
    required this.onRescue,
    required this.onMakeReport,
  });

  /// 引擎的错误码（`null` 时按 `UNKNOWN` 说）
  final String? errorCode;

  /// 技术详情（非空才显示「生成诊断包 / 复制详情」两个按钮）
  final String? errorNote;

  /// 错误的 detail（缺分卷时读 `missing` / `found`）
  final Map<String, dynamic>? errorDetail;

  /// 能不能试一把「修复索引」（CLI 的 `--rescue`）
  final bool canRescue;

  /// 「这个文件其实是……」那句话（父层算好；缺分卷/空间不足时父层会传 null）
  final ({String text, bool warn})? hint;

  final VoidCallback onRescue;
  final VoidCallback onMakeReport;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final note = errorNote ?? '';
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(Icons.error_outline, size: 64, color: cs.error),
        const SizedBox(height: 16),
        Text(
          errorText(errorCode ?? 'UNKNOWN'),
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.titleMedium,
        ),
        if (hint case final h?) ...[
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            decoration: BoxDecoration(
              color: h.warn
                  ? cs.errorContainer.withValues(alpha: 0.35)
                  : cs.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Text(
              h.text,
              textAlign: TextAlign.center,
              style: TextStyle(
                fontSize: 13,
                color: h.warn ? cs.error : null,
                height: 1.5,
              ),
            ),
          ),
        ],
        // 缺分卷是网盘资源最高频的失败原因 —— 直接说缺哪一块、怎么补救
        if (errorCode == 'MISSING_VOLUME') ...[
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            decoration: BoxDecoration(
              color: cs.errorContainer.withValues(alpha: 0.35),
              borderRadius: BorderRadius.circular(10),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  (errorDetail?['missing'] as String?) != null
                      ? t('home.missingPart',
                          {'name': '${errorDetail!['missing']}'})
                      : t('home.volumesIncomplete'),
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: cs.error,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  (errorDetail?['found'] as String?) != null
                      ? t('home.volumesFound', {'n': '${errorDetail!['found']}'})
                      : t('home.volumesMissing'),
                  style: const TextStyle(fontSize: 12, height: 1.5),
                ),
              ],
            ),
          ),
        ],
        // 索引坏了也有救：CLI 的 --rescue 在界面上就是这个按钮。
        // 用 OutlinedButton 不用 FilledButton —— 首屏「唯一最强入口」的约束
        // 有 widget 测试钉着，这里再放一个实心按钮会诱导误点。
        if (canRescue) ...[
          const SizedBox(height: 14),
          OutlinedButton.icon(
            key: const ValueKey('rescue'),
            onPressed: onRescue,
            icon: const Icon(Icons.healing_outlined, size: 18),
            label: Text(t('home.rescueBtn')),
          ),
          const SizedBox(height: 4),
          Text(
            t('home.rescueDesc'),
            textAlign: TextAlign.center,
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
        ],
        // 技术详情（`Error { code: … }`）对新手是噪音，默认收起来：
        // 需要报障时一键复制，而不是糊在脸上
        if (note.isNotEmpty)
          TextButton.icon(
            onPressed: onMakeReport,
            icon: const Icon(Icons.bug_report_outlined, size: 15),
            label: Text(t('home.diagPack'), style: const TextStyle(fontSize: 12)),
          ),
        if (note.isNotEmpty)
          TextButton.icon(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: note));
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text(t('home.copiedDetail'))),
              );
            },
            icon: const Icon(Icons.copy, size: 15),
            label: Text(t('home.copyDetail'), style: const TextStyle(fontSize: 12)),
          ),
      ],
    );
  }
}

/// 密码错了的重试区。
///
/// **和预览页的 `PreviewPasswordBox` 有一处关键差别**：这里点常用码是「填入并**立即重试**」，
/// 预览页只是填入（那时还没开始解压，重试无从谈起）。
class ErrorPasswordRetry extends StatelessWidget {
  const ErrorPasswordRetry({
    super.key,
    required this.controller,
    required this.showPlain,
    required this.recent,
    required this.onToggleShowPlain,
    required this.onRetry,
    required this.onPickRecent,
  });

  final TextEditingController controller;
  final bool showPlain;
  final List<String> recent;

  final VoidCallback onToggleShowPlain;
  final VoidCallback onRetry;

  /// 点了某个常用码（父层负责「填入 + 立刻重试」）
  final void Function(String pw) onPickRecent;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 18),
        // 密码本里的常用解压码（调研 07:874-878）
        if (recent.isNotEmpty) ...[
          Align(
            alignment: Alignment.centerLeft,
            child: Text(
              t('home.commonPw'),
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
            ),
          ),
          const SizedBox(height: 6),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final pw in recent)
                ActionChip(
                  avatar: const Icon(Icons.history, size: 14),
                  label: Text(pw, style: const TextStyle(fontSize: 12)),
                  visualDensity: VisualDensity.compact,
                  onPressed: () => onPickRecent(pw),
                ),
            ],
          ),
          const SizedBox(height: 10),
        ],
        TextField(
          controller: controller,
          obscureText: !showPlain,
          autofocus: true,
          // 回车就重试：输完密码按回车的直觉动作
          onSubmitted: (_) => onRetry(),
          textInputAction: TextInputAction.done,
          decoration: InputDecoration(
            labelText: t('home.pwLabel'),
            hintText: t('home.pwHint'),
            border: const OutlineInputBorder(),
            isDense: true,
            prefixIcon: const Icon(Icons.key),
            // 眼睛：点一下看明文，再点回密文（打错一个字符不用整段重输）
            suffixIcon: IconButton(
              icon: Icon(
                showPlain ? Icons.visibility_off : Icons.visibility,
                size: 18,
              ),
              tooltip: showPlain ? t('home.pwHide') : t('home.pwShow'),
              onPressed: onToggleShowPlain,
            ),
          ),
        ),
        const SizedBox(height: 10),
        FilledButton.icon(
          onPressed: onRetry,
          icon: const Icon(Icons.lock_open),
          label: Text(t('home.retryWithPw')),
        ),
      ],
    );
  }
}
