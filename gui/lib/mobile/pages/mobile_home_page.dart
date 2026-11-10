// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机端首页：选包 → 预览 → 解压 → 完成
//!
//! 与桌面首页（`desktop/pages/home_page.dart`，1990 行）是**两个实现**：
//! 桌面那套是「拖放 + 宽屏三栏 + 队列」，手机上是「点选 + 单列 + 一个大按钮」。
//! 两边共用 `common/engine.dart` 的引擎接口与 `common/l10n.dart` 的文案。
//!
//! 交互参考了 `refs/ui-reference/ZArchiver/` 的实拍：
//! 主界面就是列表，动作放在底部（桌面那套的右键菜单在这里不存在）。

import 'dart:async';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/format.dart';
import 'package:yshjy_gui/common/humanize.dart';
import 'package:yshjy_gui/common/history_store.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/platform_paths.dart';

class MobileHomePage extends StatefulWidget {
  const MobileHomePage({super.key});

  @override
  State<MobileHomePage> createState() => _MobileHomePageState();
}

class _MobileHomePageState extends State<MobileHomePage> {
  final EngineApi _api = EngineClient.instance;
  StreamSubscription<EngineEvent>? _sub;

  /// 当前这个包（已经是**引擎能打开的真路径**：SAF 的 content:// 会先落到缓存）
  String? _path;
  Preview? _preview;
  ExtractResult? _result;
  EngineError? _error;
  Progress? _progress;

  bool _busy = false;
  bool _needPassword = false;

  /// 正在导出到系统下载目录
  bool _exporting = false;
  final TextEditingController _pw = TextEditingController();

  /// 用户点过「换个位置」才有；null = 用默认（App 私有目录）
  String? _dest;

  @override
  void initState() {
    super.initState();
    _sub = _api.events.listen((e) {
      final p = e.progress;
      if (p != null && mounted) setState(() => _progress = p);
    });
  }

  @override
  void dispose() {
    _sub?.cancel();
    _pw.dispose();
    super.dispose();
  }

  // ── 选文件 ─────────────────────────────────────────────────────────
  Future<void> _pick() async {
    try {
      const group = XTypeGroup(label: 'any');
      final f = await openFile(acceptedTypeGroups: const [group]);
      if (f == null) return;
      final real = await _materialize(f);
      if (!mounted || real == null) return;
      setState(() {
        _path = real;
        _preview = null;
        _result = null;
        _error = null;
        _progress = null;
        _needPassword = false;
        _pw.clear();
        _dest = null;
      });
      await _identify();
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = EngineError('PICK_FAILED', '$e'));
    }
  }

  /// 把选中的文件变成引擎能打开的**真路径**
  ///
  /// 安卓 10+ 的分区存储给的是 `content://` 而不是路径，`dart:io` 打不开
  /// （`PathAccessException`）。所以：不是真路径就整份拷进 App 缓存，
  /// 拿缓存里的真路径交给引擎 —— 引擎那边不用改（参考 localsend 的做法）。
  Future<String?> _materialize(XFile f) async {
    final p = f.path;
    if (!p.startsWith('content://') && File(p).existsSync()) return p;
    final dir = Directory('$cacheDir/incoming');
    await dir.create(recursive: true);
    var name = f.name;
    if (name.isEmpty) name = 'archive';
    // 同名不同包会互相覆盖 —— 加个时间戳保证唯一
    final dst = '${dir.path}/${DateTime.now().millisecondsSinceEpoch}_$name';
    await f.saveTo(dst);
    return dst;
  }

  // ── 识别（list）──────────────────────────────────────────────────
  Future<void> _identify() async {
    final path = _path;
    if (path == null) return;
    setState(() {
      _busy = true;
      _error = null;
      _needPassword = false;
    });
    try {
      final p = await _api.list(path, password: _pw.text.isEmpty ? null : _pw.text);
      if (!mounted) return;
      setState(() => _preview = p);
    } on EngineError catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _needPassword = e.code == 'NEED_PASSWORD' || e.code == 'HEAD_ENCRYPTED';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = EngineError('INTERNAL', '$e'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  // ── 解压 ─────────────────────────────────────────────────────────
  Future<void> _extract() async {
    final path = _path;
    if (path == null) return;
    final dest = _dest ?? _defaultDest(path);
    setState(() {
      _busy = true;
      _error = null;
      _progress = null;
    });
    try {
      final r = await _api.extract(
        path,
        dest: dest,
        password: _pw.text.isEmpty ? null : _pw.text,
        // 手机上不单独要「伪装封面」那份文件，省一步
        cover: false,
        // 不问「要不要继续」这类交互：手机上就是点了就解
        ask: false,
      );
      await HistoryStore.add(HistoryEntry(
        time: DateTime.now().toIso8601String(),
        input: path,
        dest: r.dest ?? dest,
        ok: r.ok,
        bytes: r.bytesWritten,
      ));
      if (!mounted) return;
      setState(() => _result = r);
    } on EngineError catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _needPassword = e.code == 'NEED_PASSWORD' || e.code == 'WRONG_PASSWORD';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = EngineError('INTERNAL', '$e'));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _cancel() async {
    try {
      await _api.cancel();
    } catch (_) {}
  }

  /// 默认解压到 App 私有目录（方案 A：零权限、装上就能用）
  String _defaultDest(String path) {
    final base = path.split('/').last;
    final dot = base.lastIndexOf('.');
    final name = dot > 0 ? base.substring(0, dot) : base;
    return '$stateDir/out/$name';
  }

  void _reset() {
    setState(() {
      _path = null;
      _preview = null;
      _result = null;
      _error = null;
      _progress = null;
      _needPassword = false;
      _pw.clear();
      _dest = null;
    });
  }

  /// 把解压结果导出到系统「下载」目录
  ///
  /// 解压默认落在 App 私有目录（零权限），手机上文件管理器看不到；
  /// 这里走 MediaStore 一条一条插，**不需要「所有文件访问」权限**就能写进
  /// `Download/yshjy/…`（方案 C 路，见 docs/ANDROID-整体方案 §5）。
  Future<void> _export(String dir) async {
    setState(() => _exporting = true);
    try {
      final n = await const MethodChannel('yshjy/export')
          .invokeMethod<int>('exportToDownloads', {'dir': dir, 'subDir': 'yshjy'});
      _toast(t('mobile.export.done', {'n': '${n ?? 0}'}));
    } catch (_) {
      _toast(t('mobile.export.fail'));
    } finally {
      if (mounted) setState(() => _exporting = false);
    }
  }

  void _toast(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }
  // ── 界面 ─────────────────────────────────────────────────────────
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(t('shell.appName')),
        centerTitle: false,
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
        children: [
          if (_path == null) ..._emptyState(context),
          if (_path != null) ..._taskBody(context),
        ],
      ),
    );
  }

  List<Widget> _emptyState(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return [
      const SizedBox(height: 40),
      Center(
        child: Container(
          width: 96,
          height: 96,
          decoration: BoxDecoration(
            color: cs.primaryContainer,
            borderRadius: BorderRadius.circular(28),
          ),
          child: Icon(Icons.unarchive_outlined, size: 48, color: cs.onPrimaryContainer),
        ),
      ),
      const SizedBox(height: 24),
      Text(
        t('mobile.empty.title'),
        textAlign: TextAlign.center,
        style: Theme.of(context).textTheme.bodyMedium,
      ),
      const SizedBox(height: 28),
      FilledButton.icon(
        onPressed: _pick,
        icon: const Icon(Icons.folder_open),
        label: Text(t('home.pickFile')),
        style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(52)),
      ),
      const SizedBox(height: 12),
      Text(
        t('mobile.empty.hint'),
        textAlign: TextAlign.center,
        style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.outline),
      ),
    ];
  }

  List<Widget> _taskBody(BuildContext context) {
    final path = _path!;
    return [
      // 文件名
      Card(
        child: ListTile(
          leading: const Icon(Icons.insert_drive_file_outlined),
          title: Text(path.split('/').last, maxLines: 1, overflow: TextOverflow.ellipsis),
          subtitle: Text(prettyPath(path), maxLines: 1, overflow: TextOverflow.ellipsis),
          trailing: TextButton(onPressed: _busy ? null : _reset, child: Text(t('home.changeFile'))),
        ),
      ),
      const SizedBox(height: 8),
      if (_busy && _preview == null && _result == null) _identifying(context),
      if (_error != null) ..._errorCard(context),
      if (_preview != null && _result == null && !_busy) ..._previewCard(context),
      if (_result != null) ..._doneCard(context),
      if (_busy && _progress != null) _progressCard(context),
    ];
  }

  Widget _identifying(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 32),
      child: Column(
        children: [
          const CircularProgressIndicator(),
          const SizedBox(height: 16),
          Text(t('home.identifying')),
        ],
      ),
    );
  }

  Widget _progressCard(BuildContext context) {
    final p = _progress!;
    final frac = p.fraction;
    return Padding(
      padding: const EdgeInsets.only(top: 16),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(t('home.extracting')),
              const SizedBox(height: 12),
              LinearProgressIndicator(value: frac),
              const SizedBox(height: 10),
              Text(
                '${formatBytes(p.doneBytes)} / ${formatBytes(p.totalBytes)}',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 12),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(onPressed: _cancel, child: Text(t('home.cancel'))),
              ),
            ],
          ),
        ),
      ),
    );
  }

  List<Widget> _errorCard(BuildContext context) {
    final e = _error!;
    final cs = Theme.of(context).colorScheme;
    return [
      Card(
        color: cs.errorContainer,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(Icons.error_outline, color: cs.onErrorContainer),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      errorText(e.code),
                      style: TextStyle(color: cs.onErrorContainer, fontWeight: FontWeight.w600),
                    ),
                  ),
                ],
              ),
              // 引擎的技术备注：普通用户看不懂，但排障时是唯一线索 —— 小字放下面
              if (e.note.isNotEmpty) ...[
                const SizedBox(height: 8),
                Text(
                  e.note,
                  style: TextStyle(color: cs.onErrorContainer, fontSize: 12),
                ),
              ],
            ],
          ),
        ),
      ),
      if (_needPassword) ...[
        const SizedBox(height: 12),
        TextField(
          controller: _pw,
          obscureText: false,
          decoration: InputDecoration(
            labelText: t('home.pwLabel'),
            helperText: t('home.pwHint'),
            border: const OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Expanded(
              child: FilledButton(
                onPressed: _busy ? null : (_preview == null ? _identify : _extract),
                child: Text(t('home.retryWithPw')),
              ),
            ),
          ],
        ),
      ],
      const SizedBox(height: 8),
      OutlinedButton(onPressed: _busy ? null : _reset, child: Text(t('home.changeFile'))),
    ];
  }

  List<Widget> _previewCard(BuildContext context) {
    final p = _preview!;
    final dest = _dest ?? _defaultDest(_path!);
    final previewEntries = p.entries.where((e) => !e.isDir).take(50).toList();
    return [
      Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Chip(
                    label: Text(p.format.toUpperCase()),
                    visualDensity: VisualDensity.compact,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      t('home.fileCountSize', {
                        'files': '${p.fileCount}',
                        'size': formatBytes(p.totalSize),
                      }),
                      style: Theme.of(context).textTheme.bodyMedium,
                    ),
                  ),
                ],
              ),
              if (p.encryptedCount > 0)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text('🔒 ${p.encryptedCount}', style: Theme.of(context).textTheme.bodySmall),
                ),
              const Divider(height: 24),
              // 包内文件（只列前 50 个 —— 手机上没人会翻 1000 行）
              for (final e in previewEntries)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 3),
                  child: Row(
                    children: [
                      Expanded(
                        child: Text(e.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                      const SizedBox(width: 8),
                      Text(formatBytes(e.size), style: Theme.of(context).textTheme.bodySmall),
                    ],
                  ),
                ),
              if (p.fileCount > previewEntries.length)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text('… +${p.fileCount - previewEntries.length}',
                      style: Theme.of(context).textTheme.bodySmall),
                ),
            ],
          ),
        ),
      ),
      const SizedBox(height: 12),
      // 解到哪 —— 手机上给出完整路径，并可复制/改正（批量 B 权限之后才能换到公共目录）
      Card(
        child: ListTile(
          leading: const Icon(Icons.folder_outlined),
          title: Text(t('mobile.dest.default'), maxLines: 1, overflow: TextOverflow.ellipsis),
          subtitle: Text(dest, maxLines: 2, style: const TextStyle(fontSize: 12)),
          trailing: IconButton(
            tooltip: t('home.copyHowtoBtn'),
            icon: const Icon(Icons.copy_all_outlined),
            onPressed: () => Clipboard.setData(ClipboardData(text: dest)),
          ),
        ),
      ),
      const SizedBox(height: 16),
      FilledButton.icon(
        onPressed: _extract,
        icon: const Icon(Icons.unarchive),
        label: Text(t('home.startPicked', {'n': '${p.fileCount}'})),
        style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(52)),
      ),
      const SizedBox(height: 8),
      OutlinedButton(onPressed: _reset, child: Text(t('home.changeFile'))),
    ];
  }

  List<Widget> _doneCard(BuildContext context) {
    final r = _result!;
    final cs = Theme.of(context).colorScheme;
    final dest = r.dest ?? _dest ?? _defaultDest(_path!);
    return [
      Card(
        color: cs.secondaryContainer,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(r.ok ? Icons.check_circle_outline : Icons.warning_amber_outlined,
                      color: cs.onSecondaryContainer),
                  const SizedBox(width: 8),
                  Text(
                    r.ok ? t('home.done') : t('home.extractFailed'),
                    style: TextStyle(color: cs.onSecondaryContainer, fontWeight: FontWeight.w600),
                  ),
                ],
              ),
              const SizedBox(height: 10),
              Text(t('home.extractedN', {'entries': '${r.entriesDone}'})),
              const SizedBox(height: 4),
              Text(
                '${t('home.durSec', {'sec': '${(r.millis / 1000).round()}'})} · ${formatBytes(r.bytesWritten)}',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
          ),
        ),
      ),
      const SizedBox(height: 12),
      Card(
        child: ListTile(
          leading: const Icon(Icons.folder_outlined),
          title: Text(t('mobile.dest.default')),
          subtitle: Text(dest, maxLines: 2, style: const TextStyle(fontSize: 12)),
        ),
      ),
      const SizedBox(height: 12),
      FilledButton.tonalIcon(
        onPressed: _exporting ? null : () => _export(dest),
        icon: const Icon(Icons.download_outlined),
        label: Text(t('mobile.export')),
        style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(48)),
      ),
      const SizedBox(height: 16),
      FilledButton(onPressed: _reset, child: Text(t('home.again'))),
    ];
  }
}
