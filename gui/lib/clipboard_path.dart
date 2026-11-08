// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 剪贴板文本 → 路径列表（UI-SPEC §4b.6 的 `Ctrl+V 粘贴路径`）
///
/// 为什么要单独一层纯函数：这一段的输入是「别人放进剪贴板的东西」，
/// 形状完全不受我们控制。真机实测过的几种：
///   · 文件管理器复制一个文件 → 一行绝对路径
///   · 复制多个文件 → 一行一个（Nautilus / Dolphin / Windows 资源管理器都是这个形状）
///   · 从浏览器、某些编辑器、终端里复制 → 带 `file://` 前缀，空格变成 `%20`
///   · 从日志、窗口标题里复制 → 前后带引号、带行尾空白
///
/// 规则只有「去壳」，一条都不猜：
///   · 按行切，空行丢掉
///   · 每行去首尾空白
///   · 成对的引号去掉（`"…"` / `'…'` / `“…”` / `「…」`）—— **只去成对的**，
///     路径里头本来就有引号时不动它
///   · `file://` 开头转成本地路径（解 `%20` 这类转义）；转不了就按原样当路径用
///   · 保序去重（同一行粘两遍不该变成两个任务）
///
/// **不判断文件存不存在**：那要碰磁盘（`dart:io`），纯函数才好白盒测。
/// 存在性由调用方筛（`home_page.dart` 的 `_pastePath`）。
library;

/// 成对引号：左 → 右
const Map<String, String> _quotePairs = {
  '"': '"',
  "'": "'",
  '“': '”',
  '「': '」',
};

/// 把剪贴板文本解析成路径列表（空 / 没有可用行 → 空列表）
List<String> pathsFromClipboardText(String? text) {
  if (text == null) return const [];
  final out = <String>[];
  final seen = <String>{};
  for (final rawLine in text.split(RegExp(r'\r\n|\r|\n'))) {
    final stripped = _unquote(rawLine.trim());
    if (stripped.isEmpty) continue;
    final path = _fromFileUri(stripped);
    if (path.isEmpty) continue;
    if (seen.add(path)) out.add(path);
  }
  return out;
}

/// 去掉**成对**的首尾引号；不成对就原样返回
String _unquote(String s) {
  if (s.length < 2) return s;
  final close = _quotePairs[s[0]];
  if (close == null || s[s.length - 1] != close) return s;
  return s.substring(1, s.length - 1).trim();
}

/// `file:///home/…` → `/home/…`；不是 file URI 或转不了 → 原样返回
///
/// 为什么只认 `file://` 前缀：Windows 盘符路径 `C:\a\b.zip` 用 [Uri.tryParse]
/// 解析会得到一个 scheme 是 `c` 的怪异 URI —— 按 scheme 判会把正常路径弄坏。
String _fromFileUri(String s) {
  if (!s.toLowerCase().startsWith('file://')) return s;
  final uri = Uri.tryParse(s);
  if (uri == null || uri.scheme != 'file') return s;
  try {
    return uri.toFilePath();
  } catch (_) {
    // file://主机名/路径 这种带 host 的写法本地转不了，按原样交回去
    return s;
  }
}
