import 'dart:io';

import 'package:flutter/material.dart';

import '../engine.dart';
import '../humanize.dart';
import '../l10n.dart';
import '../platform_paths.dart';

/// 关于页：版本、边界、报障入口
///
/// 文案全部走 `t()`（T14）：这一页是界面多语言的样板页 —— 句子最长、
/// 占位符最多（版本号、诊断包路径、失败原因），先在这一页把三种形态
/// （纯文本 / 带一个占位符 / 带「失败原因」这类子句）都跑通，再推别的页。
class AboutPage extends StatefulWidget {
  final String engineVersion;

  const AboutPage({super.key, required this.engineVersion});

  @override
  State<AboutPage> createState() => _AboutPageState();
}

class _AboutPageState extends State<AboutPage> {
  final EngineClient _client = EngineClient.instance;
  bool _busy = false;
  String? _reportPath;

  Future<void> _makeReport() async {
    setState(() => _busy = true);
    try {
      final r = await _client.report();
      if (!mounted) return;
      setState(() => _reportPath = r['path'] as String?);
    } on EngineError catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        // 错误码先翻成人话（humanize），再套进「生成失败：{reason}」这句里
        SnackBar(content: Text(t('about.report.failed', {'reason': errorText(e.code)}))),
      );
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Row(
          children: [
            CircleAvatar(
              radius: 26,
              backgroundColor: cs.primaryContainer,
              child: Icon(Icons.inventory_2_outlined, size: 28, color: cs.onPrimaryContainer),
            ),
            const SizedBox(width: 14),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(t('app.name'), style: Theme.of(context).textTheme.titleLarge),
                Text(
                  t('app.tagline', {'version': widget.engineVersion}),
                  style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
                ),
              ],
            ),
          ],
        ),
        const SizedBox(height: 18),
        Text(
          t('about.intro'),
          style: TextStyle(fontSize: 13, height: 1.7, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 20),
        _card(
          cs,
          title: t('about.limits.title'),
          body: t('about.limits.body'),
        ),
        const SizedBox(height: 12),
        _card(
          cs,
          title: t('about.report.title'),
          body: t('about.report.body'),
        ),
        const SizedBox(height: 12),
        if (_reportPath != null)
          Container(
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              color: cs.primaryContainer.withValues(alpha: 0.35),
              borderRadius: BorderRadius.circular(8),
            ),
            child: SelectableText(
              t('about.report.path', {'path': _reportPath!}),
              style: const TextStyle(fontSize: 11),
            ),
          ),
        const SizedBox(height: 12),
        FilledButton.icon(
          onPressed: _busy ? null : _makeReport,
          icon: _busy
              ? const SizedBox(
                  width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
              : const Icon(Icons.bug_report_outlined),
          label: Text(_busy ? t('about.report.busy') : t('about.report.make')),
        ),
        const SizedBox(height: 10),
        OutlinedButton.icon(
          onPressed: () async {
            final dir = stateDir;
            await Directory(dir).create(recursive: true);
            await openDirectory(dir);
          },
          icon: const Icon(Icons.folder_open, size: 18),
          label: Text(t('about.stateDir')),
        ),
        const SizedBox(height: 20),
        Center(
          child: Text(
            t('about.footer'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
        ),
      ],
    );
  }

  Widget _card(ColorScheme cs, {required String title, required String body}) => Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: cs.surfaceContainerHighest.withValues(alpha: 0.6),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
            const SizedBox(height: 6),
            Text(body, style: const TextStyle(fontSize: 12, height: 1.6)),
          ],
        ),
      );
}
