// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
/// 码 → 人话（D21 / N1）
///
/// 规矩：**引擎只给码，外壳查文案**。这个文件就是「外壳那一份表」。
/// 两条铁律：
/// 1. 查不到的码**不给用户看原始码**（D21：缺一条就不显示），只给一句通用人话；
///    原始码仍在日志/诊断包里，需要排查时拿得到。
/// 2. 码的名字必须和引擎 `src/error.rs` 的 `as_str()` 一致（大写下划线）。
///    **以前界面写的是 `PASSWORD_REQUIRED` / `PASSWORD_WRONG`，引擎发的是
///    `NEED_PASSWORD` / `WRONG_PASSWORD`** —— 文案表改对了，但 `home_page` 里
///    「要不要显示解压码输入框」那个开关没跟着改，于是**加密包在界面上是死路**：
///    只见一句「需要解压码」，永远见不到输入框（2026-10-30 用户在真机撞上）。
///    所以这类判断一律走下面两个纯函数，**并且有单测**（`gui/test/humanize_test.dart`），
///    别在各页面里手写码名比较。
///
/// T14 起文案搬进语言包（`assets/l10n/*.json` 的 `err.*` / `diag.*` / `item.*` / `batch.*`），
/// 界面切成英文时错误提示也跟着变。**查表的规矩没变**：查不到就用该组的 `*.unknown`
/// 那句通用人话，绝不把码名甩给用户。
library;

import 'l10n.dart';

/// 查一条文案；查不到就用兜底 key（**绝不把码名显示给用户**）
String _lookup(String key, String fallbackKey) {
  final text = t(key);
  return text == key ? t(fallbackKey) : text;
}

/// 错误码 → 人话（引擎 `ErrorCode::as_str()` + 外壳自己的几个码）
///
/// 输入也接受 Rust 枚举名（`WrongPassword`）—— 协议 golden 里带的就是那种写法，
/// 统一先过 [camelToCode] 再查表。
String errorText(String code) => _lookup('err.${camelToCode(code)}', 'err.unknown');

/// 归档级诊断码 → 人话（引擎 `DiagnosticCode::as_str()`）
String diagText(String code) => _lookup('diag.${camelToCode(code)}', 'diag.unknown');

/// 条目级结果 → 人话（引擎 `ItemResult::as_str()`）
String itemResultText(String code) => _lookup('item.${camelToCode(code)}', 'item.unknown');

/// 批量结果里的任务结论 → 人话
///
/// 协议里：`ok` / `partial` / `cancelled` 是小写固定串，失败时给的是错误码
/// （老引擎给 Rust 的 Debug 名 `WrongPassword`，新引擎给 `WRONG_PASSWORD`，两种都认）。
String batchOutcomeText(String oc) {
  switch (oc) {
    case 'ok':
      return t('batch.ok');
    case 'partial':
      return t('batch.partial');
    case 'cancelled':
      return t('batch.cancelled');
  }
  return errorText(camelToCode(oc));
}

/// `WrongPassword` → `WRONG_PASSWORD`（已经是下划线式 / 全大写就原样返回）
///
/// ⚠ 2026-10-30 修的第二个 bug：老实现只判「含 `_`」就原样返回，
/// 于是**单字大写的码被逐字符拆开** —— `TRUNCATED` → `T_R_U_N_C_A_T_E_D`、
/// `CANCELLED` → `C_A_N_C_E_L_L_E_D`，查文案必然落空，用户看到「原因不明」。
/// 凡是「全大写」的串一律当成已经规范，不再动它。
String camelToCode(String s) {
  if (s.isEmpty) return s;
  if (s.contains('_') || s == s.toUpperCase()) return s;
  final b = StringBuffer();
  for (var i = 0; i < s.length; i++) {
    final c = s[i];
    if (i > 0 && c.toUpperCase() == c && c.toLowerCase() != c) b.write('_');
    b.write(c.toUpperCase());
  }
  return b.toString();
}

/// 这个错误码是不是「缺解压码」——**界面要不要给解压码输入框，只认这个函数**
///
/// 三个真码（实测 `yshjy --serve` 返回的就是这三个，驼峰也认）：
/// * `NEED_PASSWORD` —— 条目加密、没给码（解压阶段）
/// * `WRONG_PASSWORD` —— 给了码但不对
/// * `HEAD_ENCRYPTED` —— 连文件清单都加密（7z `-mhe=on`），预览阶段就会失败
///
/// 历史 bug：这里曾写成 `PASSWORD_REQUIRED` / `PASSWORD_WRONG`，**引擎从来不发
/// 这两个码**，条件恒假 → 加密包在界面上是死路。改码名时务必同时改
/// `gui/test/humanize_test.dart` 的断言。
bool codeNeedsPassword(String? code) {
  final c = camelToCode(code ?? '');
  return c == 'NEED_PASSWORD' || c == 'WRONG_PASSWORD' || c == 'HEAD_ENCRYPTED';
}

/// 这个失败还值不值得给「试着救出来」按钮
///
/// 只认引擎真会发的码（`src/error.rs` 的 `ErrorCode`）：
/// * `DATA_LOST` —— 中间有一段数据丢了，救援能捞回前后
/// * `TRUNCATED` —— 文件没下完，救援能捞回已下的部分
/// * `NOT_ARCHIVE` —— 只有当内容其实是 ZIP、只是索引坏了才值得试
///
/// 历史 bug：这里曾用 `PARTIAL_DATA`（那是**诊断码**，不是错误码，永远走不到
/// 这条分支）和 `CHECKSUM_FAILED`（**引擎里根本不存在这个码**），
/// 结果「坏了但能救」的包反而不给救援按钮。
bool codeCanRescue(String? code, {String? looksLike}) {
  final c = camelToCode(code ?? '');
  if (c.isEmpty || codeNeedsPassword(c) || c == 'MISSING_VOLUME') return false;
  if (c == 'NOT_ARCHIVE') return (looksLike ?? '').toLowerCase() == 'zip';
  return const {'DATA_LOST', 'TRUNCATED'}.contains(c);
}
