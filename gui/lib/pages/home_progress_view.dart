/// 解压进行中那一屏的三段 —— 从 `home_page.dart` 抽出来的第 6 块 B 段。
///
/// 原来 `_progressView` 是一个 115 行的方法、直接读 14 个 State 字段。按段拆成三块之后
/// 每块只吃 4–7 个参数，父层剩下的就是组装（顺序 + 间距）。
///
/// 三段的分工：
/// - `ExtractProgressBar`：标题下面的进度条 + 「当前文件 / 已解多少 · 速度 · 剩余」
/// - `ExtractStatusLines`：四种状态提示（密码尝试中 / 扫描中 / 暂停中 / 第几步）—— 都不占位当没有
/// - `ExtractActionRow`：底部按钮（全部暂停 / 跳过这个 / 取消），批量才给前两个
library;

import 'package:flutter/material.dart';

import '../engine.dart';
import '../format.dart';
import '../l10n.dart';

/// 「1.2 MB/s · 剩 3 分 12 秒」那段尾巴。
///
/// **暂停时不报速度**：用户真机验收抓到暂停画面上还挂着「1.04 GB/s · 剩 0 秒」——
/// 停都停了还显示速度，看着像没停住。恢复后下一帧进度事件会把速度算回来。
String etaText(Progress p, {required bool paused, required double speed}) {
  if (paused) return '';
  if (speed <= 0 || p.totalBytes <= 0) return '';
  final left = p.totalBytes - p.doneBytes;
  if (left <= 0) return '';
  final sec = (left / speed).round();
  final eta = sec >= 60
      ? t('home.durMinSec', {'min': '${sec ~/ 60}', 'sec': '${sec % 60}'})
      : t('home.durSec', {'sec': '$sec'});
  return t('home.speedEta', {'speed': formatBytes(speed.round()), 'eta': eta});
}

/// 进度条 + 当前文件 + 已解/总量/速度/ETA
class ExtractProgressBar extends StatelessWidget {
  const ExtractProgressBar({
    super.key,
    required this.progress,
    required this.scanDone,
    required this.scanTotal,
    required this.current,
    required this.speed,
    required this.paused,
  });

  /// 解压进度（扫描阶段没有，用下面两个扫描数字画条）
  final Progress? progress;

  final int scanDone;
  final int scanTotal;

  /// 当前正在处理的文件名（可能为空 → 显示「准备中」）
  final String? current;

  /// 字节/秒
  final double speed;

  final bool paused;

  @override
  Widget build(BuildContext context) {
    final p = progress;
    // 扫描阶段没有 progress：用 SCAN_PROGRESS 的两个数字画进度条（D1）
    final frac = p?.fraction ??
        (scanTotal > 0 ? (scanDone / scanTotal).clamp(0.0, 1.0) : null);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(8),
          child: LinearProgressIndicator(
            value: frac, // null = 不确定进度（未知总大小时）
            minHeight: 10,
            // T11：读屏软件念进度条时只说「进度指示器」，得有人告诉它
            // 「在解压」和「到哪了」。value 为 null（算不出比例）时不报假百分比。
            semanticsLabel: t('home.extracting'),
            semanticsValue: frac == null ? null : '${(frac * 100).floor()}%',
          ),
        ),
        const SizedBox(height: 12),
        Row(
          children: [
            Expanded(
              child: Text(
                current ?? t('home.preparing'),
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 12),
              ),
            ),
            Text(
              p == null
                  ? ''
                  : '${formatBytes(p.doneBytes)} / '
                      '${p.totalBytes == 0 ? "?" : formatBytes(p.totalBytes)}'
                      '${etaText(p, paused: paused, speed: speed)}',
              style: const TextStyle(fontSize: 12),
            ),
          ],
        ),
      ],
    );
  }
}

/// 四种状态提示 + 步骤说明。都是「有才显示」，一条都没有时只剩步骤那行。
class ExtractStatusLines extends StatelessWidget {
  const ExtractStatusLines({
    super.key,
    required this.stage,
    required this.scanDone,
    required this.scanTotal,
    required this.pwTry,
    required this.pwTotal,
    required this.pauseRequested,
    required this.paused,
  });

  final String stage;
  final int scanDone;
  final int scanTotal;
  final int pwTry;
  final int pwTotal;
  final bool pauseRequested;
  final bool paused;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // 正在试第几个密码（加密包里多个候选时）
        if (pwTry > 0 && pwTotal > 0)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(
              t('home.pwTrying', {'cur': '$pwTry', 'total': '$pwTotal'}),
              style: TextStyle(fontSize: 12, color: cs.primary),
            ),
          ),
        // 扫描阶段（还没开始真解压）
        if (stage == 'LOCATE' && scanTotal > 0)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(
              t('home.scanning',
                  {'done': formatBytes(scanDone), 'total': formatBytes(scanTotal)}),
              style: TextStyle(fontSize: 12, color: cs.primary),
            ),
          ),
        // T18-4：暂停中 / 已暂停，都在这一行说清楚「现在是什么状态」
        // （按钮点下去到真停之间有间隔，不说清楚就像卡住了）
        if (pauseRequested || paused)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(
              paused ? t('home.pausedNote') : t('home.pausing'),
              style: TextStyle(fontSize: 12, color: cs.primary),
            ),
          ),
        Text(
          stage == 'LOCATE'
              ? t('home.step1')
              : stage == 'LIST'
                  ? t('home.step2')
                  : t('home.step3'),
          style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
        ),
      ],
    );
  }
}

/// 底部按钮行。
///
/// **批量才给「全部暂停」和「跳过这个」**：单个文件暂停没意义（它本来就快跑完了，
/// 暂停只会让用户以为卡住）；跳过单个和取消整批是一回事，多一个按钮反而更乱。
class ExtractActionRow extends StatelessWidget {
  const ExtractActionRow({
    super.key,
    required this.batchSize,
    required this.paused,
    required this.pauseRequested,
    required this.onPauseOrResume,
    required this.onSkipCurrent,
    required this.onCancel,
  });

  /// 队列里有多少个任务（`> 1` 才显示前两个按钮）
  final int batchSize;

  final bool paused;
  final bool pauseRequested;

  final VoidCallback onPauseOrResume;
  final VoidCallback onSkipCurrent;
  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final batch = batchSize > 1;
    return Row(
      mainAxisAlignment: MainAxisAlignment.end,
      children: [
        if (batch) ...[
          OutlinedButton.icon(
            // 已经点了暂停、还没真停 → 按钮暂时禁用，避免连点
            onPressed: pauseRequested ? null : onPauseOrResume,
            icon: Icon(paused
                ? Icons.play_arrow_outlined
                : Icons.pause_circle_outline),
            label: Text(paused ? t('home.resumeQueue') : t('home.pauseQueue')),
          ),
          const SizedBox(width: 8),
        ],
        if (batch) ...[
          OutlinedButton.icon(
            onPressed: onSkipCurrent,
            icon: const Icon(Icons.skip_next_outlined),
            label: Text(t('home.skipThisItem')),
          ),
          const SizedBox(width: 8),
        ],
        OutlinedButton.icon(
          onPressed: onCancel,
          icon: const Icon(Icons.stop_circle_outlined),
          label: Text(batch ? t('home.cancelBatch') : t('home.cancel')),
        ),
      ],
    );
  }
}
