// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 主页的四个纯函数 —— 从 `home_page.dart` 抽出来的第 10 块。
///
/// 这些原来都是 `State` 的私有方法，看似「就几行」，实际全都**读着 State 字段**
/// （`_preview` / `_error`），于是只能靠 pump 整个页面 + 造引擎状态才验得到。
/// 改成带参数的纯函数之后，一个 `test()` 就能把边界钉死。
///
/// ★ `shortPath` 顺带修了一个同类问题：原来只按 `/` 切分，Windows 的 `C:\a\b.zip`
/// 一个都切不开、原样显示（AppBar 只能靠省略号硬截）。现在 `[/\\]` 都认 ——
/// 和之前面包屑那个「只认 `/`」的 bug 是同一类。
library;

import '../engine.dart';
import '../format.dart';
import '../l10n.dart';

/// 格式码 → 人话（`mp4` → 「MP4 视频」这种）
String kindName(String k) => switch (k) {
      'mp4' => t('home.fmt.mp4'),
      'mov' => t('home.fmt.mov'),
      'mkv' => t('home.fmt.mkv'),
      'webm' => t('home.fmt.webm'),
      'avi' => t('home.fmt.avi'),
      'flv' => t('home.fmt.flv'),
      'ts' => t('home.fmt.ts'),
      'png' => t('home.fmt.png'),
      'jpeg' => t('home.fmt.jpg'),
      'gif' => t('home.fmt.gif'),
      'webp' => t('home.fmt.webp'),
      'bmp' => t('home.fmt.bmp'),
      'tiff' => t('home.fmt.tiff'),
      'mp3' => t('home.fmt.mp3'),
      'flac' => t('home.fmt.flac'),
      'ogg' => t('home.fmt.ogg'),
      'wav' => t('home.fmt.wav'),
      'pdf' => t('home.fmt.pdf'),
      'elf' => t('home.fmt.elf'),
      'exe' => t('home.fmt.pe'),
      'zip' => t('home.fmt.zip'),
      '7z' => t('home.fmt.7z'),
      'rar' => t('home.fmt.rar'),
      'gzip' => t('home.fmt.gz'),
      'xz' => t('home.fmt.xz'),
      'zstd' => t('home.fmt.zst'),
      'bzip2' => t('home.fmt.bz2'),
      'lz4' => t('home.fmt.lz4'),
      'text' => t('home.fmt.text'),
      _ => t('home.fmt.unknown'),
    };

/// 失败时那句「这个文件其实是……」。
///
/// `warn: true` 的场合只有一个：文件其实是**程序**（exe / ELF）—— 那是要提醒用户当心，
/// 不是「好消息」。其余都是中性信息，染成红色只会让人以为出了大事。
({String text, bool warn})? typeHint(EngineError? e) {
  if (e == null || e.detail.isEmpty) return null;
  final size = e.size;
  if (size != null && size == 0) {
    return (text: t('home.emptyFile'), warn: false);
  }
  final kind = kindName(e.looksLike ?? 'unknown');
  final sizeText = size == null ? '' : '，${formatBytes(size)}';
  if (e.extMismatch) {
    final extName = '.${e.ext ?? ''}';
    if (e.looksKind == 'archive') {
      return (
        text: t('home.guiseLikely', {'ext': extName, 'kind': kind, 'size': sizeText}),
        warn: false
      );
    }
    if (e.looksKind == 'program') {
      return (
        text: t('home.guiseProgram', {'ext': extName, 'kind': kind, 'size': sizeText}),
        warn: true
      );
    }
    return (
      text: t('home.guiseRenamed', {'ext': extName, 'kind': kind, 'size': sizeText}),
      warn: false
    );
  }
  if (e.looksKind == 'program') {
    return (text: t('home.guiseReal', {'kind': kind, 'size': sizeText}), warn: true);
  }
  if ((e.looksLike ?? '') == 'unknown') {
    return (text: t('home.unknownFile', {'size': sizeText}), warn: false);
  }
  return (text: t('home.fileLooksLike', {'kind': kind, 'size': sizeText}), warn: false);
}

/// 「解完该点哪个」—— 在产物里找一个像启动文件的东西（setup / install / 启动游戏 / .bat …）
String? entryHint(Preview? pv) {
  if (pv == null) return null;
  bool looksLikeEntry(String n) {
    final l = n.toLowerCase();
    return l.contains('setup') ||
        l.contains('install') ||
        l.contains(t('home.launch')) ||
        l.contains(t('home.startGame')) ||
        l.endsWith('.bat') ||
        l.endsWith('run.sh') ||
        l.endsWith('.appimage') ||
        l.endsWith('game.exe');
  }

  final hits = pv.entries.where((e) => !e.isDir && looksLikeEntry(e.path)).toList();
  return hits.isEmpty ? null : hits.first.path;
}

/// 路径压短：只留最后两级（`…/文件夹/文件.zip`）。
///
/// `[/\\]` 都认 —— Windows 路径一个 `/` 都没有，只按 `/` 切的话整条原样返回。
String shortPath(String p) {
  final parts = p.split(RegExp(r'[/\\]')).where((s) => s.isNotEmpty).toList();
  if (parts.length <= 2) return p;
  return '…/${parts.sublist(parts.length - 2).join('/')}';
}
