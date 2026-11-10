// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机端设置：只留手机上真正有意义的几项
//!
//! 桌面的设置页（745 行）里有一半在手机上不存在：托盘/关闭行为、磁盘类型（机械还是固态）、
//! 落盘检验、并行任务数（手机就一个存储）、快捷键……这里**不搬**。
//! 手机版只有：主题、语言、解压位置（看）、关于。

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/platform_paths.dart';
import 'package:yshjy_gui/common/theme_mode.dart';

class MobileSettingsPage extends StatefulWidget {
  const MobileSettingsPage({super.key});

  @override
  State<MobileSettingsPage> createState() => _MobileSettingsPageState();
}

class _MobileSettingsPageState extends State<MobileSettingsPage> {
  String? _version;

  @override
  void initState() {
    super.initState();
    _loadVersion();
  }

  Future<void> _loadVersion() async {
    try {
      final h = await EngineClient.instance.hello();
      if (mounted) setState(() => _version = h['version']?.toString());
    } catch (_) {
      if (mounted) setState(() => _version = null);
    }
  }

  /// 把 `[general] <key> = "<value>"` 写回 config.toml（只动这一个键，别的行原样保留）
  Future<void> _writeGeneral(String key, String value) async {
    final f = File('$configDir/config.toml');
    try {
      await f.parent.create(recursive: true);
      final lines = (await f.exists()) ? (await f.readAsString()).split('\n') : <String>[];
      var inGeneral = false;
      var found = false;
      var hasGeneral = false;
      for (var i = 0; i < lines.length; i++) {
        final t = lines[i].trim();
        if (t.startsWith('[') && t.endsWith(']')) {
          inGeneral = t == '[general]';
          if (inGeneral) hasGeneral = true;
          continue;
        }
        if (!inGeneral) continue;
        final eq = t.indexOf('=');
        if (eq <= 0) continue;
        if (t.substring(0, eq).trim() == key) {
          lines[i] = '$key = "$value"';
          found = true;
          break;
        }
      }
      if (!found) {
        if (!hasGeneral) lines.add('[general]');
        // 插到 [general] 头后面（找不到就追加到末尾）
        final idx = lines.indexWhere((l) => l.trim() == '[general]');
        if (idx >= 0) {
          lines.insert(idx + 1, '$key = "$value"');
        } else {
          lines.add('$key = "$value"');
        }
      }
      await f.writeAsString(lines.join('\n'));
    } catch (_) {
      // 写不进去不拦操作：这一次的选择照样立刻生效，只是下次启动恢复默认
    }
  }

  Future<void> _pickTheme(ThemeMode m) async {
    themeMode.value = m;
    await _writeGeneral('theme', themeModeName(m));
    if (mounted) setState(() {});
  }

  Future<void> _pickLanguage(String code) async {
    await switchLocale(code);
    await _writeGeneral('ui_language', code);
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(t('settings.title'))),
      body: ListView(
        children: [
          _section(context, t('settings.section.appearance')),
          _themeTile(ThemeMode.system, t('settings.theme.auto')),
          _themeTile(ThemeMode.light, t('settings.theme.light')),
          _themeTile(ThemeMode.dark, t('settings.theme.dark')),
          const Divider(),
          ListTile(
            leading: const Icon(Icons.translate),
            title: Text(t('settings.language')),
            subtitle: Text(
              uiLocaleSetting.value == 'system'
                  ? t('settings.theme.auto')
                  : (localeNames[uiLocaleSetting.value] ?? uiLocaleSetting.value),
            ),
            trailing: const Icon(Icons.chevron_right),
            onTap: _pickLocale,
          ),
          _section(context, t('settings.section.paths')),
          ListTile(
            leading: const Icon(Icons.folder_special_outlined),
            title: Text(t('settings.paths.state')),
            subtitle: Text(stateDir, style: const TextStyle(fontSize: 12)),
            trailing: IconButton(
              icon: const Icon(Icons.copy_all_outlined),
              tooltip: t('home.copyHowtoBtn'),
              onPressed: () => Clipboard.setData(ClipboardData(text: stateDir)),
            ),
          ),
          ListTile(
            leading: const Icon(Icons.tune),
            title: Text(t('settings.paths.config')),
            subtitle: Text(configDir, style: const TextStyle(fontSize: 12)),
          ),
          const Divider(),
          _section(context, t('about.license.title')),
          ListTile(
            leading: const Icon(Icons.info_outline),
            title: Text(t('shell.appName')),
            subtitle: Text(_version == null ? '—' : 'v$_version · MIT'),
          ),
          ListTile(
            leading: const Icon(Icons.code),
            title: Text(t('about.footer')),
            dense: true,
          ),
          const SizedBox(height: 24),
        ],
      ),
    );
  }

  Future<void> _pickLocale() async {
    final code = await showDialog<String>(
      context: context,
      builder: (ctx) => SimpleDialog(
        title: Text(t('settings.language')),
        children: [
          SimpleDialogOption(
            onPressed: () => Navigator.of(ctx).pop('system'),
            child: Text(t('settings.theme.auto')),
          ),
          for (final l in availableLocales)
            SimpleDialogOption(
              onPressed: () => Navigator.of(ctx).pop(l),
              child: Text(localeNames[l] ?? l),
            ),
        ],
      ),
    );
    if (code != null) await _pickLanguage(code);
  }

  /// 主题选项：用 ListTile + 勾号，不用 RadioListTile
  /// （Flutter 3.32 起 `groupValue/onChanged` 被标废弃，改成 RadioGroup 又要求新版本 API；
  /// 手机上这三行本来就是单选，一个勾号最直白。）
  Widget _themeTile(ThemeMode m, String label) => ListTile(
        title: Text(label),
        trailing: themeMode.value == m
            ? Icon(Icons.check, color: Theme.of(context).colorScheme.primary)
            : null,
        onTap: () => _pickTheme(m),
      );
  Widget _section(BuildContext context, String title) => Padding(        padding: const EdgeInsets.fromLTRB(16, 18, 16, 6),
        child: Text(
          title,
          style: Theme.of(context).textTheme.labelLarge?.copyWith(
                color: Theme.of(context).colorScheme.primary,
                fontWeight: FontWeight.w700,
              ),
        ),
      );
}
