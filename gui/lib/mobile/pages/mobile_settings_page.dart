// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 手机端设置：只留手机上真正有意义的几项
//!
//! 桌面的设置页（745 行）里有一半在手机上不存在：托盘/关闭行为、磁盘类型（机械还是固态）、
//! 落盘检验、并行任务数（手机就一个存储）、快捷键……这里**不搬**。
//! 手机版只有：主题、语言、**解压位置**、路径、关于。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/l10n.dart';
import 'package:yshjy_gui/common/mobile_prefs.dart';
import 'package:yshjy_gui/common/platform_paths.dart';
import 'package:yshjy_gui/common/theme_mode.dart';

class MobileSettingsPage extends StatefulWidget {
  const MobileSettingsPage({super.key});

  @override
  State<MobileSettingsPage> createState() => _MobileSettingsPageState();
}

class _MobileSettingsPageState extends State<MobileSettingsPage>
    with WidgetsBindingObserver {
  String? _version;

  /// 解到系统「下载」目录（需要「所有文件访问」）
  bool _toDownload = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _loadVersion();
    _loadPrefs();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  /// 从系统设置页回来时刷新（用户可能刚把「所有文件访问」勾上/取消）
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _loadPrefs();
  }

  Future<void> _loadPrefs() async {
    await MobilePrefs.refreshOut();
    if (!mounted) return;
    setState(() => _toDownload = MobilePrefs.outToDownloadActive.value);
  }

  Future<void> _loadVersion() async {
    try {
      final h = await EngineClient.instance.hello();
      if (mounted) setState(() => _version = h['version']?.toString());
    } catch (_) {
      if (mounted) setState(() => _version = null);
    }
  }

  Future<void> _toggleDownload(bool v) async {
    if (v) {
      if (!await MobilePrefs.hasAllFiles()) {
        // 这条权限没有「弹窗申请」，只能跳系统页让用户勾
        await MobilePrefs.requestAllFiles();
        _toast(t('mobile.out.needPermission'));
        return;
      }
    }
    await MobilePrefs.setOutToDownload(v);
    await MobilePrefs.refreshOut();
    if (mounted) setState(() => _toDownload = MobilePrefs.outToDownloadActive.value);
  }

  void _toast(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _pickTheme(ThemeMode m) async {
    themeMode.value = m;
    await MobilePrefs.writeGeneral('theme', themeModeName(m));
    if (mounted) setState(() {});
  }

  Future<void> _pickLanguage(String code) async {
    await switchLocale(code);
    await MobilePrefs.writeGeneral('ui_language', code);
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
          const Divider(),
          _section(context, t('settings.section.output')),
          SwitchListTile(
            secondary: const Icon(Icons.download_outlined),
            title: Text(t('mobile.out.download')),
            subtitle: Text(t('mobile.out.download.hint')),
            value: _toDownload,
            onChanged: _toggleDownload,
          ),
          _section(context, t('settings.section.paths')),
          ListTile(
            leading: const Icon(Icons.folder_special_outlined),
            title: Text(t('settings.paths.state')),
            subtitle: Text(stateDir, style: const TextStyle(fontSize: 12)),
            trailing: IconButton(
              icon: const Icon(Icons.copy_all_outlined),
              tooltip: t('home.copyHowtoBtn'),
              onPressed: () {
                Clipboard.setData(ClipboardData(text: stateDir));
                _toast(t('home.copiedSend'));
              },
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

  Widget _section(BuildContext context, String title) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 18, 16, 6),
        child: Text(
          title,
          style: Theme.of(context).textTheme.labelLarge?.copyWith(
                color: Theme.of(context).colorScheme.primary,
                fontWeight: FontWeight.w700,
              ),
        ),
      );
}
