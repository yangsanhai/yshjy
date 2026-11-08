// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'dart:async';
import 'dart:io';
import 'app_settings.dart';

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
import 'l10n.dart';
import 'theme_mode.dart';

/// 托盘状态行的一次决策结果
///
/// - [text]：这次要写进托盘的文字；null = 这次**不写**（没到该写的时候）
/// - [lastPct]：更新后的节流基准（上次真正写出去的百分比；-1 = 还没写过）
typedef TrayStatus = ({String? text, double lastPct});

/// 节流步长：百分比变化不到 2 就不重发托盘
///
/// 为什么是 2：进度事件是**每块数据**一次（几 MB 一条），一个几百 MB 的包
/// 能刷出几千条。托盘状态行是原生 GTK 菜单项，每次 set_status 都要过一趟
/// MethodChannel —— 全发的话只是浪费，用户还看不出区别（1% 的跳动谁看得见）。
/// 2% 是「用户能看出在动」和「别刷屏」之间的折中。
const double kTrayThrottleStep = 2.0;

/// 引擎事件 → 托盘状态行文案（**唯一真源**）
///
/// 为什么单独抽成纯函数：这段逻辑是「规则」而不是「状态机」——
/// 规则可以脱离界面单测，界面只负责把结果搬到托盘上。
///
/// 规则（照着写，别另设一套）：
/// 1. `EXTRACT_PROGRESS` / `PACK_PROGRESS` 且读得出百分比 → 「正在解压 N%」/「正在打包 N%」；
///    百分比涨了至少 [kTrayThrottleStep] 才发（100% 一定发 —— 那是用户最关心的）
/// 2. `JOB_FINISHED` → 「空闲」，并把节流基准重置成 -1（下一个任务从头算）
/// 3. 其他事件 → 不写托盘（null）
TrayStatus trayStatusForEvent(EngineEvent e, double lastPct) {
  switch (e.kind) {
    case 'EXTRACT_PROGRESS':
    case 'PACK_PROGRESS':
      final f = e.progress?.fraction;
      if (f == null) {
        // 总字节还不知道（total_bytes == 0）→ 没有百分比可言，不写
        return (text: null, lastPct: lastPct);
      }
      final pct = (f * 100).floor().clamp(0, 100).toInt();
      // 已经写过百分比、且涨得不够 → 跳过这一条
      if (pct < 100 && lastPct >= 0 && pct - lastPct < kTrayThrottleStep) {
        return (text: null, lastPct: lastPct);
      }
      final verb = e.kind == 'PACK_PROGRESS' ? t('shell.status.packing') : t('shell.status.extracting');
      return (text: '$verb $pct%', lastPct: pct.toDouble());
    case 'JOB_FINISHED':
      // 任务收工：托盘回到空闲，节流基准也清零（下一单从头算百分比）
      return (text: t('shell.status.idle'), lastPct: -1);
    default:
      return (text: null, lastPct: lastPct);
  }
}

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
  /// 状态栏文字。**null 表示「还没收到任何状态」**，此时显示「就绪」。
  ///
  /// 为什么不用 `String _status = t('shell.status.ready')`：那样在 State 创建时
  /// 就把当时的语言定死了，用户切界面语言后状态栏会一直停在旧语言（真机撞到过：
  /// 英文界面下底部仍写着「就绪」）。存 null 让它在 build 里现算，切语言就跟着变。
  String? _statusRaw;

  /// 引擎事件订阅（托盘进度行的数据源）
  StreamSubscription<EngineEvent>? _eventSub;

  /// 上次真正写进托盘的百分比（-1 = 还没写过）
  ///
  /// 这是节流基准：进度事件很密，只有涨够步长才值得过一趟原生通道。
  double _lastTrayPct = -1;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (widget.bootstrapEngine) _bootstrap();
    // 订阅引擎事件：托盘状态行要显示**真实进度**，不能只抄底部状态栏
    // （底部状态栏是给人看的碎文字：「找到 3 个可解包」之类，跟进度无关）。
    // 只订阅一次 —— 引擎事件流是广播流，多订一份就多一份开销。
    // EngineClient.instance 是静态单例，永远可用（不会 null）。
    _eventSub = _client.events.listen(_onEngineEvent);
    // 托盘「退出」：先让 Dart 收拾，再让原生真退（见 _onQuitRequest 的说明）
    Tray.onQuitRequest(_onQuitRequest);
    // 标题栏深浅跟着界面主题走（Windows 专属；设置页切主题后立刻生效）
    themeMode.addListener(_syncTitleBar);
    // 首帧之后再弹引导（initState 里直接 showDialog 会拿不到 Navigator）
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _syncTitleBar();
      _maybeOnboarding();
    });
  }

  /// 把 Windows 标题栏的深浅同步成当前界面主题。
  ///
  /// 为什么单独做：标题栏是系统画的，Flutter 的 ThemeMode 管不到它；
  /// 「跟随系统」时系统切换深浅色也得跟着变，所以还要接 didChangePlatformBrightness。
  void _syncTitleBar() {
    if (!Platform.isWindows) return;
    final bool sysDark =
        WidgetsBinding.instance.platformDispatcher.platformBrightness ==
            Brightness.dark;
    final bool dark = switch (themeMode.value) {
      ThemeMode.dark => true,
      ThemeMode.light => false,
      ThemeMode.system => sysDark,
    };
    Tray.setTitleBarDark(dark);
  }

  @override
  void didChangePlatformBrightness() {
    _syncTitleBar();
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
          title: Text(t('shell.onboard.title')),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(t('shell.onboard.step1')),
              const SizedBox(height: 10),
              Text(t('shell.onboard.step2')),
              const SizedBox(height: 10),
              Text(t('shell.onboard.step3')),
            ],
          ),
          actions: [
            FilledButton(
              onPressed: () => Navigator.pop(c),
              child: Text(t('shell.onboard.ok')),
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
    themeMode.removeListener(_syncTitleBar);
    _eventSub?.cancel();
    // 撤掉退出回调：界面都没了，再收到「请退出」没人接得住，
    // 而且回调里会碰 _client / setState，打到废对象上会炸。
    Tray.onQuitRequest(null);
    super.dispose();
  }

  /// 托盘「退出」：先把引擎安顿好，再让原生真退。
  ///
  /// 为什么退出要绕一圈 Dart（这是这一整段的核心）：
  /// 引擎是**子进程**，它靠我们这条 stdin 管道活着。进程一没，引擎读 stdin
  /// 立刻撞 EOF，自己就退了 —— 正在解的那一项被硬中断，只剩一个
  /// .yshjy-part 半成品（原子写入兜底，但不算干净）。
  ///
  /// 所以「退出」不能由原生单方面拍板：原生先问 Dart，Dart 说「可以退」
  /// 才真的退。中间这段时间正好用来干两件事：
  /// 1. `_client.cancel()` —— 让引擎在**数据块边界**停下（不是立刻砍断），
  ///    已解出来的部分完整保留；
  /// 2. 等 400ms —— 引擎取消是异步的，得给它收尾的时间，
  ///    免得进程先没、引擎还在往半截文件上写。
  ///
  /// 引擎不忙的时候没什么可保的，直接退。
  Future<void> _onQuitRequest() async {
    if (_client.busy) {
      await _client.cancel();
      await Future<void>.delayed(const Duration(milliseconds: 400));
    }
    await Tray.quitNow();
  }

  /// 引擎事件来了：按规则决定要不要更新托盘状态行
  ///
  /// 规则在 [trayStatusForEvent] 里（唯一真源），这里只负责把结果搬过去。
  void _onEngineEvent(EngineEvent e) {
    final r = trayStatusForEvent(e, _lastTrayPct);
    _lastTrayPct = r.lastPct;
    final text = r.text;
    if (text == null) return;
    // 不 await：进度事件很密，不能让原生往返把事件流堵住。
    // Tray.setStatus 自己把异常吃掉了，失败无所谓（托盘只是锦上添花）。
    unawaited(Tray.setStatus(text));
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
    // 正忙时关窗口：动作由设置页的「关窗口时」决定（默认分平台，见 app_settings）。
    // 「缩到托盘」= 取消这次退出请求，把窗口藏起来让引擎后台跑完；
    // 「直接退出」= 先 cancel 引擎、给它 400ms 在数据块边界收尾（与托盘「退出」
    // 菜单同一条路），再放行 —— 用户明确选了退出，强扭的瓜不甜。
    final action = await closeActionSetting();
    if (action == 'exit') {
      await _client.cancel();
      await Future<void>.delayed(const Duration(milliseconds: 400));
      return AppExitResponse.exit;
    }
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
      setState(() => _statusRaw = t('shell.engine.notReady', {'type': '${e.runtimeType}'}));
    }
  }

  void _setStatus(String s) {
    if (!mounted) return;
    setState(() => _statusRaw = s);
    // 底部状态栏的文字**不再**直接抄进托盘。
    //
    // 为什么：底部状态栏说的是「人话流水账」（「找到 3 个可解包」「已识别：12 个文件」…），
    // 托盘状态行只有一个位置、宽度有限，而且是窗口藏起来之后用户唯一的进度出口 ——
    // 那里应该是**进度**（正在解压 42%），不是碎碎念。所以托盘只接受两种：
    //   ① 引擎事件算出来的真实百分比（见 _onEngineEvent）
    //   ② 引擎忙但还没有百分比时的「正在处理…」
    // 别的状态一律不上托盘，免得把进度行冲掉。
    //
    // 「正在处理…」只在「引擎正忙、且还没有百分比」时才写：
    // 有百分比之后就不再抢（不然「正在处理…」会把「正在解压 42%」盖掉）。
    if (_client.busy && _lastTrayPct < 0) {
      unawaited(Tray.setStatus(t('shell.working')));
    }
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
                message: t('shell.appName'),
                child: CircleAvatar(
                  radius: 18,
                  backgroundColor: cs.primaryContainer,
                  child: Icon(Icons.inventory_2_outlined, size: 20, color: cs.onPrimaryContainer),
                ),
              ),
            ),
            destinations: [
              NavigationRailDestination(
                icon: const Icon(Icons.unarchive_outlined),
                selectedIcon: const Icon(Icons.unarchive),
                label: Text(t('shell.nav.extract')),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.archive_outlined),
                selectedIcon: const Icon(Icons.archive),
                label: Text(t('shell.nav.pack')),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.history),
                selectedIcon: const Icon(Icons.history_toggle_off),
                label: Text(t('shell.nav.history')),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.settings_outlined),
                selectedIcon: const Icon(Icons.settings),
                label: Text(t('shell.nav.settings')),
              ),
              NavigationRailDestination(
                icon: const Icon(Icons.info_outline),
                selectedIcon: const Icon(Icons.info),
                label: Text(t('shell.nav.about')),
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
                        // 只有首页在前台时才接拖放：两页都是铺满整页的 DropTarget，
                        // 不按前台页关掉的话，在打包页拖文件会被首页抢走。
                        acceptDrop: _index == 0,
                      ),
                      PackPage(
                        onStatus: _setStatus,
                        acceptDrop: _index == 1,
                      ),
                      // 不能加 const：切界面语言时父级 rebuild，同一个 const 实例会被
                      // Flutter 判定为「没变化」而跳过重建 —— 历史页内容会停在旧语言。
                      // ignore: prefer_const_constructors
                      HistoryPage(),
                      // 同上：设置页也要能跟着语言重建。
                      // ignore: prefer_const_constructors
                      SettingsPage(),
                      AboutPage(engineVersion: _engineVersion),
                    ],
                  ),
                ),
                _StatusBar(
                  engineVersion: _engineVersion,
                  status: _statusRaw ?? t('shell.status.ready'),
                ),
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
            // 必须包 Flexible：Text 的 ellipsis 只有在拿到**有限宽度**时才生效，
            // 直接放进 Row 会按内容宽度撑开 → 路径一长就 `RIGHT OVERFLOWED`
            // （2026-11-01 测试里用长临时目录当场撞出来，真机上长用户名/深路径同样会中）。
            Flexible(
              child: Tooltip(
                message: t('shell.tray.logDir'),
                child: Text(
                  t('shell.tray.stateDir', {'path': outDir}),
                  style: dim,
                  overflow: TextOverflow.ellipsis,
                  maxLines: 1,
                ),
              ),
            ),
          const SizedBox(width: 12),
          Text(t('shell.tray.engine', {'version': engineVersion}), style: dim),
        ],
      ),
    );
  }
}
