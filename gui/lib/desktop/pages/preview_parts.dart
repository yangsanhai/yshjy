// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 预览页的三个小零件 —— 从 `home_page.dart` 抽出来的第 5d-1 步。
///
/// 为什么先抽这三个：`_previewView` 主体（325 行）要传 25 个参数才能搬，
/// 得先设计一个状态对象，属于架构活、单独一轮做。但这三个是**纯的**：
/// 一个渲染小按钮、两个纯函数，抽出来零风险，还能让主体的参数表短一点。
///
/// 顺带的好处：`pickedBytes` / `smartDirHint` 变成顶层纯函数之后**能单测了**
/// （原来埋在 State 里，只能靠整页 pump 间接验）。
library;

import 'package:flutter/material.dart';

import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/common/l10n.dart';

/// 紧凑小按钮。
///
/// 默认 `TextButton` 高 48，而预览页顶部那一行要塞四个（全选 / 全不选 / 树形切换 / 高级），
/// 不压缩的话直接吃掉近 200 像素，把下面的文件列表挤成 0 高 —— 用户看到的是「滚轮怎么滚都不动」。
class MiniButton extends StatelessWidget {
  const MiniButton(this.text, this.onTap, {super.key, this.icon});

  final String text;
  final VoidCallback onTap;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    const st = ButtonStyle(
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      visualDensity: VisualDensity.compact,
      minimumSize: WidgetStatePropertyAll(Size(0, 28)),
      padding: WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 8)),
      textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 12)),
    );
    final label = Text(text, style: const TextStyle(fontSize: 12));
    return icon == null
        ? TextButton(style: st, onPressed: onTap, child: label)
        : TextButton.icon(
            style: st,
            onPressed: onTap,
            icon: Icon(icon, size: 15),
            label: label,
          );
  }
}

/// 勾上的文件合计多少字节（状态栏「已选 N 项 · X MB」用）。
///
/// 目录不算 —— 目录自己不产出文件，算进去会把总量重复计一遍。
int pickedBytes(Preview pv, Set<int> picked) {
  var n = 0;
  for (final e in pv.entries) {
    // 审计 A-6：声明的解压尺寸不可信的条目不进合计 —— 那是包自己写的假数字
    if (!e.isDir && picked.contains(e.index) && !e.sizeSuspect) n += e.size;
  }
  return n;
}

/// 勾上的条目里有没有「声明尺寸不可信」的（审计 A-6）
///
/// 有的话状态栏就不写合计（会是个假 GB），改写「尺寸异常」。
bool pickedHasSuspect(Preview pv, Set<int> picked) => pv.entries.any(
    (e) => !e.isDir && picked.contains(e.index) && e.sizeSuspect);

/// 「点开始解压之前，先告诉他会发生什么」—— 对应引擎智能解压的三条规则。
///
/// 单文件 → 直接解；包里只有一个顶层目录 → 解到那个目录里；散文件 → 新建同名文件夹。
/// 这句话必须和引擎的真实行为一致，写错的代价比不写更大（用户照着他以为的去找文件）。
String smartDirHint(Preview pv) {
  final files = pv.entries.where((e) => !e.isDir).toList();
  if (files.isEmpty) return t('home.noFilesInArchive');
  if (files.length == 1) return t('home.willExtractOne');
  final roots = files.map((e) => e.path.split('/').first).toSet();
  final allNested = files.every((e) => e.path.contains('/'));
  if (roots.length == 1 && allNested) {
    return t('home.willExtractInto', {'root': roots.first});
  }
  return t('home.willCreateFolder', {'n': '${files.length}'});
}
