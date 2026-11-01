/// 主页顶部那条 AppBar —— 从 `home_page.dart` 抽出来的第 9 块（A 段）。
///
/// 只抽 AppBar、**不抽 `_scaffold` 和 `_body`**：那两个是骨架 + 相位分派，
/// `_body` 一个 switch 就要调八个视图方法，抽出去只能变成「传八个 Widget 参数」的搬运机。
/// 判据看引用成员数，不看行数 —— 骨架本来就该待在 State 里。
library;

import 'package:flutter/material.dart';

import '../l10n.dart';
import 'home_hints.dart';

class HomeAppBar extends StatelessWidget implements PreferredSizeWidget {
  const HomeAppBar({
    super.key,
    required this.file,
    required this.canPick,
    required this.onPick,
  });

  /// 当前处理的文件（`null` = 还没选）
  final String? file;

  /// 解压中不许再选文件
  final bool canPick;

  final VoidCallback onPick;

  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    return AppBar(
      title: Row(
        children: [
          const Icon(Icons.inventory_2_outlined, size: 22),
          const SizedBox(width: 8),
          Text(t('home.brandName')),
          const SizedBox(width: 10),
          if (file case final f?)
            Flexible(
              child: Text(
                shortPath(f),
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
        ],
      ),
      actions: [
        IconButton(
          onPressed: canPick ? onPick : null,
          icon: const Icon(Icons.folder_open),
          tooltip: t('home.pickFile'),
        ),
      ],
    );
  }
}
