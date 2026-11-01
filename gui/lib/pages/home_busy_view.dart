/// 处理中（扫描 / 起引擎 / 收尾）的等待页 —— 从 `home_page.dart` 抽出来的第 4 块。
///
/// 零状态依赖：只有一个 `text` 参数，所以它能独立存在、独立测。
///
/// ⚠️ 里面的 `CircularProgressIndicator` 是**无限动画**：任何 `pumpAndSettle` 撞上它都会
/// 一直等到超时（报 "An animation is still running..."）。这一屏本来就只出现几百毫秒
/// （扫描文件头 / 起引擎），对用户没问题；但**写测试要用定次 `pump`，不要 `pumpAndSettle`**。
library;

import 'package:flutter/material.dart';

class BusyView extends StatelessWidget {
  const BusyView({super.key, required this.text});

  /// 正在干什么（「正在扫描…」这类）
  final String text;

  @override
  Widget build(BuildContext context) {
    return Center(
      key: const ValueKey('busy'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const CircularProgressIndicator(),
          const SizedBox(height: 20),
          Text(text),
        ],
      ),
    );
  }
}
