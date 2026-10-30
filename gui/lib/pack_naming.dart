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
