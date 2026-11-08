// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
// 预览页的初始界面状态 —— 引擎的 `list` 结果 → 界面字段（批 7c）
//
// 为什么抽出来（2026-11-01）：
//   这套「拿到清单之后界面该长什么样」的初始化，原来在 `home_page.dart` 里写了**两遍**：
//   一遍在 `_open()`（真机路径），一遍在 `initState` 的 `debugPreview` 测试注入口
//   （那段代码原来的注释自己写着「两处要一起改 —— 否则测试验的是另一套初始状态」）。
//   两处手工同步 = 迟早漏一处，而漏了的表现是「测试绿、真机上默认勾选不一样」。
//   现在两处都调这一个纯函数，改一次两边生效。
//
// 五条默认值各自的理由（都是踩出来的，不是随口定的）：
//   ① `wantCover = pv.hasCover` —— 有封面区才默认勾「伪装封面」；普通 zip 连那一行都不显示，
//      勾着一个不存在的东西会让引擎多找一圈封面文件。
//   ② `wantExtract = true` —— 换一个包必须回到「解内容」（用户上一包取消了勾选，
//      不代表这一包也不解）。
//   ③ `picked` = 所有**文件**的序号 —— 「默认全选、取消勾选才是只解一部分」；
//      目录不进去：只有文件能进协议的 `select`（引擎 `src/select.rs` 的序号锚点）。
//   ④ `tree` / `expanded` —— 预览树建一次、默认全展开；用户下钻前先看到全貌。
//   ⑤ 换包必须清掉上一个包的解压码/输出目录/编码 —— 那三样不属于这里（是 `_open` 的字面赋值），
//      但理由记在这：旧解压码静默套到新包上，表现成「明明没输密码却报解压码不对」（2026-10-30 修过）。

import '../engine.dart';
import '../entry_tree.dart';

/// 预览页初始化后的五个界面字段
class PreviewUiState {
  /// 是否勾「伪装封面」
  final bool wantCover;

  /// 是否勾「解压内容」（换包一律回到 true）
  final bool wantExtract;

  /// 默认勾选的条目序号（只含文件）
  final Set<int> picked;

  /// 预览树
  final List<EntryNode> tree;

  /// 默认展开的目录路径（全展开）
  final Set<String> expanded;

  const PreviewUiState({
    required this.wantCover,
    required this.wantExtract,
    required this.picked,
    required this.tree,
    required this.expanded,
  });
}

/// 拿到一份清单 → 预览页该有的初始状态。
///
/// 纯函数：不读全局、不碰引擎、不改入参 —— 所以 `flutter test` 直接喂一个 `Preview` 就能测，
/// 不用起界面、不用假引擎。
PreviewUiState previewUiState(Preview pv) {
  final tree = buildEntryTree(pv.entries);
  return PreviewUiState(
    // 有封面区才默认勾（普通 zip 根本没有那一行）
    wantCover: pv.hasCover,
    wantExtract: true,
    // 默认全选，但**只勾文件**：目录在预览树里是聚合节点，进 engines 的 select 没有意义
    picked: {
      for (final e in pv.entries)
        if (!e.isDir) e.index,
    },
    tree: tree,
    // 默认全展开：新手上来先看全貌，再决定下钻到哪一层
    expanded: allDirPaths(tree).toSet(),
  );
}
