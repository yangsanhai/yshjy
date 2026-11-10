// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 主题：桌面壳与手机壳共用这一份
//!
//! 为什么单独抽出来：原来它写在 `main.dart` 的 `YshjyApp` 里，只能给桌面用。
//! 手机是另一套壳（`mobile/mobile_app.dart`），两套要长得一致 —— 主题就只能有一份。

import 'dart:io';

import 'package:flutter/material.dart';

/// 品牌种子色：青色系（#00BCD4），与主人喜欢的初音色一致
const Color kSeedColor = Color(0xFF00BCD4);

ThemeData buildYshjyTheme(Brightness brightness) {
  // Windows 上 Flutter 默认字体是 Segoe UI，中文全靠系统回退；实测回退会挑到
  // 「Microsoft YaHei Light」，11~12px 的小字笔画发细，看着发灰像半透明。
  // 显式把中文回退钉成常规字重的「Microsoft YaHei UI」，西文仍用 Segoe UI。
  // （Linux 不动：那边回退到 Noto Sans CJK；安卓走系统字体，也不用管。）
  final bool isWindows = Platform.isWindows;
  return ThemeData(
    useMaterial3: true,
    colorScheme: ColorScheme.fromSeed(seedColor: kSeedColor, brightness: brightness),
    visualDensity: VisualDensity.standard,
    fontFamily: isWindows ? 'Segoe UI' : null,
    fontFamilyFallback: isWindows ? const ['Microsoft YaHei UI', 'Microsoft YaHei'] : null,
  );
}
