/// 字节数 → 人话（"1.23 GB" / "456.7 MB" / "789 KB" / "42 B"）
///
/// 抽出来是因为同一段实现原来在三个页面里各写了一份
/// （`home_page.dart` / `pack_page.dart` / `history_page.dart`）。
/// 这次只把首页那份接过来（行为逐字一致），另外两处暂不动 —— 免得一次改三处、
/// 出问题分不清是哪处复制的锅。
library;

String formatBytes(int bytes) {
  const k = 1024;
  if (bytes >= k * k * k) return '${(bytes / (k * k * k)).toStringAsFixed(2)} GB';
  if (bytes >= k * k) return '${(bytes / (k * k)).toStringAsFixed(1)} MB';
  if (bytes >= k) return '${(bytes / k).toStringAsFixed(0)} KB';
  return '$bytes B';
}
