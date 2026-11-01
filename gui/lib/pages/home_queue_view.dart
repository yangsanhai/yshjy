/// 批量解压队列（S13）—— 从 `home_page.dart` 抽出来的第一块。
///
/// **为什么先抽这一块**：原文件 3117 行，其中 2976 行是同一个 State 类的实现，
/// 改界面得在里头翻。这块是所有视图里最独立的 —— 只依赖「一个队列列表 + 六个回调」，
/// 对比 `_previewView` 要引用 38 个不同的成员。
///
/// **做法**：逻辑逐字搬移，只把 `_queue` / `_foldersScanned` / `setState(…removeAt…)`
/// 这类对 State 的直连换成构造参数与回调。搬完 `home_page.dart` 里只剩一行转调。
///
/// **★ 键盘导航的第二次改法（真机抓出来的）**：
/// 第一版给每一行挂 `Focus(autofocus: i == 0)` —— widget 测试全绿，**真机按什么都没反应**。
/// 两个原因：
///   ① `autofocus` 只在行**首次挂载**时请求焦点，真实流程是「拖文件 → 队列才出现」，
///      那时焦点已经在别处，`autofocus` 抢不到；测试里把 `QueueView` 当根 widget 挂，
///      必然抢得到，所以测试是假绿。
///   ② 就算焦点移动了，`ListTile` 默认**几乎没有焦点样式** —— 用户按 ↓ 屏幕上看不出
///      任何变化，会以为程序坏了。
/// 现在改成列表键盘导航的标准做法：**整块一个 `Focus` + 内部选中索引 + 选中行高亮**，
/// 并且挂载后第一帧主动 `requestFocus()`。对应回归：
/// `test/home_queue_keyboard_integration_test.dart`（用真实 HomePage 结构验，不是把这块当根挂）。
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import '../format.dart';
import '../l10n.dart';

/// 队列里的一行（S13 批量队列）
class QueueItem {
  final String path;
  final String name;
  final int size;
  final String kind;
  final int volumes;
  final bool archive;

  /// S10 多候选：这个文件里像是有几处藏着东西
  ///
  /// 正常包永远只有一条（`chosen`）—— `length >= 2` 才值得给用户看选择界面。
  final List<ArchiveCandidate> candidates;

  /// pending / running / ok / partial / failed / cancelled
  String status = 'pending';

  /// 引擎给这一行的任务号（`JOB_STARTED` 里带回来）；还没开始跑就是 null
  ///
  /// D5 用：单独取消某一行时要把这个号发给引擎，引擎才能只跳这一条。
  int? jobNo;

  QueueItem({
    required this.path,
    required this.name,
    required this.size,
    required this.kind,
    required this.volumes,
    required this.archive,
    this.candidates = const [],
  });

  /// 是不是「一个文件里好几处都像藏着东西」
  bool get hasManyCandidates => candidates.length >= 2;
}

/// 批量解压队列
class QueueView extends StatefulWidget {
  const QueueView({
    super.key,
    required this.queue,
    required this.foldersScanned,
    required this.onReset,
    required this.onRemove,
    required this.onCancelRow,
    required this.onTryOtherCandidates,
    required this.onStartAll,
  });

  final List<QueueItem> queue;

  /// 拖进来的是文件夹时 > 0，用来显示「会怎么处理」那行提示
  final int foldersScanned;

  final VoidCallback onReset;
  final void Function(int index) onRemove;
  final void Function(int index) onCancelRow;
  final void Function(int index) onTryOtherCandidates;
  final VoidCallback onStartAll;

  @override
  State<QueueView> createState() => _QueueViewState();
}

class _QueueViewState extends State<QueueView> {
  /// 键盘导航的当前行（鼠标点某行也会把它同步过来）
  int _sel = 0;

  final FocusNode _focus = FocusNode(debugLabel: 'queue-list');

  @override
  void initState() {
    super.initState();
    // ★ 不靠行上的 autofocus：真实流程里（拖文件 → 队列出现）焦点已经在别处，
    //   autofocus 抢不到 —— 第一版真机按键盘全无反应就是这么来的。
    //   挂载后第一帧主动要焦点，队列一出现键盘就能用。
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focus.requestFocus();
    });
  }

  @override
  void didUpdateWidget(QueueView oldWidget) {
    super.didUpdateWidget(oldWidget);
    // 父层删行 / 清空队列之后，选中索引不能越界
    if (widget.queue.isEmpty) {
      _sel = 0;
    } else if (_sel >= widget.queue.length) {
      _sel = widget.queue.length - 1;
    }
  }

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final key = event.logicalKey;
    final n = widget.queue.length;

    // Del / Backspace 移出当前选中的那行（D4：以前只能整队清空）
    if (key == LogicalKeyboardKey.delete ||
        key == LogicalKeyboardKey.backspace) {
      if (n == 0) return KeyEventResult.ignored;
      final i = _sel.clamp(0, n - 1);
      setState(() {
        // 删的是最后一行就往前退一格，否则停在同一位置看下一项
        if (_sel >= n - 1) _sel = n >= 2 ? n - 2 : 0;
      });
      widget.onRemove(i);
      return KeyEventResult.handled;
    }

    // ↑ / ↓ 在行间移动选中（选中行会高亮 + 左边一条竖线，看得见）
    if (key == LogicalKeyboardKey.arrowDown) {
      if (_sel < n - 1) setState(() => _sel++);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowUp) {
      if (_sel > 0) setState(() => _sel--);
      return KeyEventResult.handled;
    }

    // Enter = 开始解压。依据是项目自己的 UI-SPEC §4b.6
    //   「Enter 确认」= 提交本屏主操作，也就是下面那个大按钮；
    // Ctrl+E 是别名，对齐 PeaZip 的惯例
    //   （refs/外部-归档软件/PeaZip/peazip-sources/res/share/lang/en.txt:1414
    //    原文 keyboard shortcuts: Ctrl+E (or F12) extracts the entire archive）。
    // 一个可解的都没有时什么都不做 —— 按钮那时也是禁用的。
    final isEnter = key == LogicalKeyboardKey.enter ||
        key == LogicalKeyboardKey.numpadEnter;
    final isCtrlE = key == LogicalKeyboardKey.keyE &&
        HardwareKeyboard.instance.isControlPressed;
    if (isEnter || isCtrlE) {
      final ready = widget.queue.where((q) => q.archive).length;
      if (ready > 0) widget.onStartAll();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final queue = widget.queue;
    final ready = queue.where((q) => q.archive).length;
    return Focus(
      focusNode: _focus,
      onKeyEvent: _onKey,
      child: Column(
        key: const ValueKey('queue'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text(t('home.foundArchives', {'n': '$ready'}),
                  style: Theme.of(context).textTheme.titleMedium),
              if (queue.length > ready)
                Padding(
                  padding: const EdgeInsets.only(left: 8),
                  child: Text(
                      t('home.skippedUnrelated',
                          {'n': '${queue.length - ready}'}),
                      style:
                          TextStyle(fontSize: 11, color: cs.onSurfaceVariant)),
                ),
              const Spacer(),
              TextButton(onPressed: widget.onReset, child: Text(t('home.clear'))),
            ],
          ),
          // D6：拖/选的是文件夹 → 明说会怎么处理，别让人猜
          if (widget.foldersScanned > 0)
            Padding(
              padding: const EdgeInsets.only(top: 4, bottom: 4),
              child: Text(
                t('home.dirHint'),
                style: TextStyle(fontSize: 11, height: 1.5, color: cs.primary),
              ),
            ),
          // P2 智能解压：规则在引擎里（Bandizip 三规则），但要让用户知道会发生什么
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(
              t('home.smartDirRule'),
              style: TextStyle(
                  fontSize: 11, height: 1.5, color: cs.onSurfaceVariant),
            ),
          ),
          const SizedBox(height: 0),
          Expanded(
            child: Card(
              clipBehavior: Clip.antiAlias,
              child: ListView.separated(
                itemCount: queue.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (context, i) {
                  final q = queue[i];
                  final selected = i == _sel;
                  return ListTile(
                    dense: true,
                    // 键盘导航的选中行：底色 + 左边一条竖线，用户能看出焦点在哪
                    selected: selected,
                    selectedTileColor: cs.primary.withValues(alpha: 0.12),
                    shape: selected
                        ? Border(
                            left: BorderSide(color: cs.primary, width: 3))
                        : null,
                    onTap: () {
                      setState(() => _sel = i);
                      _focus.requestFocus();
                    },
                    // 多候选那行会多出一句话，普通两行装不下
                    isThreeLine: q.hasManyCandidates,
                    leading: Icon(
                      q.archive
                          ? Icons.inventory_2_outlined
                          : Icons.help_outline,
                      size: 20,
                      color: q.archive ? cs.primary : cs.onSurfaceVariant,
                    ),
                    title: Row(
                      children: [
                        Flexible(
                          child: Text(q.name,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontSize: 13)),
                        ),
                        if (q.volumes > 1) ...[
                          const SizedBox(width: 6),
                          Container(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 6, vertical: 1),
                            decoration: BoxDecoration(
                              color: cs.secondaryContainer,
                              borderRadius: BorderRadius.circular(4),
                            ),
                            child: Text(
                                t('home.volumeBadge', {'n': '${q.volumes}'}),
                                style: TextStyle(
                                    fontSize: 10,
                                    color: cs.onSecondaryContainer)),
                          ),
                        ],
                      ],
                    ),
                    subtitle: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          q.archive
                              ? '${q.kind.toUpperCase()} · ${formatBytes(q.size)}'
                              : t('home.notArchive',
                                  {'size': formatBytes(q.size)}),
                          style: const TextStyle(fontSize: 11),
                        ),
                        // S10 多候选：一个文件里好几处都像藏着东西
                        //
                        // 不列「第1处/第2处」让用户自己挑 —— 新手面对
                        // 「从开头算 5.2 MB 处」根本无从判断。只给一句话 +
                        // 一个「换个地方试」，程序替他挨个试。
                        if (q.hasManyCandidates)
                          Row(
                            children: [
                              Icon(Icons.travel_explore,
                                  size: 13, color: cs.primary),
                              const SizedBox(width: 4),
                              Expanded(
                                child: Text(
                                  t('home.candidateCount',
                                      {'n': '${q.candidates.length}'}),
                                  style: TextStyle(
                                      fontSize: 11, color: cs.primary),
                                ),
                              ),
                              TextButton(
                                onPressed: () =>
                                    widget.onTryOtherCandidates(i),
                                style: TextButton.styleFrom(
                                  visualDensity: VisualDensity.compact,
                                  padding:
                                      const EdgeInsets.symmetric(horizontal: 6),
                                ),
                                child: Text(t('home.tryElsewhere'),
                                    style: const TextStyle(fontSize: 11)),
                              ),
                            ],
                          ),
                      ],
                    ),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        // D5：只取消这一行（其余任务照跑，和「停掉整批」不是一回事）
                        if (q.archive &&
                            (q.status == 'pending' || q.status == 'running'))
                          IconButton(
                            icon: const Icon(Icons.block, size: 16),
                            tooltip: q.status == 'running'
                                ? t('home.cancelItemRest')
                                : t('home.dontRunItem'),
                            visualDensity: VisualDensity.compact,
                            onPressed: () => widget.onCancelRow(i),
                          ),
                        _QueueStatusChip(item: q),
                        IconButton(
                          icon: const Icon(Icons.close, size: 16),
                          tooltip: t('home.removeFromQueue'),
                          visualDensity: VisualDensity.compact,
                          onPressed: () => widget.onRemove(i),
                        ),
                      ],
                    ),
                  );
                },
              ),
            ),
          ),
          const SizedBox(height: 12),
          FilledButton.icon(
            onPressed: ready == 0 ? null : widget.onStartAll,
            icon: const Icon(Icons.play_arrow),
            label: Text(ready == 0
                ? t('home.nothingToExtract')
                : t('home.startAll', {'n': '$ready'})),
            style: FilledButton.styleFrom(
                padding: const EdgeInsets.symmetric(vertical: 14)),
          ),
        ],
      ),
    );
  }
}

/// 一行右侧的状态小标（处理中 / 完成 / 部分 / 失败 / 已取消 / 跳过）
class _QueueStatusChip extends StatelessWidget {
  const _QueueStatusChip({required this.item});

  final QueueItem item;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final (text, color) = switch (item.status) {
      'running' => (t('home.st.processing'), cs.primary),
      'ok' => (t('home.st.done'), cs.primary),
      'partial' => (t('home.st.partial'), Colors.orange),
      'failed' => (t('home.st.failed'), cs.error),
      'cancelled' => (t('home.st.cancelled'), cs.onSurfaceVariant),
      _ => (t('home.st.waiting'), cs.onSurfaceVariant),
    };
    if (item.status == 'pending' && !item.archive) {
      return Text(t('home.st.skipped'),
          style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant));
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (item.status == 'running')
          const SizedBox(
            width: 12,
            height: 12,
            child: CircularProgressIndicator(strokeWidth: 1.5),
          )
        else
          Icon(
            switch (item.status) {
              'ok' => Icons.check_circle_outline,
              'partial' => Icons.warning_amber_outlined,
              'failed' => Icons.error_outline,
              'cancelled' => Icons.block,
              _ => Icons.schedule,
            },
            size: 14,
            color: color,
          ),
        const SizedBox(width: 4),
        Text(text, style: TextStyle(fontSize: 11, color: color)),
      ],
    );
  }
}
