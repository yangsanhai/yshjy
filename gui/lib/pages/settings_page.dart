import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

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
  String get _home => Platform.environment['HOME'] ?? '.';

  String get _configDir {
    final xdg = Platform.environment['XDG_CONFIG_HOME'];
    return '${xdg ?? '$_home/.config'}/yshjy';
  }

  String get _configPath => '$_configDir/config.toml';

  String get _stateDir {
    final xdg = Platform.environment['XDG_STATE_HOME'];
    return '${xdg ?? '$_home/.local/state'}/yshjy';
  }

  final _outDirCtrl = TextEditingController();
  String _conflict = 'rename';
  String _onDone = 'keep';
  /// 主题：auto（跟随系统）/ light / dark
  String _theme = 'auto';
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
      if (!mounted) return;
      setState(() {
        _outDirCtrl.text = out['dir'] ?? '';
        _conflict = out['conflict'] ?? 'rename';
        _onDone = out['on_done'] ?? 'keep';
        _theme = gen['theme'] ?? 'auto';
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
      updated = _upsertSection(updated, 'general', {'theme': _theme});
      await f.parent.create(recursive: true);
      await f.writeAsString(updated);
      themeMode.value = parseThemeMode(_theme);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('已保存，下次处理就生效（不用重启）')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('保存失败：$e')),
      );
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _pickDir() async {
    try {
      final dir = await getDirectoryPath(confirmButtonText: '就用这个目录');
      if (dir != null && mounted) {
        setState(() => _outDirCtrl.text = dir);
      }
    } catch (_) {}
  }

  Future<void> _open(String path) async {
    try {
      await Directory(path).create(recursive: true);
      await Process.run('xdg-open', [path]);
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('设置', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 4),
        Text(
          '改完点保存，下次处理就生效（不用重启）',
          style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 18),
        _section('外观', cs),
        DropdownButtonFormField<String>(
          initialValue: _theme,
          decoration: const InputDecoration(
            labelText: '主题',
            isDense: true,
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'auto', child: Text('跟随系统（推荐）')),
            DropdownMenuItem(value: 'light', child: Text('浅色')),
            DropdownMenuItem(value: 'dark', child: Text('深色')),
          ],
          onChanged: (v) => setState(() => _theme = v ?? 'auto'),
        ),
        const SizedBox(height: 18),
        _section('解压输出', cs),
        const SizedBox(height: 6),
        TextField(
          controller: _outDirCtrl,
          decoration: InputDecoration(
            labelText: '默认输出目录（留空 = 解到包旁边）',
            hintText: '例如 /home/你/下载/解压结果',
            isDense: true,
            border: const OutlineInputBorder(),
            prefixIcon: const Icon(Icons.folder_outlined, size: 18),
            suffixIcon: TextButton(
              onPressed: _pickDir,
              child: const Text('选择…'),
            ),
          ),
        ),
        const SizedBox(height: 12),
        Row(
          children: [
            Expanded(
              child: DropdownButtonFormField<String>(
                initialValue: _conflict,
                decoration: const InputDecoration(
                  labelText: '遇到同名文件',
                  isDense: true,
                  border: OutlineInputBorder(),
                ),
                items: const [
                  DropdownMenuItem(value: 'rename', child: Text('自动改名（推荐）')),
                  DropdownMenuItem(value: 'skip', child: Text('跳过已有的')),
                  DropdownMenuItem(value: 'replace', child: Text('覆盖'),
                  ),
                ],
                onChanged: (v) => setState(() => _conflict = v ?? 'rename'),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: DropdownButtonFormField<String>(
                initialValue: _onDone,
                decoration: const InputDecoration(
                  labelText: '解压完原包',
                  isDense: true,
                  border: OutlineInputBorder(),
                ),
                items: const [
                  DropdownMenuItem(value: 'keep', child: Text('保留（推荐）')),
                  DropdownMenuItem(value: 'trash', child: Text('移入回收站')),
                ],
                onChanged: (v) => setState(() => _onDone = v ?? 'keep'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 6),
        Text(
          '「移入回收站」只放进系统回收站（可还原），本工具从不永久删除文件。',
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 14),
        Align(
          alignment: Alignment.centerRight,
          child: FilledButton.icon(
            onPressed: _saving || !_loaded ? null : _save,
            icon: const Icon(Icons.save_outlined, size: 18),
            label: Text(_saving ? '保存中…' : '保存'),
          ),
        ),
        const SizedBox(height: 20),
        _section('位置', cs),
        _row(
          icon: Icons.settings_outlined,
          title: '配置文件',
          subtitle: _configPath,
          action: TextButton(
            onPressed: () => _open(_configDir),
            child: const Text('打开目录'),
          ),
        ),
        _row(
          icon: Icons.key_outlined,
          title: '密码本',
          subtitle: '$_configDir/passwords.txt（一行一个，导入时自动去重）',
          action: TextButton(
            onPressed: () => _open(_configDir),
            child: const Text('打开目录'),
          ),
        ),
        _row(
          icon: Icons.bug_report_outlined,
          title: '状态目录',
          subtitle: '$_stateDir（崩溃日志、历史记录）',
          action: TextButton(
            onPressed: () => _open(_stateDir),
            child: const Text('打开目录'),
          ),
        ),
        const SizedBox(height: 18),
        _section('其它（改 config.toml）', cs),
        _kv('界面语言', 'zh-CN / zh-TW / en-US / ja-JP / ko-KR / ru-RU / es-ES'),
        _kv('并行任务数', 'jobs = 1（机械盘建议 1）'),
        _kv('压缩比上限', 'zip_bomb_ratio = 100（防炸弹）'),
        _kv('可执行提醒', 'warn_executables = true'),
        _kv('自动抓解压码', 'auto_from_notes = true（从旁边的说明文件里找）'),
        const SizedBox(height: 18),
        _section('还没做的（我不装样子）', cs),
        Container(
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            color: cs.surfaceContainerHighest,
            borderRadius: BorderRadius.circular(10),
          ),
          child: Text(
            '· 界面文案多语言切换（引擎已支持 7 种语言，界面文案还没抽成语言包）\n'
            '· 主题切换（现在跟随系统深/浅色）',
            style: TextStyle(fontSize: 12, height: 1.7, color: cs.onSurfaceVariant),
          ),
        ),
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
