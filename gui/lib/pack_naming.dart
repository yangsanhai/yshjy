// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 打包页的**命名与路径规则**（纯函数，可单测）
///
/// 为什么单独一个文件：这些函数原先散在 `pack_page.dart` 里当私有方法，
/// 于是「换输入文件后输出名不跟着变」这种 bug 一条测试都碰不到（要触发系统文件选择器）。
/// 2026-10-30 抽出来，`gui/test/pack_naming_test.dart` 直接测。
///
/// **跨平台铁律**：路径分隔符 Windows 是 `\`、Unix 是 `/`。
/// 写死 `'/'` 在 Windows 上会算错（选 `C:\a\b.zip` 会被当成没有目录），
/// 这个病项目里栽过（W9 三处写死 Unix 路径）。这里一律用 [sepRe]。
library;

import 'dart:convert';

/// 同时匹配 `/` 和 `\`
final RegExp sepRe = RegExp(r'[/\\]');

/// 取最后一段（文件名或最后一级目录名）
String baseName(String path) {
  final i = path.lastIndexOf(sepRe);
  return i >= 0 ? path.substring(i + 1) : path;
}

/// 取所在目录（**带尾部分隔符**，方便直接拼文件名；没有目录返回空串）
String dirOf(String path) {
  final i = path.lastIndexOf(sepRe);
  return i >= 0 ? path.substring(0, i + 1) : '';
}

/// 取父目录（**不带**尾部分隔符，给「打开所在文件夹」用）
///
/// 没有分隔符时原样返回（调用方自己兜底）。
String parentOf(String path) {
  final i = path.lastIndexOf(sepRe);
  return i > 0 ? path.substring(0, i) : path;
}

/// 从路径取小写扩展名（没有扩展名返回空串）
///
/// 注意：点必须出现在最后一段里才算扩展名，`/a.b/c` 不算 `c` 有扩展名。
String extOf(String path) {
  final base = baseName(path);
  final dot = base.lastIndexOf('.');
  return dot > 0 ? base.substring(dot + 1).toLowerCase() : '';
}

/// 把输出名的扩展名换成 [ext]，保留目录和主名
///
/// 如果原路径本来就没有扩展名，直接追加（`xxx` → `xxx.zip`）。
String withExt(String path, String ext) {
  final base = baseName(path);
  final dot = base.lastIndexOf('.');
  if (dot > 0) {
    final cut = path.length - (base.length - dot);
    return '${path.substring(0, cut)}.$ext';
  }
  return '$path.$ext';
}

/// 按「第一个输入」推一个建议输出路径：**同目录、同名、换扩展名**
///
/// 例：`/home/x/下载/电影.mp4` + ext=`zip` → `/home/x/下载/电影.zip`
/// 文件夹输入同理：`/home/x/下载/我的图` → `/home/x/下载/我的图.zip`（与文件夹并排）。
/// Windows：`C:\a\b.zip` + ext=`7z` → `C:\a\b.7z`（2026-10-30 前这里写死 `/`，算错）。
String suggestOut(String first, String ext) {
  return '${dirOf(first)}${withExt(baseName(first), ext)}';
}

/// 常见「外壳」扩展名 —— 与 Rust 侧 `pack::常见外壳扩展名` **必须逐字一致**
///
/// 界面要即时提示「这个名字能不能用」，不能等引擎回错；两边各有一条测试盯着。
/// 判据：只有这些后缀才算「用户写好的扩展名」，`1080p` / `2026` / `x265` 都不算。
const List<String> knownShellExts = [
  // 图片
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'tif', 'tiff', 'heic',
  // 视频
  'mp4', 'mkv', 'avi', 'mov', 'webm', 'flv', 'wmv', 'm4v', 'ts',
  // 音频
  'mp3', 'flac', 'wav', 'aac', 'ogg', 'm4a',
  // 文档
  'txt', 'md', 'log', 'ini', 'cfg', 'csv', 'pdf', 'doc', 'docx', 'xls', 'xlsx',
  'ppt', 'pptx', 'epub',
  // 归档
  'zip', 'rar', '7z', 'tar', 'gz', 'xz', 'zst',
  // 程序 / 镜像
  'exe', 'msi', 'apk', 'iso', 'dmg', 'deb', 'rpm', 'appimage',
];

/// 这个字符串算不算「外壳类型」（大小写不敏感，允许带前导点）
bool isShellExt(String ext) {
  final e = ext.replaceFirst(RegExp(r'^\.+'), '').toLowerCase();
  return knownShellExts.contains(e);
}

/// 追加后缀（**不替换**）：`电影.2026.1080p` → `电影.2026.1080p.mp4`
///
/// 用 [withExt] 会把它变成 `电影.2026.mp4` —— 把 `1080p` 当扩展名吃掉了
/// （T18-2 做显示名时实测踩到的形状）。
String ensureExt(String path, String ext) {
  return extOf(path) == ext.toLowerCase() ? path : '$path.$ext';
}

/// 「换后缀」的正确姿势：用户**没写**外壳扩展名时追加，写了才替换
///
/// - `电影.2026.1080p` + `mp4` → `电影.2026.1080p.mp4`（1080p 不是扩展名）
/// - `电影.png` + `mp4` → `电影.mp4`（切换外壳/格式时后缀必须跟着换，否则一认文件头就露馅）
String reExt(String path, String ext) {
  final cur = extOf(path);
  if (cur.isEmpty || !isShellExt(cur)) return '$path.$ext';
  return withExt(path, ext);
}

/// 按**文件头魔数**认外壳类型 —— 与 Rust `pack::外壳类型` 同一套规则，返回规范后缀或 null
///
/// 为什么不看文件名：用户把截图存成 `封面.dat` 很常见，而产物后缀必须跟**内容**一致
/// （PNG 内容叫 .mp4，看图软件一认魔数就判坏文件）。
String? shellExtFromHead(List<int> h) {
  bool starts(List<int> sig, [int at = 0]) {
    if (h.length < at + sig.length) return false;
    for (var i = 0; i < sig.length; i++) {
      if (h[at + i] != sig[i]) return false;
    }
    return true;
  }

  if (starts([0x89, 0x50, 0x4E, 0x47])) return 'png';
  if (starts([0xFF, 0xD8, 0xFF])) return 'jpg';
  if (starts([0x47, 0x49, 0x46, 0x38])) return 'gif'; // GIF8x
  if (h.length > 12 && starts([0x52, 0x49, 0x46, 0x46])) {
    // RIFF....
    final t = String.fromCharCodes(h.sublist(8, 12));
    if (t == 'WEBP') return 'webp';
    if (t == 'WAVE') return 'wav';
    if (t == 'AVI ') return 'avi';
    return null;
  }
  if (h.length > 8 && String.fromCharCodes(h.sublist(4, 8)) == 'ftyp') return 'mp4';
  if (starts([0x1A, 0x45, 0xDF, 0xA3])) return 'mkv';
  if (starts([0x25, 0x50, 0x44, 0x46])) return 'pdf'; // %PDF
  if (starts([0x50, 0x4B, 0x03, 0x04])) return 'zip';
  if (starts([0x52, 0x61, 0x72, 0x21])) return 'rar'; // Rar!
  if (starts([0x37, 0x7A, 0xBC, 0xAF])) return '7z';
  if (starts([0x4D, 0x5A])) return 'exe'; // MZ
  if (starts([0x49, 0x44, 0x33]) || starts([0xFF, 0xFB])) return 'mp3';
  if (starts([0x66, 0x4C, 0x61, 0x43])) return 'flac'; // fLaC
  if (starts([0x4F, 0x67, 0x67, 0x53])) return 'ogg'; // OggS
  // 认不出就返回 null（调用方退回文件名扩展名），别猜一个
  return null;
}

/// 显示名校验 —— 与 Rust 侧 `pack::校验显示名` **同一套规则**
///
/// 只返回错误码（文案走语言包）：`null` = 合法。
/// 规则都在「这条名字在 Windows 上会不会出问题」上：我们的包是发给别人的，
/// Linux 合法但 Windows 炸的名字（`:` `*` 设备名 末尾点）必须当场拦。
String? nameError(String name) {
  if (name.trim().isEmpty) return 'NAME_EMPTY';
  if (name.trim() != name) return 'NAME_SPACE';
  // `.` / `..` 必须排在「末尾不能是点」前面，否则报的是错的理由
  if (name == '.' || name == '..') return 'NAME_DOT';
  if (name.endsWith('.')) return 'NAME_SPACE';
  if (name.contains('/') || name.contains('\\')) return 'NAME_SEP';
  for (final r in name.runes) {
    if (r < 0x20 || r == 0x7f) return 'NAME_CTRL';
  }
  if (RegExp(r'[<>:"|?*]').hasMatch(name)) return 'NAME_WINCHAR';
  if (utf8.encode(name).length > 200) return 'NAME_LONG';
  final main = name.split('.').first.toUpperCase();
  if (const ['CON', 'PRN', 'AUX', 'NUL'].contains(main)) return 'NAME_RESERVED';
  if (main.length == 4 &&
      (main.startsWith('COM') || main.startsWith('LPT')) &&
      RegExp(r'^[1-9]$').hasMatch(main[3])) {
    return 'NAME_RESERVED';
  }
  return null;
}
