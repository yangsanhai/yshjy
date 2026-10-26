import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'dart:ui' show AppExitResponse;

import 'engine.dart';
import 'platform_paths.dart';
import 'pages/about_page.dart';
import 'pages/home_page.dart';
import 'pages/history_page.dart';
import 'pages/pack_page.dart';
import 'pages/settings_page.dart';
import 'tray.dart';

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

class _AppShellState extends State<AppShell> with WidgetsBindingObserver {
  final EngineClient _client = EngineClient.instance;
  int _index = 0;
  String _engineVersion = '…';
  String _status = '就绪';

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (widget.bootstrapEngine) _bootstrap();
    // 首帧之后再弹引导（initState 里直接 showDialog 会拿不到 Navigator）
    WidgetsBinding.instance.addPostFrameCallback((_) => _maybeOnboarding());
  }

  /// 第一次打开时的三步引导（只弹一次；标记写在 state 目录）
  Future<void> _maybeOnboarding() async {
    try {
      final dir = Directory(stateDir);
      final flag = File('${dir.path}/onboarding_done');
      if (await flag.exists()) return;
      await dir.create(recursive: true);
      await flag.writeAsString('1');
      if (!mounted) return;
      await showDialog<void>(
        context: context,
        builder: (c) => AlertDialog(
          title: const Text('三步就会了'),
          content: const Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('1️⃣ 把「.mp4 / .png」拖进这个窗口 —— 不用管它真是什么'),
              SizedBox(height: 10),
              Text('2️⃣ 自动认格式、自动找解压码；认不出也会试着扫一遍'),
              SizedBox(height: 10),
              Text('3️⃣ 解压完会告诉你该点哪个文件启动'),
            ],
          ),
          actions: [
            FilledButton(
              onPressed: () => Navigator.pop(c),
              child: const Text('知道了'),
            ),
          ],
        ),
      );
    } catch (_) {
      // 引导失败不影响主流程
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  /// 关闭窗口时（D13 → D14）：正在处理就**藏到托盘**，让它在后台跑完。
  ///
  /// 背景（为什么以前是硬中断）：最早是直接断管道、引擎读 stdin 碰到 EOF
  /// 自己退出 —— 正在解的那个包会被硬中断，半成品留在那里也没人知道。
  /// 上一批改成「弹对话框问一句 → 确定后 cancel 引擎再退出」，让引擎在
  /// 数据块级停下并保留已解出的部分。
  ///
  /// 现在为什么不问了：对话框挡在中间 —— 用户明明只想「先别挡着我干活」，
  /// 却被要求在「继续处理 / 关闭」之间做选择，还一按「关闭」就等于把活停了。
  /// 而托盘已经在上一批做好了（顶栏图标 + 显示窗口/退出菜单），所以正忙时
  /// 关窗口直接**藏起来**：进程、引擎、正在解的包全都继续跑，用户从托盘
  /// 菜单点「显示窗口」随时回来，状态也写在托盘状态行里，一眼看得见。
  /// 引擎不忙的时候没什么可保的，照旧直接退出。
  @override
  Future<AppExitResponse> didRequestAppExit() async {
    if (!_client.busy) return AppExitResponse.exit;
    // 藏窗口 = 取消这次退出请求（return cancel），窗口关不掉是故意的：
    // 引擎还在跑，退出请求一旦放行进程就没了。
    await Tray.hideWindow();
    return AppExitResponse.cancel;
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
    // 托盘状态行是底部状态栏的第二个出口：窗口藏起来之后，用户只能从托盘
    // 看到进度，所以两边文字必须一致。
    // 这里不 await —— 状态更新是 UI 动作，不能等原生往返；托盘那边失败也
    // 无所谓（Tray.setStatus 已经把异常吃掉了）。
    unawaited(Tray.setStatus(s));
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
    final outDir = prettyPath(stateDir);
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
