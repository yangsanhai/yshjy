// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show rootBundle;

import 'package:yshjy_gui/common/l10n.dart';

/// 开源许可清单（数据来自 `assets/third_party.json`，由
/// `scripts/gen-third-party.py` 从 `cargo metadata` 生成）。
///
/// 为什么要有这一页：MIT / BSD / Apache-2.0 都要求分发二进制时**保留版权声明与
/// 许可文本**。不列出来是实打实的许可违规，不是「礼不礼貌」的问题。
///
/// 为什么链接只是文字、不点开浏览器：本软件的口径是**完全离线**（关于页
/// 「完全离线」那张卡片、`tests/offline_probe.py` 都在守这条）。为了点一个
/// 许可链接就引入 `url_launcher` 并且联网，等于为了合规破掉一条产品承诺。
/// 所以地址用 `SelectableText` 给出来，谁想去看就自己复制。
class LicensesPage extends StatefulWidget {
  /// 数据源，默认读打包进来的 `assets/third_party.json`。
  ///
  /// 为什么留这个口子：`rootBundle` 在 widget 测试里**每个用例之间不稳定**
  /// （同一次跑里第一个用例能读到、后面几个会一直停在加载中，2026-11-05 实测），
  /// 注入之后测试不再依赖平台通道，页面逻辑照样被完整覆盖。
  /// 生产路径永远是默认值（真实 asset），这个参数只给测试用。
  final Future<String> Function()? loader;

  const LicensesPage({super.key, this.loader});

  @override
  State<LicensesPage> createState() => _LicensesPageState();
}

class _LicensesPageState extends State<LicensesPage> {
  Map<String, dynamic>? _data;
  String? _err;
  String _filter = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final raw = await (widget.loader ??
          () => rootBundle.loadString('assets/third_party.json'))();
      final m = jsonDecode(raw) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() => _data = m);
    } catch (e) {
      if (mounted) setState(() => _err = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final data = _data;

    if (_err != null) {
      return Scaffold(
        appBar: AppBar(title: Text(t('licenses.title'))),
        body: Padding(
          padding: const EdgeInsets.all(16),
          child: Text(
            t('licenses.loadFailed', {'reason': _err!}),
            style: TextStyle(fontSize: 13, color: cs.error),
          ),
        ),
      );
    }
    if (data == null) {
      // 不用转圈：转圈是无限动画，会让 pumpAndSettle 一直等（本项目的既有教训）
      return Scaffold(
        appBar: AppBar(title: Text(t('licenses.title'))),
        body: const Center(child: Text('…')),
      );
    }

    final deps = (data['deps'] as List).cast<Map<String, dynamic>>();
    final credits = (data['credits'] as List?)?.cast<Map<String, dynamic>>() ?? const [];
    final app = (data['app'] as Map?)?.cast<String, dynamic>() ?? const {};
    final q = _filter.trim().toLowerCase();
    final shown = q.isEmpty
        ? deps
        : deps.where((d) => '${d['name']}'.toLowerCase().contains(q)).toList();

    return Scaffold(
      appBar: AppBar(title: Text(t('licenses.title'))),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          // 本软件自己的许可
          Text(t('licenses.app'), style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
          const SizedBox(height: 4),
          Text(
            '${app['name'] ?? 'yshjy'} · ${app['license'] ?? 'MIT'}',
            style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
          ),
          const SizedBox(height: 16),

          // 第三方库
          Text(
            t('licenses.depsHeader', {'count': '${data['_count'] ?? deps.length}'}),
            style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 8),
          TextField(
            decoration: InputDecoration(
              labelText: t('licenses.search'),
              isDense: true,
              border: const OutlineInputBorder(),
            ),
            onChanged: (v) => setState(() => _filter = v),
          ),
          const SizedBox(height: 8),
          if (shown.isEmpty)
            Text(t('licenses.empty'), style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant))
          else
            for (final d in shown)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        SizedBox(
                          width: 170,
                          child: Text(
                            '${d['name']} ${d['version']}',
                            style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500),
                          ),
                        ),
                        Expanded(
                          child: Text(
                            '${d['license']}',
                            style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
                          ),
                        ),
                      ],
                    ),
                    if ('${d['url'] ?? ''}'.isNotEmpty)
                      SelectableText(
                        '${d['url']}',
                        style: TextStyle(fontSize: 11, color: cs.primary, height: 1.4),
                      ),
                  ],
                ),
              ),

          // 参考与致谢
          if (credits.isNotEmpty) ...[
            const SizedBox(height: 20),
            Text(t('licenses.creditsHeader'),
                style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
            const SizedBox(height: 4),
            // 逐项不显示 json 里的中文 `what`：界面有 7 种语言，只翻译 7 个项目的
            // 长句说明不值当（而且会漂）。那句通用口径走 l10n，细节留在
            // docs/REFERENCE-INDEX.md 里给想看的人。
            Text(
              t('licenses.creditsNote'),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant, height: 1.5),
            ),
            const SizedBox(height: 8),
            for (final c in credits)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('${c['name']}', style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
                    if ('${c['url'] ?? ''}'.isNotEmpty)
                      SelectableText(
                        '${c['url']}',
                        style: TextStyle(fontSize: 11, color: cs.primary),
                      ),
                  ],
                ),
              ),
          ],

          const SizedBox(height: 16),
          Text(
            t('licenses.copyHint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant, height: 1.5),
          ),
          const SizedBox(height: 24),
        ],
      ),
    );
  }
}
