// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 预览页的面包屑路径条 —— 从 `home_page.dart` 抽出来的第 5a 块。
///
/// **为什么是面包屑不是下拉框**（调研 07:616-631 / :682）：
/// PeaZip 的面包屑是被用户夸的点，7-Zip 的下拉框是被吐槽的点 —— 层数一深，
/// 下拉框里根本看不出自己在哪一层。所以这里是「可点的路径链」。
///
/// 第一节显示**包名**而不是「全部」：一长串路径里，用户最先要认出「这是哪个包」。
///
/// 依赖只有三个（当前层 / 包文件路径 / 点某层去哪），零状态 —— 这块自己不改状态。
library;

import 'package:flutter/material.dart';

import '../entry_tree.dart';
import '../pack_naming.dart';

class CrumbBar extends StatelessWidget {
  const CrumbBar({
    super.key,
    required this.crumb,
    required this.filePath,
    required this.onCrumbTap,
  });

  /// 当前在包内的哪一层（null / 空串 = 根）
  final String? crumb;

  /// 包文件路径 —— 只用它的文件名（第一节文案）
  final String? filePath;

  /// 点了某一节 → 跳去那一层
  final void Function(String path) onCrumbTap;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final crumbs = breadcrumbs(crumb ?? '');
    // ★ 用 baseName 而不是 split('/').last：这是**本地文件路径**，Windows 下是反斜杠，
    //   只认 / 会把整条路径当成文件名显示出来。
    //   （包内条目路径是另一回事 —— ZIP 规范固定用 /，那边 split('/') 是对的，别一起改）
    final pkg = baseName(filePath ?? '');
    return Padding(
      padding: const EdgeInsets.only(top: 4, bottom: 2),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (var i = 0; i < crumbs.length; i++) ...[
              if (i > 0)
                Icon(Icons.chevron_right, size: 16, color: cs.onSurfaceVariant),
              // T11：自绘的可点区域（InkWell 包文字）在无障碍树里只是个「文本」，
              // 读屏软件不会说它能按。包一层 Semantics 才读成「合集A，按钮」。
              // excludeSemantics 去掉子 Text 自带的同名文本，免得念两遍。
              Semantics(
                button: true,
                label: i == 0
                    ? (pkg.isEmpty ? crumbs[i].name : pkg)
                    : crumbs[i].name,
                excludeSemantics: true,
                // ★ 语义层必须自己带 onTap：加了 excludeSemantics 之后子节点（InkWell）
                //   的 tap 动作被排掉了，只有 button 标记 —— 读屏会念「按钮」却点不动
                //   （2026-11-01 测试当场抓到：isButton ✔ 但 missing actions: [tap]）。
                //   两处 onTap 指的是同一个回调，不是两份真相。
                onTap: () => onCrumbTap(crumbs[i].path),
                child: InkWell(
                onTap: () => onCrumbTap(crumbs[i].path),
                child: Padding(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                  child: Text(
                    i == 0
                        ? (pkg.isEmpty ? crumbs[i].name : pkg)
                        : crumbs[i].name,
                    style: TextStyle(
                      fontSize: 12,
                      color: i == crumbs.length - 1
                          ? cs.onSurface
                          : cs.primary,
                      fontWeight: i == crumbs.length - 1
                          ? FontWeight.w600
                          : FontWeight.normal,
                    ),
                  ),
                ),
              ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
