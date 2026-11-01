// 文件挑选：对话框实现 + 「挑完之后干什么」的判断（批 7d）
//
// 为什么单独一层（2026-11-01）：
//   原来 `_pick()` 里三件事挤在一起 —— 弹系统对话框、判断「单选还是多选」、再决定走预览还是走队列。
//   后两件是**判断**，却只能靠人手点对话框才验得到（widget 测试弹不出系统对话框）。
//   抽出来之后：
//     · `pickOutcomeOf` 是纯函数，取消 / 单选 / 多选三种走法一条单测就能钉死；
//     · 对话框本身通过 `HomePage(pickFiles: ...)` 注入，测试可以塞一个「假装用户取消了」的替身。
//
// ⚠️ 不设扩展名白名单（老规矩，别改回去）：伪装包的外壳可能是 .pdf / .docx / .mp3 / .exe，
//    名单一写死用户在对话框里就看不到它 —— 而拖拽却能拖进来，同一个文件两种行为，最难解释。

import 'package:file_selector/file_selector.dart';
import 'package:flutter/services.dart';

import '../l10n.dart';

/// 挑完文件之后该走哪条路
enum PickOutcome {
  /// 用户取消了（或对话框返回空）
  none,

  /// 只挑了一个包 → 直接进预览页看清单
  preview,

  /// 挑了多个 → 进队列，批量解压
  queue,
}

/// 单选 → 预览；多选 → 队列；空 → 什么都不做。
///
/// 为什么不把单选也塞进队列：单个包进队列会让用户多点一次「开始」，
/// 而预览页本来就能看到里面有什么、还能勾选 —— 少一次点击。
PickOutcome pickOutcomeOf(List<String> paths) {
  if (paths.isEmpty) return PickOutcome.none;
  if (paths.length == 1) return PickOutcome.preview;
  return PickOutcome.queue;
}

/// 「挑一堆文件」的能力（页面构造参数注入用；测试塞替身）
typedef PickFilesFn = Future<List<String>> Function();

/// 「挑一个文件夹」的能力（同上）
typedef PickFolderFn = Future<String?> Function();

/// 「读剪贴板纯文本」的能力（注入口用；测试塞替身，不碰真实剪贴板）
///
/// 为什么要注入：`Clipboard.getData` 走平台通道，widget 测试里默认返回 null，
/// 想验「粘进来之后走预览还是走队列」就必须能塞一段假文本进来。
typedef ReadClipboardTextFn = Future<String?> Function();

/// 系统对话框的默认实现：返回所选文件的路径
Future<List<String>> pickFilesFromSystem() async {
  final group = XTypeGroup(label: t('home.archiveOrCover'));
  final files = await openFiles(acceptedTypeGroups: [group]);
  return files.map((f) => f.path).toList();
}

/// 系统对话框的默认实现：返回所选文件夹路径（取消 = null）
Future<String?> pickFolderFromSystem() => getDirectoryPath();

/// 剪贴板的默认实现：读纯文本（没有内容 → null）
///
/// 只读 `text/plain`：文件管理器「复制文件」在 Linux 上同时提供
/// `text/uri-list` 和 `x-special/gnome-copied-files`，但**也**给纯文本路径 ——
/// 三种都解析会造出三份真源，粘一次触发三次。纯文本是三家（GNOME/KDE/Windows）
/// 都有的那个最小公倍数。
Future<String?> readClipboardTextFromSystem() async {
  final data = await Clipboard.getData(Clipboard.kTextPlain);
  return data?.text;
}
