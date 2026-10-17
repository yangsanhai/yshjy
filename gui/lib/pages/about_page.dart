import 'dart:io';

import 'package:flutter/material.dart';

import '../engine.dart';

/// 关于页：版本、边界、报障入口
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
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('生成失败：${e.code}')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final home = Platform.environment['HOME'] ?? '.';
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
                Text('杨三海解压', style: Theme.of(context).textTheme.titleLarge),
                Text(
                  'yshjy · 引擎 ${widget.engineVersion} · 协议 v1 · MIT 许可',
                  style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
                ),
              ],
            ),
          ],
        ),
        const SizedBox(height: 18),
        Text(
          '专门对付「网盘下载下来、扩展名是 .mp4 / .png，但所有工具都打不开」的压缩包。\n'
          '自己定位真归档藏在哪，再交给成熟的解码库解压；反向也能打包成伪装视频。',
          style: TextStyle(fontSize: 13, height: 1.7, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 20),
        _card(
          cs,
          title: '它不做什么（硬边界）',
          body: '· 不联网、不上传任何东西\n'
              '· 不做字典/爆破、不内置密码表\n'
              '· 抓到的解压码只试不存\n'
              '· 解出可执行文件一定提醒你',
        ),
        const SizedBox(height: 12),
        _card(
          cs,
          title: '出问题了？',
          body: '点下面生成诊断包（路径已脱敏），把这一个 zip 发给开发者就行。\n'
              '它包含：版本、系统、当时参数、识别结果；**不含**文件内容和密码。',
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
              '诊断包：$_reportPath',
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
          label: Text(_busy ? '生成中…' : '生成诊断包'),
        ),
        const SizedBox(height: 10),
        OutlinedButton.icon(
          onPressed: () async {
            final dir = '$home/.local/state/yshjy';
            await Directory(dir).create(recursive: true);
            await Process.run('xdg-open', [dir]);
          },
          icon: const Icon(Icons.folder_open, size: 18),
          label: const Text('打开状态目录（崩溃日志在这里）'),
        ),
        const SizedBox(height: 20),
        Center(
          child: Text(
            '用 Flutter + Rust 做的 · 界面与引擎通过 docs/PROTOCOL.md 通信',
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
