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

  // ── T18-2 自定义显示名 ──────────────────────────────────────────────
  group('isShellExt：只有常见外壳后缀才算「写好的扩展名」', () {
    test('常见类型认识（大小写、带点都行）', () {
      for (final e in ['mp4', 'MP4', '.png', 'pdf', 'zip', 'exe', 'mkv']) {
        expect(isShellExt(e), isTrue, reason: '$e 该认成外壳类型');
      }
    });
    test('名字里的年份/清晰度不是扩展名', () {
      for (final e in ['1080p', '2026', 'x265', 'part1', '']) {
        expect(isShellExt(e), isFalse, reason: '$e 不该认成外壳类型');
      }
    });
  });

  group('ensureExt：只追加、不替换', () {
    test('电影.2026.1080p + mp4 → 补成 .mp4（不吃掉 1080p）', () {
      expect(ensureExt('/tmp/电影.2026.1080p', 'mp4'), '/tmp/电影.2026.1080p.mp4');
    });
    test('后缀已经对了就原样', () {
      expect(ensureExt('/tmp/电影.MP4', 'mp4'), '/tmp/电影.MP4',
          reason: '大小写不同不算「没写扩展名」，别加出 .mp4.mp4');
    });
  });

  group('reExt：没写外壳后缀就追加，写了才替换', () {
    test('多点的名字不被吃掉', () {
      expect(reExt('/tmp/电影.2026.1080p', 'mp4'), '/tmp/电影.2026.1080p.mp4');
    });
    test('切外壳时后缀必须跟着换（否则一认文件头就露馅）', () {
      expect(reExt('/tmp/电影.png', 'mp4'), '/tmp/电影.mp4');
    });
    test('tar.gz 这种多点后缀也要换对', () {
      expect(reExt('/tmp/备份.zip', 'tar.gz'), '/tmp/备份.tar.gz');
    });
  });

  group('nameError：与引擎 pack::校验显示名 同一套规则', () {
    test('正常名字放行', () {
      for (final n in [
        '电影.2026.1080p.mp4',
        '我的游戏.zip',
        '带 空格 的名字.mp4',
        '日本語のファイル.mp4',
      ]) {
        expect(nameError(n), isNull, reason: '$n 该放行');
      }
    });
    test('空 / 首尾空格 / 末尾点', () {
      expect(nameError(''), 'NAME_EMPTY');
      expect(nameError('   '), 'NAME_EMPTY');
      expect(nameError(' a.mp4'), 'NAME_SPACE');
      expect(nameError('a.mp4 '), 'NAME_SPACE');
      expect(nameError('a.mp4.'), 'NAME_SPACE');
    });
    test('路径分隔符与 . / ..', () {
      expect(nameError('a/b.mp4'), 'NAME_SEP');
      expect(nameError(r'a\b.mp4'), 'NAME_SEP');
      expect(nameError('..'), 'NAME_DOT');
      expect(nameError('.'), 'NAME_DOT');
    });
    test('Windows 非法字符与控制字符', () {
      for (final bad in ['a:b.mp4', 'a*b.mp4', 'a?b.mp4', 'a|b.mp4', 'a<b.mp4']) {
        expect(nameError(bad), 'NAME_WINCHAR', reason: '$bad 在 Windows 上不合法');
      }
      expect(nameError('换\n行.mp4'), 'NAME_CTRL');
    });
    test('Windows 保留设备名', () {
      expect(nameError('CON'), 'NAME_RESERVED');
      expect(nameError('con.mp4'), 'NAME_RESERVED');
      expect(nameError('COM1.mp4'), 'NAME_RESERVED');
      expect(nameError('LPT9.log'), 'NAME_RESERVED');
      // COM0 / LPT0 不是保留名，别跟着一起误伤
      expect(nameError('COM0.mp4'), isNull);
      expect(nameError('LPT0.mp4'), isNull);
    });
    test('长度上限按字节算（200）', () {
      expect(nameError('a' * 200), isNull);
      expect(nameError('a' * 201), 'NAME_LONG');
      expect(nameError('中' * 67), 'NAME_LONG');
      expect(nameError('中' * 66), isNull);
    });
  });
}
