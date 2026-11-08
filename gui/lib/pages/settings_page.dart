// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../engine.dart';
import '../key_action_scope.dart';
// KeyAction 枚举在这里（keyHelpRows 已随快捷键表搬去关于页，这个枚举还在用）
import '../keyboard.dart';
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
  /// 并行任务数（T17-2）：三档 **1 / 2 / 4**，默认 1（设计口径：机械盘不该被打成碎片）
  int _jobs = 1;
  /// 压缩比上限（T20）：解开后的体积最多是压缩包的几倍，超过就拦。
  ///
  /// **0 = 不检查，也是默认值**（2026-11-02 拍板）：参考项目 7-Zip / PeaZip /
  /// Ark / libarchive 都没有这道闸，只有「解压前算磁盘空间」那一道。
  /// 想更严的人可以自己选档位（100 / 1000 / 2000 倍）。
  int _ratioLimit = 0;
  /// 解出可执行文件时提醒（T17-2）：默认开，与引擎 config.rs 的 `warn_executables` 默认一致
  bool _warnExec = true;
  /// 本机写入盘类型：`solid` / `rotational` / `unknown`（引擎 hello.disk 真探测来的）
  ///
  /// ★ 只用于**给建议**，不参与任何判断。探不到就 unknown —— 绝不对用户说"你是固态"。
  String _disk = 'unknown';
  bool _loaded = false;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _load();
    // 磁盘类型单独拿（只影响一句建议文案；拿不到就用 unknown，不阻塞别的东西）
    _loadDisk();
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
        // 并行任务数（T17-2）：只认三档 1 / 2 / 4。配置被手改成别的数时**夹到最近的档位**
        // —— 下拉里放一个不在 items 里的值会直接 assert 崩（Flutter 的硬规则）。
        _jobs = _normalizeJobs(gen['jobs']);
        // 压缩比上限（T20）：只认 100 / 500 / 2000 / 0 四档。
        // 配置被手改成别的数（比如 300）→ 夹到最近的档，**绝不把原值塞进下拉**
        // （Dropdown 的 items 里没有的值会直接 assert 崩）。
        _ratioLimit = _normalizeRatio(saf['zip_bomb_ratio']);
        // 可执行提醒（T17-2）：默认开（引擎 config.rs 也是 true），只有明确写 off 才算关
        _warnExec = _readSwitch(saf['warn_executables'], dflt: true);
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

  /// 把配置里的 `jobs` 夹到三档（1 / 2 / 4）。
  ///
  /// 为什么往上取整不是往下：用户手写 3 说明他想开并行，但界面只提供 2 和 4 两档；
  /// 取 2 更保守（机械盘上并行的收益本来就小、代价是磁头来回跳）。
  static int _normalizeJobs(String? v) {
    final n = int.tryParse((v ?? '').trim()) ?? 1;
    if (n >= 4) return 4;
    if (n >= 2) return 2;
    return 1;
  }

  /// 把配置里的 `zip_bomb_ratio` 夹到四档（0 / 100 / 1000 / 2000）。
  ///
  /// 0 原样保留（= 不检查，默认）。其余往上取整到最近的档：
  /// 用户手写 300 说明他想设限，给 1000 而不是塞回 100 —— 那等于把设置抹掉。
  static int _normalizeRatio(String? v) {
    final n = int.tryParse((v ?? '').trim()) ?? 0;
    if (n <= 0) return 0; // 没写 / 0 / 负数 = 不检查
    if (n > 1000) return 2000;
    if (n > 100) return 1000;
    return 100;
  }

  /// 问引擎要「本机写入盘是机械还是固态」（T17-2 的那句建议靠它才不是假话）。
  ///
  /// 只影响一行提示文案：引擎没装 / 老引擎没 `hello.disk` / 探测失败
  /// → 全部当 `unknown`，界面照常能用，**绝不因此报错**。
  ///
  /// ⚠️ 要重试一次：应用刚启动时这次 hello 是**最先**打出去的（AppShell 和格式表
  /// 也各要一次），引擎是刚刚 fork 出来的、还没预热，真机上第一次会失败
  /// （用户 2026-11-01 截图里那句「本机磁盘类型没探到」就是它）。
  /// `hello()` 已经把并发请求合并成一次且失败不缓存，所以这里再试一次就能拿到。
  Future<void> _loadDisk() async {
    for (var attempt = 0; attempt < 2; attempt++) {
      try {
        final h = await EngineClient.instance.hello();
        if (!mounted) return;
        final d = h['disk'];
        setState(() {
          // 拿到响应就定下来：机器标识认不出（老引擎 / 探测不到）就如实说没探到
          _disk = (d == 'solid' || d == 'rotational') ? d as String : 'unknown';
        });
        return;
      } catch (_) {
        if (attempt == 1) return; // 两次都失败 → 保持 unknown，不给用户报错
        await Future<void>.delayed(const Duration(milliseconds: 300));
      }
    }
  }

  /// 把机器标识翻成用户看得懂的一句话（语言包里的三个键）
  String _diskText() {
    switch (_disk) {
      case 'solid':
        return t('settings.perf.diskSolid');
      case 'rotational':
        return t('settings.perf.diskRotational');
      default:
        return t('settings.perf.diskUnknown');
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

  /// 在原文里逐行更新某个段的键；段不存在就追加
  ///
  /// `numeric` 里的键**不加引号**写（`jobs = 2`）。为什么要这个参数：
  /// 引擎读 `jobs` 用的是 `as_integer()`，写成 `jobs = "2"` 会被静默当没写
  /// → 用户在界面选了并行数、其实一点没生效（本页最怕的「假开关」）。
  String _upsertSection(
    String text,
    String section,
    Map<String, String> vals, {
    Set<String> numeric = const {},
  }) {
    final lines = text.isEmpty ? <String>[] : text.split('\n');
    final result = <String>[];
    final seen = <String>{};
    var inOutput = false;
    var sectionFound = false;

    String render(String k, String v) =>
        numeric.contains(k) ? '$k = $v' : '$k = "$v"';

    void flushMissing() {
      for (final e in vals.entries) {
        if (!seen.contains(e.key)) {
          result.add(render(e.key, e.value));
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
          result.add(render(k, vals[k]!));
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
        result.add(render(e.key, e.value));
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
        // 并行任务数（T17-2）：引擎按整数读，必须写成 `jobs = 2` 而不是 `jobs = "2"`
        'jobs': '$_jobs',
      }, numeric: {'jobs'});
      // 落盘检验开关：引擎读 [safety] 这一段（config.rs 的 flush_movable）
      updated = _upsertSection(updated, 'safety', {
        'flush_movable': _flushMovable ? 'on' : 'off',
        // 可执行提醒（T17-2）：界面从「一行说明」改成真开关，关掉后引擎不再提醒
        'warn_executables': _warnExec ? 'on' : 'off',
        // 压缩比上限（T20）：引擎用 `as_integer()` 读，必须写成 `zip_bomb_ratio = 100`
        // 而不是 `= "100"` —— 写成字符串会被静默当没写（「假开关」）
        'zip_bomb_ratio': '$_ratioLimit',
      }, numeric: {'zip_bomb_ratio'});
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
    return KeyActionScope(
      // ★ takeFocus 必须 false：和首页在 IndexedStack 里同时挂着，抢焦点会把首页的 Enter 弄失灵。
      //   代价：「刚切过来没点过任何东西就按 Enter」不生效 —— 要保存必然先改过东西，不影响正常流程。
      takeFocus: false,
      // 与保存按钮 onPressed 同一个条件：正在存 / 还没读完配置就都不接
      handlers: {
        KeyAction.primary: _saving || !_loaded ? null : () => _save(),
      },
      // 保存按钮：右下角常驻的圆形按钮（2026-11-05 用户要求 —— 原来那个文字按钮
      // 夹在列表中间、跟着滚动跑，既不好找也不好看）。
      // 用 Stack 把 ListView 和按钮分开：按钮不随内容滚动，一直钉在右下角。
      child: Stack(
        children: [
          ListView(
            // 底部多留 88：不然滚到底时最后一节会被圆形按钮压住
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 88),
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
          // 选完**立刻生效**（和下面「界面语言」下拉同一个规矩）。
          // 以前只 `setState(_theme)`、把 `themeMode.value` 留到 `_save()` 里 ——
          // 用户切了深色/浅色看不到任何变化，以为主题功能坏了（2026-11-05 真机反馈）。
          // 回归：test/settings_theme_test.dart
          onChanged: (v) {
            final picked = v ?? 'auto';
            setState(() => _theme = picked);
            themeMode.value = parseThemeMode(picked);
          },
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
            // Windows 没有 notify-send（走托盘气泡），文案不能照抄 Linux
            t(Platform.isWindows
                ? 'settings.notify.hintWindows'
                : 'settings.notify.hint'),
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
            // Windows 没实现落盘检验（下面 onChanged 也禁掉）：文案不能照抄 Linux
            t(Platform.isWindows
                ? 'settings.flush.hintWindows'
                : 'settings.flush.hint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _flushMovable,
          // Windows 版没有这套落盘检验（is_removable 与等待逻辑都是 Linux 专属）：
          // 开关直接禁用并说清楚，别让用户以为打开就生效。
          onChanged: Platform.isWindows
              ? null
              : (v) => setState(() => _flushMovable = v),
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
        // 「关着也会提醒拔盘」只在 Linux 成立；Windows 那条路径根本不会跑
        if (!Platform.isWindows)
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
        // 「支持的格式」表已搬到关于页（2026-11-05 用户要求：那些不是设置）——
        // 见 pages/reference_tables.dart 的 CapsTable
        _section(t('settings.section.perf'), cs),
        // 并行任务数（T17-2）：三档下拉。
        //
        // 为什么不按 CPU 核数自动定档：并行的瓶颈在**磁盘**不在 CPU —— 机械盘多开
        // 反而更慢（磁头来回跳）；而"探测完自动给个值"在廉价固态 / 网络盘上会猜错，
        // 猜错比不猜更糟。所以给档位 + 把怎么选写清楚，值由用户自己定。
        DropdownButtonFormField<String>(
          key: const ValueKey('jobs-select'),
          initialValue: '$_jobs',
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.perf.jobs'),
            helperText: t('settings.perf.jobsHint'),
            helperMaxLines: 3,
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(value: '1', child: Text(t('settings.perf.jobs1'))),
            DropdownMenuItem(value: '2', child: Text(t('settings.perf.jobs2'))),
            DropdownMenuItem(value: '4', child: Text(t('settings.perf.jobs4'))),
          ],
          onChanged: (v) => setState(() => _jobs = _normalizeJobs(v)),
        ),
        const SizedBox(height: 12),
        // 压缩比上限（T20）：防压缩炸弹的那道闸，默认 100 倍。
        // 为什么要给用户改：大日志 / CSV / 稀疏镜像的压缩比轻松过百，
        // 默认值会把它们拦下来；以前还会误报成「数据损坏」，让人以为包坏了。
        DropdownButtonFormField<String>(
          key: const ValueKey('ratio-select'),
          initialValue: '$_ratioLimit',
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('settings.safety.ratio'),
            helperText: t('settings.safety.ratioHint'),
            helperMaxLines: 3,
            isDense: true,
            border: const OutlineInputBorder(),
          ),
          items: [
            DropdownMenuItem(
                value: '0', child: Text(t('settings.safety.ratioOff'))),
            DropdownMenuItem(
                value: '100', child: Text(t('settings.safety.ratio100'))),
            DropdownMenuItem(
                value: '1000', child: Text(t('settings.safety.ratio1000'))),
            DropdownMenuItem(
                value: '2000', child: Text(t('settings.safety.ratio2000'))),
          ],
          onChanged: (v) => setState(() => _ratioLimit = _normalizeRatio(v)),
        ),
        const SizedBox(height: 6),
        // 本机磁盘类型（引擎 hello.disk 真探测来的，不是写死的文案）
        Row(
          children: [
            Icon(Icons.storage_outlined, size: 14, color: cs.onSurfaceVariant),
            const SizedBox(width: 6),
            Expanded(
              child: Text(
                _diskText(),
                style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
              ),
            ),
          ],
        ),
        // 可执行提醒（T17-2）：原来只是一行说明文字，现在是真的开关。
        // 引擎侧 `config.rs` 的 `warn_executables` 一直在读，只是界面从来没给出入口。
        SwitchListTile(
          key: const ValueKey('warn-exec-toggle'),
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: Text(t('settings.perf.warn')),
          subtitle: Text(
            t('settings.perf.warnHint'),
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _warnExec,
          onChanged: (v) => setState(() => _warnExec = v),
        ),
        // 「快捷键」表已搬到关于页（2026-11-05）—— 见 pages/reference_tables.dart 的 KeysTable
      ],
      ),
          Positioned(
            right: 20,
            bottom: 20,
            child: Tooltip(
              // 键位就地提示（T11）：Enter = 保存（焦点不在输入框里时；
              // 在输入框里按 Enter 属于输入框自己，不会被截走）
              message: 'Enter',
              child: FloatingActionButton(
                key: const ValueKey('settings-save'),
                // 圆形按钮没有文字，tooltip 同时充当无障碍名字和悬停提示
                tooltip: _saving ? t('settings.saving') : t('settings.save'),
                onPressed: _saving || !_loaded ? null : _save,
                // ⚠️ 不转圈：CircularProgressIndicator 是无限动画，会让 pumpAndSettle 等到超时
                //    （本项目在设置页踩过这个坑）。保存很快，沙漏图标 + 禁用态足够。
                child: Icon(
                  _saving ? Icons.hourglass_top : Icons.save_outlined,
                  size: 22,
                ),
              ),
            ),
          ),
        ],
      ),
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
}
