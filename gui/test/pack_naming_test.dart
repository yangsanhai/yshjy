// 打包页命名/路径规则的单元测试（2026-10-30）
//
// 这些逻辑原先躲在 pack_page.dart 里当私有方法，于是：
// ① 「换输入文件后输出名不跟着变」这个 bug 一条测试都碰不到（要触发系统文件选择器）；
// ② 更狠的是它**写死了 `/`**：Windows 上选 `C:\a\b.zip` 会被当成「没有目录」，
//    建议输出名直接算错（跟 W9 修的「界面三处写死 Unix 路径」同一个病）。
// 抽成纯函数之后，这两类问题都能用断言钉死。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/pack_naming.dart';

void main() {
  group('suggestOut：默认跟源同目录、同名、换扩展名', () {
    test('Linux 文件：电影.mp4 → 同目录 电影.zip', () {
      expect(suggestOut('/home/x/下载/电影.mp4', 'zip'), '/home/x/下载/电影.zip');
    });

    test('Linux 文件夹：与文件夹**并排**，不塞进文件夹里', () {
      expect(suggestOut('/home/x/下载/我的图', 'zip'), '/home/x/下载/我的图.zip');
    });

    test('没有任何扩展名的文件', () {
      expect(suggestOut('/home/x/数据', 'zip'), '/home/x/数据.zip');
    });

    test('名义上带点的目录不会被当成扩展名', () {
      // /a.b/c 里 c 没有扩展名 → 输出 c.zip，且目录 a.b 原样保留
      expect(suggestOut('/home/a.b/c', 'zip'), '/home/a.b/c.zip');
    });

    test('🎯 Windows 反斜杠：C:\\a\\b.zip → C:\\a\\b.7z', () {
      expect(suggestOut(r'C:\Users\x\b.zip', '7z'), r'C:\Users\x\b.7z');
    });

    test('🎯 Windows 无扩展名的文件夹', () {
      expect(suggestOut(r'D:\素材\我的图', 'zip'), r'D:\素材\我的图.zip');
    });

    test('纯文件名（没有目录）只返回文件名', () {
      expect(suggestOut('b.zip', '7z'), 'b.7z');
    });
  });

  group('withExt：只换扩展名，别啃掉路径', () {
    test('普通情况', () {
      expect(withExt('/a/b.mp4', 'zip'), '/a/b.zip');
    });
    test('目录里有别的点', () {
      expect(withExt('/a.b/c.mp4', 'zip'), '/a.b/c.zip');
    });
    test('本来没扩展名就追加', () {
      expect(withExt('/a/b', 'zip'), '/a/b.zip');
    });
    test('Windows 路径', () {
      expect(withExt(r'C:\a.b\c.mp4', 'zip'), r'C:\a.b\c.zip');
    });
  });

  group('extOf', () {
    test('取小写扩展名', () {
      expect(extOf('/a/b.MP4'), 'mp4');
    });
    test('目录里带点不算', () {
      expect(extOf('/a.b/c'), '');
    });
    test('点开头的隐藏文件不算扩展名', () {
      expect(extOf('/a/.bashrc'), '');
    });
    test('Windows', () {
      expect(extOf(r'C:\a\b.RAR'), 'rar');
    });
  });

  group('parentOf：给「打开所在文件夹」用', () {
    test('Linux', () {
      expect(parentOf('/home/x/下载/电影.zip'), '/home/x/下载');
    });
    test('Windows', () {
      expect(parentOf(r'C:\Users\x\电影.zip'), r'C:\Users\x');
    });
    test('没有目录时原样返回', () {
      expect(parentOf('电影.zip'), '电影.zip');
    });
  });

  group('baseName / dirOf', () {
    test('Linux', () {
      expect(baseName('/a/b/c.txt'), 'c.txt');
      expect(dirOf('/a/b/c.txt'), '/a/b/');
    });
    test('Windows', () {
      expect(baseName(r'C:\a\c.txt'), 'c.txt');
      expect(dirOf(r'C:\a\c.txt'), r'C:\a\');
    });
  });
}
