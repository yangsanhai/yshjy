/// 预览页的解压码输入区 —— 从 `home_page.dart` 抽出来的第 5d-2 第 1 段。
///
/// **为什么在预览阶段就问密码**（2026-10-30 用户真机反馈的原话）：
/// 以前加密包非得点「开始解压」、失败一次，才在错误页见到输入框；
/// 可引擎在预览里早就给了 `encryptedCount` + `enc_kind`，没理由不提前问。
///
/// 三种输入方式都给：手输 / 眼睛看明文 / 点密码本里的常用码。
/// 密码本只读不回写，来源是 `$XDG_CONFIG_HOME/yshjy/passwords.txt`。
///
/// **`encryptedCount == 0` 时整块返回 `SizedBox.shrink()`** —— 把「什么时候显示」也收进来，
/// 父层就不用再写一层 `if`，少一处判断少一处能写反的地方。
library;

import 'package:flutter/material.dart';

import '../l10n.dart';

class PreviewPasswordBox extends StatelessWidget {
  const PreviewPasswordBox({
    super.key,
    required this.encryptedCount,
    required this.controller,
    required this.showPlain,
    required this.recent,
    required this.onToggleShowPlain,
    required this.onChanged,
    required this.onPickRecent,
  });

  /// 有几个条目加密（`<= 0` 时整块不显示）
  final int encryptedCount;

  /// 密码输入框控制器 —— **owner 在父层**（换文件时父层要清空它，别在组件里自己建）
  final TextEditingController controller;

  /// 现在是不是明文显示
  final bool showPlain;

  /// 密码本里的常用码（可能为空 → 不显示 chips）
  final List<String> recent;

  final VoidCallback onToggleShowPlain;

  /// 输入变化（已 trim；`null` 表示空）
  final void Function(String? pw) onChanged;

  /// 点了某个常用码
  final void Function(String pw) onPickRecent;

  @override
  Widget build(BuildContext context) {
    if (encryptedCount <= 0) return const SizedBox.shrink();
    return Column(
      // stretch：TextField 要撑满宽度（原来它直接挂在父 Column 的 children 里）
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          controller: controller,
          obscureText: !showPlain,
          onChanged: (v) {
            final txt = v.trim();
            // 不用 setState：这个字段不影响本帧画面
            onChanged(txt.isEmpty ? null : txt);
          },
          decoration: InputDecoration(
            labelText: t('home.pwForEncrypted', {'n': '$encryptedCount'}),
            hintText: t('home.pwEmptyHint'),
            isDense: true,
            border: const OutlineInputBorder(),
            prefixIcon: const Icon(Icons.key, size: 18),
            suffixIcon: IconButton(
              icon: Icon(
                showPlain ? Icons.visibility_off : Icons.visibility,
                size: 18,
              ),
              tooltip: showPlain ? t('home.pwHide') : t('home.pwShow'),
              onPressed: onToggleShowPlain,
            ),
          ),
        ),
        if (recent.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Wrap(
              spacing: 6,
              runSpacing: 6,
              children: [
                for (final pw in recent)
                  ActionChip(
                    avatar: const Icon(Icons.history, size: 14),
                    label: Text(pw, style: const TextStyle(fontSize: 12)),
                    visualDensity: VisualDensity.compact,
                    onPressed: () => onPickRecent(pw),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}
