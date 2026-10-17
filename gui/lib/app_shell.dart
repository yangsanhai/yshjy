import 'dart:io';

import 'package:flutter/material.dart';

import 'engine.dart';
import 'history_store.dart';
import 'pages/about_page.dart';
import 'pages/home_page.dart';
import 'pages/history_page.dart';
import 'pages/pack_page.dart';
import 'pages/settings_page.dart';

/// 应用外壳：左侧导航 + 内容区 + 状态栏
///
/// 为什么用左侧窄导航（NavigationRail）而不是顶部 Tab：
/// 窗口按"小工具"尺寸设计（约 30% 屏幕），横向空间紧；竖着放导航更省地方，
/// 也和 7-Zip 文件管理器 / Ark 这类工具的习惯一致。
class AppShell extends StatefulWidget {
  /// 启动时要直接打开的文件（右键菜单 / 命令行传进来）
  final String? initialFile;

  /// 是否在启动时拉起引擎进程。
  /// widget 测试里必须传 false —— 真起进程会在 fake_async 里留下 pending timer，
  /// 测试直接判失败（而且测试环境也没有引擎可执行文件）。
  final bool bootstrapEngine;

  const AppShell({super.key, this.initialFile, this.bootstrapEngine = true});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  final EngineClient _client = EngineClient.instance;
  int _index = 0;
  String _engineVersion = '…';
  String _status = '就绪';

  @override
  void initState() {
    super.initState();
    if (widget.bootstrapEngine) _bootstrap();
  }

  Future<void> _bootstrap() async {
    try {
      final h = await _client.hello();
      if (!mounted) return;
      setState(() {
        _engineVersion = (h['engine'] as String?) ?? '?';
      });
    } catch (e) {
      // 引擎没装、崩了、路径不对 —— 界面必须还能开，只在状态栏说明
      if (!mounted) return;
      setState(() => _status = '引擎未就绪（${e.runtimeType}）');
    }
  }

  void _setStatus(String s) {
    if (!mounted) return;
    setState(() => _status = s);
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    return Scaffold(
      body: Row(
        children: [
          NavigationRail(
            selectedIndex: _index,
            onDestinationSelected: (i) => setState(() => _index = i),
            labelType: NavigationRailLabelType.all,
            minWidth: 68,
            leading: Padding(
              padding: const EdgeInsets.only(top: 8, bottom: 12),
              child: Tooltip(
                message: '杨三海解压',
                child: CircleAvatar(
                  radius: 18,
                  backgroundColor: cs.primaryContainer,
                  child: Icon(Icons.inventory_2_outlined, size: 20, color: cs.onPrimaryContainer),
                ),
              ),
            ),
            destinations: const [
              NavigationRailDestination(
                icon: Icon(Icons.unarchive_outlined),
                selectedIcon: Icon(Icons.unarchive),
                label: Text('解压'),
              ),
              NavigationRailDestination(
                icon: Icon(Icons.archive_outlined),
                selectedIcon: Icon(Icons.archive),
                label: Text('打包'),
              ),
              NavigationRailDestination(
                icon: Icon(Icons.history),
                selectedIcon: Icon(Icons.history_toggle_off),
                label: Text('历史'),
              ),
              NavigationRailDestination(
                icon: Icon(Icons.settings_outlined),
                selectedIcon: Icon(Icons.settings),
                label: Text('设置'),
              ),
              NavigationRailDestination(
                icon: Icon(Icons.info_outline),
                selectedIcon: Icon(Icons.info),
                label: Text('关于'),
              ),
            ],
          ),
          VerticalDivider(width: 1, color: cs.outlineVariant.withValues(alpha: 0.4)),
          Expanded(
            child: Column(
              children: [
                Expanded(
                  child: IndexedStack(
                    index: _index,
                    children: [
                      HomePage(
                        initialFile: widget.initialFile,
                        onStatus: _setStatus,
                        autoStart: widget.bootstrapEngine,
                      ),
                      PackPage(onStatus: _setStatus),
                      const HistoryPage(),
                      const SettingsPage(),
                      AboutPage(engineVersion: _engineVersion),
                    ],
                  ),
                ),
                _StatusBar(engineVersion: _engineVersion, status: _status),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// 底部状态栏：一眼看到"引擎在不在、在干什么"
class _StatusBar extends StatelessWidget {
  final String engineVersion;
  final String status;

  const _StatusBar({required this.engineVersion, required this.status});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final dim = Theme.of(context).textTheme.bodySmall?.copyWith(
          fontSize: 11,
          color: cs.onSurfaceVariant,
        );
    final outDir = Platform.environment['HOME'] != null
        ? '${Platform.environment['HOME']}/.local/state/yshjy'
        : '';
    return Container(
      height: 26,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      decoration: BoxDecoration(
        color: cs.surfaceContainerHighest.withValues(alpha: 0.5),
        border: Border(top: BorderSide(color: cs.outlineVariant.withValues(alpha: 0.4))),
      ),
      child: Row(
        children: [
          Icon(Icons.circle, size: 7, color: cs.primary),
          const SizedBox(width: 6),
          Text(status, style: dim),
          const Spacer(),
          if (outDir.isNotEmpty)
            Tooltip(
              message: '崩溃日志 / 状态目录',
              child: Text('状态目录 $outDir', style: dim, overflow: TextOverflow.ellipsis),
            ),
          const SizedBox(width: 12),
          Text('引擎 $engineVersion', style: dim),
        ],
      ),
    );
  }
}
