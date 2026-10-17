import 'dart:io';

import 'package:flutter/material.dart';

/// 设置页
///
/// 现在引擎侧的配置项（语言 / 并发 / 重名策略 / 压缩比 / 可执行提醒 / 密码本）
/// 都写在 `~/.config/yshjy/config.toml` 里，**逐字段宽容**（写错一项只忽略那一项）。
/// 这一页先做到「看得见路径 + 一键打开」，避免做半成品的控件反而误导用户。
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

  String get _stateDir {
    final xdg = Platform.environment['XDG_STATE_HOME'];
    return '${xdg ?? '$_home/.local/state'}/yshjy';
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
          '配置文件是纯文本，改完下次处理就生效（不用重启）',
          style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
        ),
        const SizedBox(height: 16),
        _section('位置', cs),
        _row(
          icon: Icons.settings_outlined,
          title: '配置文件',
          subtitle: '$_configDir/config.toml',
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
        _section('有哪些可以改（改 config.toml）', cs),
        _kv('界面语言', 'zh-CN / zh-TW / en-US / ja-JP / ko-KR / ru-RU / es-ES'),
        _kv('并行任务数', 'jobs = 1（机械盘建议 1）'),
        _kv('默认输出目录', 'out_dir = "包旁边" 或某个绝对路径'),
        _kv('重名策略', 'conflict = "rename" / "skip" / "replace"'),
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
            '· 主题切换（现在跟随系统深/浅色）\n'
            '· 用界面控件直接改上面这些配置项（需要给协议加 config.set）',
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
