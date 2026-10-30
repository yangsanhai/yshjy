import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../engine.dart';
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
  /// 处理完成后是否发桌面通知
  bool _notify = true;
  /// 解压完成后是否自动打开输出目录（默认关：批量解压会弹一堆窗口）
  bool _openAfter = false;
  // 解压后检验「移动设备是否真落盘」（默认关，见 docs/TODO.md 与 src/flush.rs）
  bool _flushMovable = false;
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
      if (!mounted) return;
      setState(() {
        _outDirCtrl.text = out['dir'] ?? '';
        _conflict = out['conflict'] ?? 'rename';
        _onDone = out['on_done'] ?? 'keep';
        _theme = gen['theme'] ?? 'auto';
        _notify = (gen['notify'] ?? 'on') != 'off';
        _openAfter = (gen['open_after'] ?? 'off') == 'on';
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
        'notify': _notify ? 'on' : 'off',
        'open_after': _openAfter ? 'on' : 'off',
        'close_action': _closeAction,
      });
      // 落盘检验开关：引擎读 [safety] 这一段（config.rs 的 flush_movable）
      updated = _upsertSection(updated, 'safety', {
        'flush_movable': _flushMovable ? 'on' : 'off',
      });
      Notifier.enabled = _notify; // 立刻生效，不用重启
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
    await Directory(path).create(recursive: true);
    await openDirectory(path);
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
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: const Text('处理完成后发桌面通知'),
          subtitle: Text(
            '解压/打包结束、或解压失败时给一条系统通知（需要系统有 notify-send）',
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _notify,
          onChanged: (v) => setState(() => _notify = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: const Text('解压完自动打开输出目录'),
          subtitle: Text(
            '默认关闭。批量解一堆包时不要开（会一个包弹一个窗口）；只解一个包想看结果时再打开，'
            '或者直接用完成页的「打开输出目录」按钮。',
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _openAfter,
          onChanged: (v) => setState(() => _openAfter = v),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          dense: true,
          title: const Text('解压到U盘/移动硬盘时，等数据真正写完再报成功'),
          subtitle: Text(
            '默认关闭。打开后：目标在U盘/移动硬盘上时，解完会多一步「正在检验落盘」，'
            '把数据真正写进设备才算完成——中途会显示「请勿拔出设备」。'
            '本机硬盘不受影响，一秒都不多等。\n'
            '注意：慢速U盘上可能要等几分钟（这块盘写 300MB 约 5 分钟）。',
            style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
          ),
          value: _flushMovable,
          onChanged: (v) => setState(() => _flushMovable = v),
        ),
        const SizedBox(height: 6),
        Text(
          '关着的时候也会提醒「拔盘前请先安全弹出」——提醒不花时间，但能救命。',
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
        ),
        DropdownButtonFormField<String>(
          initialValue: _closeAction,
          decoration: const InputDecoration(
            labelText: '关窗口时（正忙的话）',
            isDense: true,
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'tray', child: Text('缩到托盘（后台跑完）')),
            DropdownMenuItem(value: 'exit', child: Text('直接退出')),
          ],
          onChanged: (v) => setState(() => _closeAction = v ?? 'exit'),
        ),
        const SizedBox(height: 6),
        Text(
          Platform.isWindows
              ? '默认「直接退出」。选「缩到托盘」后，正忙时点关闭会藏进托盘继续跑，从托盘图标随时回来。'
              : '默认「缩到托盘」。正忙时点关闭会藏进托盘继续跑，从托盘菜单随时回来；空闲时照旧直接退出。',
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
        _section('各格式能力（问引擎要的，不是写死的）', cs),
        const _CapsTable(),
        const SizedBox(height: 18),
        _section('其它（改 config.toml）', cs),
        // T7-P5：以前这里并列写着 7 个语言代码，看着像「改 config.toml 就能切界面语言」，
        // 实际界面文案全是硬编码中文。直接说实话，别骗人。
        _kv('界面语言', '目前只有中文（引擎支持 7 种语言，界面文案还没抽成语言包）'),
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
            '· 界面多语言切换（要先把界面里 401 处硬编码中文抽成语言包，工作量大）\n'
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
          _err = '这个引擎版本太旧（hello 里没有 formats），升级引擎后就能看';
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
            child: Text(_loading ? '读取中…' : '读取各格式能力'),
          ),
          if (_err != null)
            Expanded(
              child: Text(
                '读不到能力表：$_err',
                style: TextStyle(fontSize: 12, color: cs.error),
              ),
            ),
        ],
      );
    }
    // ⚠️ Dart 的标识符只能是 ASCII —— 中文变量名编译不过（Rust 可以，Dart 不行）
    const capsLabels = <String, String>{
      'encrypted': '带密码',
      'encrypted_names': '密文件名',
      'split_volumes': '分卷',
      'selective': '可选解',
      'restores_mtime': '还原时间',
      'can_create': '可打包',
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
                    r['shape'] == 'volumes' ? '${r['format']}（分卷）' : '${r['format']}',
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
