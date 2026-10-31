import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../engine.dart';
import '../l10n.dart';
import '../notify.dart';
import '../platform_paths.dart';
import '../theme_mode.dart';

/// 设置页（可写）
///
/// 只动 `[output]` 段的三项（输出目录 / 重名策略 / 原包处理）：
/// **逐行替换**，其余内容和用户手写的注释原样保留 —— 不做「整份重写」，
/// 那样会把用户自己写的说明全冲掉。
class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  String get _configDir => configDir;

  String get _configPath => '$_configDir/config.toml';

  String get _stateDir => stateDir;

  final _outDirCtrl = TextEditingController();
  String _conflict = 'rename';
  String _onDone = 'keep';
  /// 主题：auto（跟随系统）/ light / dark
  String _theme = 'auto';
  /// 界面语言：`system`（跟随系统）或 supportedLocales 里的某个 code（T14）
  String _uiLocale = 'system';
  /// 处理完成后是否发桌面通知
  bool _notify = true;
  /// 解压完成后是否自动打开输出目录（默认关：批量解压会弹一堆窗口）
  bool _openAfter = false;
  // 解压后检验「移动设备是否真落盘」（默认关，见 docs/TODO.md 与 src/flush.rs）
  bool _flushMovable = false;
  // 自动找解压码的两个开关（T8 界面半）：默认都开，与引擎 `config.rs` 的默认值一致。
  // 分两档是因为代价不同：密码本是用户亲手记的，说明文件是程序去翻包旁边的文本。
  // 默认关：引擎侧 config.rs 的默认值也是 false，两边必须一致
  bool _autoFromNotes = false;
  bool _autoFromBook = false;
  // 关窗口时：tray（缩到托盘）| exit（直接退出），默认分平台（见 app_settings.dart）
  String _closeAction = Platform.isWindows ? 'exit' : 'tray';
  bool _loaded = false;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _outDirCtrl.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final f = File(_configPath);
      final text = await f.exists() ? await f.readAsString() : '';
      final out = _parseSection(text, 'output');
      final gen = _parseSection(text, 'general');
      final saf = _parseSection(text, 'safety');
      final pwd = _parseSection(text, 'password');
      if (!mounted) return;
      setState(() {
        _outDirCtrl.text = out['dir'] ?? '';
        _conflict = out['conflict'] ?? 'rename';
        _onDone = out['on_done'] ?? 'keep';
        _theme = gen['theme'] ?? 'auto';
        // 界面语言：认不出的值一律当「跟随系统」（配置被手改错不该让语言变未知状态）
        final uiLang = gen['ui_language'] ?? 'system';
        _uiLocale = availableLocales.contains(uiLang) ? uiLang : 'system';
        _notify = (gen['notify'] ?? 'on') != 'off';
        _openAfter = (gen['open_after'] ?? 'off') == 'on';
        // 默认开（引擎默认也是 true）；只有明确写 off/false 才算关。
        // 文件里可能写成 TOML 布尔 true/false（用户手改）或 "on"/"off"（本页保存的写法）。
        _autoFromNotes = _readSwitch(pwd['auto_from_notes'], dflt: false);
        _autoFromBook = _readSwitch(pwd['auto_from_book'], dflt: false);
        // 默认关；只有明确写 on/true 才算开
        _flushMovable = const ['on', 'true'].contains(saf['flush_movable'] ?? 'off');
        final ca = gen['close_action'];
        _closeAction = (ca == 'tray' || ca == 'exit')
            ? ca!
            : (Platform.isWindows ? 'exit' : 'tray');
        _loaded = true;
      });
    } catch (_) {
      if (mounted) setState(() => _loaded = true);
    }
  }

  /// 只解析 `[output]` 段（够用且不引 TOML 依赖）
  Map<String, String> _parseSection(String text, String section) {
    final out = <String, String>{};
    var inOutput = false;
    for (final line in text.split('\n')) {
      final t = line.trim();
      if (t.startsWith('[') && t.endsWith(']')) {
        inOutput = t == '[$section]';
        continue;
      }
      if (!inOutput || t.isEmpty || t.startsWith('#')) continue;
      final i = t.indexOf('=');
      if (i <= 0) continue;
      final k = t.substring(0, i).trim();
      var v = t.substring(i + 1).trim();
      if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) {
        v = v.substring(1, v.length - 1);
      }
      out[k] = v;
    }
    return out;
  }

  /// 读一个开关：认得 TOML 布尔（`true`/`false`，用户手改的）和字符串（`"on"`/`"off"`，
  /// 本页 `_upsertSection` 保存的写法）。认不出就用默认值 —— 绝不让配置异常炸界面。
  bool _readSwitch(String? v, {required bool dflt}) {
    final s = (v ?? '').trim().toLowerCase();
    if (const ['on', 'true', 'yes', '1'].contains(s)) return true;
    if (const ['off', 'false', 'no', '0'].contains(s)) return false;
    return dflt;
  }

  /// 在原文里逐行更新 `[output]` 的三个键；段不存在就追加
  String _upsertSection(String text, String section, Map<String, String> vals) {
    final lines = text.isEmpty ? <String>[] : text.split('\n');
    final result = <String>[];
    final seen = <String>{};
    var inOutput = false;
    var sectionFound = false;

    void flushMissing() {
      for (final e in vals.entries) {
        if (!seen.contains(e.key)) {
          result.add('${e.key} = "${e.value}"');
          seen.add(e.key);
        }
      }
    }

    for (final line in lines) {
      final t = line.trim();
      if (t.startsWith('[') && t.endsWith(']')) {
        if (inOutput) flushMissing();
        inOutput = t == '[$section]';
        if (inOutput) sectionFound = true;
        result.add(line);
        continue;
      }
      if (inOutput && !t.startsWith('#') && t.contains('=')) {
        final k = t.substring(0, t.indexOf('=')).trim();
        if (vals.containsKey(k)) {
          result.add('$k = "${vals[k]}"');
          seen.add(k);
          continue;
        }
      }
      result.add(line);
    }
    if (inOutput) flushMissing();
    if (!sectionFound) {
      result
        ..add('')
        ..add('[$section]');
      for (final e in vals.entries) {
        result.add('${e.key} = "${e.value}"');
      }
    }
    return result.join('\n');
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final f = File(_configPath);
      final text = await f.exists() ? await f.readAsString() : '';
      var updated = _upsertSection(text, 'output', {
        'dir': _outDirCtrl.text.trim(),
        'conflict': _conflict,
        'on_done': _onDone,
      });
      updated = _upsertSection(updated, 'general', {
        'theme': _theme,
        'ui_language': _uiLocale,
        'notify': _notify ? 'on' : 'off',
        'open_after': _openAfter ? 'on' : 'off',
        'close_action': _closeAction,
      });
      // 落盘检验开关：引擎读 [safety] 这一段（config.rs 的 flush_movable）
      updated = _upsertSection(updated, 'safety', {
        'flush_movable': _flushMovable ? 'on' : 'off',
      });
      // 自动找解压码的两个开关：引擎读 [password] 这一段（config.rs 的 auto_from_notes
      // / auto_from_book）。只写这两行，`file` 和 `max_candidates` 由 _upsertSection
      // 原样保留 —— 用户手写的值不会被本页冲掉。
      updated = _upsertSection(updated, 'password', {
        'auto_from_notes': _autoFromNotes ? 'on' : 'off',
        'auto_from_book': _autoFromBook ? 'on' : 'off',
      });
      Notifier.enabled = _notify; // 立刻生效，不用重启
      await f.parent.create(recursive: true);
      await f.writeAsString(updated);
      themeMode.value = parseThemeMode(_theme);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(t('settings.saved'))),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(t('settings.saveFailed', {'reason': '$e'}))),
      );
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _pickDir() async {
    try {
      final dir = await getDirectoryPath(confirmButtonText: t('settings.pickDirConfirm'));
      if (dir != null && mounted) {
        setState(() => _outDirCtrl.text = dir);
      }
    } catch (_) {}
  }

  Future<void> _open(String path) async {
    await Directory(path).create(recursive: true);
    await openDirectory(path);
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(t('settings.title'), style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 4),
        Text(
          t('settings.subtitle'),
          style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 18),
        _section(t('settings.section.appearance'), cs),
        DropdownButtonFormField<String>(
          initialValue: _theme,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.theme'),
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: 'auto', child: Text(t('settings.theme.auto'))),
            DropdownMenuItem(value: 'light', child: Text(t('settings.theme.light'))),
            DropdownMenuItem(value: 'dark', child: Text(t('settings.theme.dark'))),
          ],
          onChanged: (v) => setState(() => _theme = v ?? 'auto'),
        ),
        const SizedBox(height: 12),
        // 界面语言（T14）：选完**立刻生效**（不用等「保存」），保存只是记住选择。
        // 每种语言用**它自己语言里的名字**（localeNames），不翻译 —— 否则界面变成
        // 看不懂的语言之后，用户连「怎么切回中文」都找不到。
        DropdownButtonFormField<String>(
          initialValue: _uiLocale,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.language'),
            helperText: t('settings.language.help'),
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: 'system', child: Text(t('settings.theme.auto'))),
            for (final code in availableLocales)
              DropdownMenuItem(value: code, child: Text(localeNames[code] ?? code)),
          ],
          onChanged: (v) async {
            final picked = v ?? 'system';
            setState(() => _uiLocale = picked);
            await switchLocale(picked);
          },
        ),
        const SizedBox(height: 18),
        _section(t('settings.section.output'), cs),
        const SizedBox(height: 6),
        TextField(
          controller: _outDirCtrl,
          decoration: InputDecoration(
            labelText: t('settings.output.dir'),
            hintText: t('settings.output.dirHint'),
            isDense: true,
            border: const OutlineInputBorder(),
            prefixIcon: const Icon(Icons.folder_outlined, size: 18),
            suffixIcon: TextButton(
              onPressed: _pickDir,
              child: Text(t('settings.pick')),
            ),
          ),
        ),
        const SizedBox(height: 12),
        // 两个下拉原本并排（Row + 两个 Expanded）。英文 label 比中文长得多
        // （"Rename automatically (recommended)" / "The archive itself after extraction"），
        // 并排在 576px 的真机窗口里直接把右边挤出去 —— 用户在真机截图上撞到
        // `RIGHT OVERFLOWED BY 40 PIXELS`。改成上下排列 + `isExpanded`（让按钮内文本
        // 能吃满宽度并省略），中英文都不会溢出。**以后再往里加东西，别改回 Row。**
        DropdownButtonFormField<String>(
          initialValue: _conflict,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.output.conflict'),
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: 'rename', child: Text(t('settings.conflict.rename'))),
            DropdownMenuItem(value: 'skip', child: Text(t('settings.conflict.skip'))),
            DropdownMenuItem(value: 'replace', child: Text(t('settings.conflict.replace'))),
          ],
          onChanged: (v) => setState(() => _conflict = v ?? 'rename'),
        ),
        const SizedBox(height: 12),
        DropdownButtonFormField<String>(
          initialValue: _onDone,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.output.onDone'),
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: 'keep', child: Text(t('settings.onDone.keep'))),
            DropdownMenuItem(value: 'trash', child: Text(t('settings.onDone.trash'))),
          ],
          onChanged: (v) => setState(() => _onDone = v ?? 'keep'),
        ),
        const SizedBox(height: 6),
        Text(
          t('settings.onDone.hint'),
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.notify')),
          subtitle: Text(
            t('settings.notify.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _notify,
          onChanged: (v) => setState(() => _notify = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.openAfter')),
          subtitle: Text(
            t('settings.openAfter.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _openAfter,
          onChanged: (v) => setState(() => _openAfter = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.flush')),
          subtitle: Text(
            t('settings.flush.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _flushMovable,
          onChanged: (v) => setState(() => _flushMovable = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.autoNotes')),
          subtitle: Text(
            t('settings.autoNotes.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _autoFromNotes,
          onChanged: (v) => setState(() => _autoFromNotes = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.autoBook')),
          subtitle: Text(
            t('settings.autoBook.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _autoFromBook,
          onChanged: (v) => setState(() => _autoFromBook = v),
        ),
        const SizedBox(height: 6),
        Text(
          t('settings.flush.note'),
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
        DropdownButtonFormField<String>(
          initialValue: _closeAction,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.closeAction'),
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: 'tray', child: Text(t('settings.closeAction.tray'))),
            DropdownMenuItem(value: 'exit', child: Text(t('settings.closeAction.exit'))),
          ],
          onChanged: (v) => setState(() => _closeAction = v ?? 'exit'),
        ),
        const SizedBox(height: 6),
        Text(
          t(Platform.isWindows
              ? 'settings.closeAction.hintWindows'
              : 'settings.closeAction.hintDefault'),
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 14),
        Align(
          alignment: Alignment.centerRight,
          child: FilledButton.icon(
            onPressed: _saving || !_loaded ? null : _save,
            icon: const Icon(Icons.save_outlined, size: 18),
            label: Text(_saving ? t('settings.saving') : t('settings.save')),
          ),
        ),
        const SizedBox(height: 20),
        _section(t('settings.section.paths'), cs),
        _row(
          icon: Icons.settings_outlined,
          title: t('settings.paths.config'),
          subtitle: _configPath,
          action: TextButton(
            onPressed: () => _open(_configDir),
            child: Text(t('settings.openDir')),
          ),
        ),
        _row(
          icon: Icons.key_outlined,
          title: t('settings.paths.book'),
          subtitle: t('settings.paths.bookHint', {'path': '$_configDir/passwords.txt'}),
          action: TextButton(
            onPressed: () => _open(_configDir),
            child: Text(t('settings.openDir')),
          ),
        ),
        _row(
          icon: Icons.bug_report_outlined,
          title: t('settings.paths.state'),
          subtitle: t('settings.paths.stateHint', {'path': _stateDir}),
          action: TextButton(
            onPressed: () => _open(_stateDir),
            child: Text(t('settings.openDir')),
          ),
        ),
        const SizedBox(height: 18),
        _section(t('settings.section.caps'), cs),
        const _CapsTable(),
        const SizedBox(height: 18),
        _section(t('settings.section.misc'), cs),
        // 以前这里还并列写着 7 个语言代码（像在暗示「改 config.toml 就能切界面语言」，
        // 而当时文案全是硬编码中文），后来界面语言进了「外观」下拉，那行就删掉了。
        // 再下面原本还有一段「还没做的（我不装样子）」—— T14 把 479 条文案搬完、
        // 七种语言全部上线之后它就成了过期话，一并删掉。
        _kv(t('settings.misc.jobs'), t('settings.misc.jobsValue')),
        _kv(t('settings.misc.ratio'), t('settings.misc.ratioValue')),
        _kv(t('settings.misc.warn'), 'warn_executables = true'),
      ],
    );
  }

  Widget _section(String title, ColorScheme cs) => Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text(
          title,
          style: TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w600,
            color: cs.primary,
          ),
        ),
      );

  Widget _row({
    required IconData icon,
    required String title,
    required String subtitle,
    Widget? action,
  }) =>
      ListTile(
        dense: true,
        contentPadding: EdgeInsets.zero,
        leading: Icon(icon, size: 20),
        title: Text(title, style: const TextStyle(fontSize: 13)),
        subtitle: Text(subtitle, style: const TextStyle(fontSize: 11)),
        trailing: action,
      );

  Widget _kv(String k, String v) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 96,
              child: Text(k, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
            ),
            Expanded(
              child: Text(v, style: const TextStyle(fontSize: 12)),
            ),
          ],
        ),
      );
}

/// 各格式能力表（阶段 1 梯队三·第 3 项）
///
/// 为什么不是写死的：能力位只在引擎的 `Caps` 里（`InArchive::caps()`），
/// 界面自己抄一份就等于第二本账 —— 引擎哪天改了能力，界面还在按老账本显示。
/// 所以这里每次进设置页都问引擎要（`--caps --json`），要不到就如实说读不到。
class _CapsTable extends StatefulWidget {
  const _CapsTable();

  @override
  State<_CapsTable> createState() => _CapsTableState();
}

class _CapsTableState extends State<_CapsTable> {
  List<Map<String, dynamic>>? _rows;
  String? _err;
  bool _loading = false;

  // ⚠️ 故意**不**在 initState 里加载：
  //   ① widget 测试的 fake_async 环境会把 `Process.run` 的定时器当成「未完成」判失败
  //   ② 每次进设置页就 spawn 一个进程也不划算（等 5 秒才出表格）
  //   改成点一下才读，用户想看才付出这点代价。
  Future<void> _load() async {
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
      return Row(
        children: [
          TextButton(
            onPressed: _loading ? null : _load,
            child: Text(_loading ? t('settings.caps.reading') : t('settings.caps.read')),
          ),
          if (_err != null)
            Expanded(
              child: Text(
                t('settings.caps.error', {'reason': '$_err'}),
                style: TextStyle(fontSize: 12, color: cs.error),
              ),
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
