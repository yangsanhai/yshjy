import 'package:flutter/material.dart';

import '../engine.dart';
import '../keyboard.dart';
import '../l10n.dart';

/// 「这个软件能干什么」的两张只读表。
///
/// 原来它们长在设置页最下面（T17-1 / T11 那两批做的），2026-11-05 用户提出来：
/// 「设置里面一大段话的那些能不能移动到关于里面，因为它们也没啥设置」——
/// 确实，这两张表一个字都不能改，放在「设置」里只是把真正能改的项往下挤。
/// 现在由关于页渲染（`pages/about_page.dart`），设置页只剩能改的东西。
///
/// ⚠️ 文案键名仍然叫 `settings.*`（`settings.section.caps` / `settings.caps.xxx` 等）：
///    改名要同步 7 种语言的 561 个键，纯 churn；键名不对外可见，这里用注释交代清楚就够了。
///
/// 支持的格式表（T17-1：进页面直接显示，不再需要点按钮）
///
/// 为什么不是写死的：能力位只在引擎的 `Caps` 里（`InArchive::caps()`），
/// 界面自己抄一份就等于第二本账 —— 引擎哪天改了能力，界面还在按老账本显示。
/// 所以每次进页面都问引擎要（协议 `hello.formats`），要不到就如实说读不到。
///
/// 2026-11-01 改成自动加载：以前要点一下「读取各格式能力」才出表格，用户的感觉像
/// 「在查远程数据」；现在进页面就自动读 —— 走的是**已经握过手的传输层**（不是 spawn 子进程），
/// 一次往返，代价可以忽略。
class CapsTable extends StatefulWidget {
  const CapsTable({super.key});

  @override
  State<CapsTable> createState() => _CapsTableState();
}

class _CapsTableState extends State<CapsTable> {
  List<Map<String, dynamic>>? _rows;
  String? _err;
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    if (_loading) return;
    setState(() {
      _loading = true;
      _err = null;
    });
    try {
      // 走**传输层**问引擎（协议 `hello.formats`），不再 spawn 子进程跑 `--caps`：
      // ① iOS 禁止 fork/exec，子进程这条路在手机端物理上跑不了（而 caps 表就是要给手机端看的）；
      // ② 桌面端也省一次进程启动，还和「界面只依赖 EngineTransport」这条规矩保持一致。
      final h = await EngineClient.instance.hello();
      if (!mounted) return;
      final raw = h['formats'];
      if (raw is! List) {
        // 老引擎（还没有 hello.formats）—— 如实说读不到，别假装是空表
        setState(() {
          _err = t('settings.caps.tooOld');
          _rows = null;
        });
        return;
      }
      setState(() {
        _rows = raw.map((e) => (e as Map).cast<String, dynamic>()).toList();
      });
    } catch (e) {
      if (mounted) setState(() => _err = '$e');
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final rows = _rows;
    if (rows == null) {
      // 读不到就如实说，**不摆一排按钮** —— 只给一个「重试」小文字按钮。
      // ⚠️ 这里不用 CircularProgressIndicator：转圈是无限动画，任何 pumpAndSettle
      //    都会等到超时（关于页也会在别的测试里被 pump 过）。
      if (_loading) {
        return Text(
          t('settings.caps.reading'),
          style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
        );
      }
      return Row(
        children: [
          Expanded(
            child: Text(
              _err == null
                  ? t('settings.caps.tooOld')
                  : t('settings.caps.error', {'reason': _err!}),
              style: TextStyle(fontSize: 12, color: cs.error),
            ),
          ),
          TextButton(
            onPressed: _load,
            child: Text(t('settings.caps.retry')),
          ),
        ],
      );
    }
    // ⚠️ Dart 的标识符只能是 ASCII —— 中文变量名编译不过（Rust 可以，Dart 不行）
    final capsLabels = <String, String>{
      'encrypted': t('settings.caps.encrypted'),
      'encrypted_names': t('settings.caps.encryptedNames'),
      'split_volumes': t('settings.caps.splitVolumes'),
      'selective': t('settings.caps.selective'),
      'restores_mtime': t('settings.caps.restoresMtime'),
      'can_create': t('settings.caps.canCreate'),
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final r in rows)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: 96,
                  child: Text(
                    // format / shape 都是机器标识（zip / volumes…），展示名在这里拼
                    r['shape'] == 'volumes'
                        ? '${r['format']}${t('settings.caps.volumesSuffix')}'
                        : '${r['format']}',
                    style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500),
                  ),
                ),
                Expanded(
                  child: Text(
                    [
                      for (final e in capsLabels.entries)
                        if (r[e.key] == true) e.value,
                    ].join(' · '),
                    style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
                  ),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// 快捷键表（T11）—— 只读，因为键位是 UI-SPEC 定死的、不做重映射。
///
/// ★ 行的来源是 `lib/keyboard.dart` 的 `keyHelpRows`，**不是**在这里手写一份 ——
///   否则以后改了键、说明还停在旧键上，用户照着按没反应只会以为功能坏了。
/// ★ 键名（Enter / Esc / Ctrl+E）不翻译：各国用户看到的物理键就是这几个字。
class KeysTable extends StatelessWidget {
  const KeysTable({super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      key: const ValueKey('keys-table'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final r in keyHelpRows)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 3),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: 84,
                  child: Text(
                    r.keys,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      fontFamily: 'monospace',
                      color: cs.primary,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(t(r.whatKey),
                      style: const TextStyle(fontSize: 12)),
                ),
              ],
            ),
          ),
        const SizedBox(height: 8),
        Text(
          t('keys.note'),
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
      ],
    );
  }
}
