// 剪贴板文本 → 路径列表的纯函数回归（T11 的 Ctrl+V）
//
// 为什么值得一组单测：这一段的输入**完全不受我们控制**（剪贴板里可能是路径、
// 可能是 file:// URI、可能是别人刚复制的一段话）。写错的表现是「粘了没反应」
// 或「把无关文本当文件名去解压」，两种都只能靠拆开函数逐种输入钉死。
//
// 键位出处：UI-SPEC §4b.6「Ctrl+V 粘贴路径（自动开始识别）」。

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/clipboard_path.dart';

void main() {
  test('剪贴板是空的（null / 空串 / 纯空白）→ 一个路径都没有', () {
    expect(pathsFromClipboardText(null), isEmpty);
    expect(pathsFromClipboardText(''), isEmpty);
    expect(pathsFromClipboardText('   \n\t\n  '), isEmpty,
        reason: '纯空白被当成路径的话，会拿空字符串去问引擎，得到一个莫名其妙的错误');
  });

  test('单行绝对路径原样返回', () {
    expect(pathsFromClipboardText('/home/u/下载/包.mp4'), ['/home/u/下载/包.mp4']);
  });

  test('多行（文件管理器多选）→ 一行一个，顺序不变', () {
    final r = pathsFromClipboardText('/a/一.zip\n/b/二.zip\n/c/三.zip');
    expect(r, ['/a/一.zip', '/b/二.zip', '/c/三.zip'],
        reason: '顺序变了会让「第一个是主包」之类的判断出错');
  });

  test('CRLF / 单独 CR 也算换行（Windows 复制过来是 \\r\\n）', () {
    expect(pathsFromClipboardText('/a/一.zip\r\n/b/二.zip'), ['/a/一.zip', '/b/二.zip']);
    expect(pathsFromClipboardText('/a/一.zip\r/b/二.zip'), ['/a/一.zip', '/b/二.zip']);
  });

  test('每行首尾空白去掉，中间空格保留（文件名里真的有空格）', () {
    expect(pathsFromClipboardText('  /a/有 空格 的包.zip  \n'), ['/a/有 空格 的包.zip']);
  });

  test('成对引号去掉：双引号 / 单引号 / 中文引号 / 书名号式引号', () {
    expect(pathsFromClipboardText('"/a/包.zip"'), ['/a/包.zip']);
    expect(pathsFromClipboardText("'/a/包.zip'"), ['/a/包.zip']);
    expect(pathsFromClipboardText('“/a/包.zip”'), ['/a/包.zip']);
    expect(pathsFromClipboardText('「/a/包.zip」'), ['/a/包.zip']);
  });

  test('不成对的引号**不能**动 —— 那是文件名的一部分', () {
    expect(pathsFromClipboardText('/a/他说"你好".zip'), ['/a/他说"你好".zip']);
    expect(pathsFromClipboardText('"/a/只开头有引号.zip'), ['"/a/只开头有引号.zip']);
  });

  test('file:// URI 转成本地路径，%20 这类转义解回来', () {
    // `Uri.toFilePath()` 的结果按平台分隔符走：Windows 上是 `\home\u\...`。
    // 这里断言的是「转义解回来了、层级没丢」，不是「分隔符一定是 /」——
    // 写死 POSIX 会让 Windows 上的测试套件永远红一条（2026-11-07 踩过）。
    final expected = Platform.isWindows
        ? r'\home\u\我的 文档\包.zip'
        : '/home/u/我的 文档/包.zip';
    expect(
      pathsFromClipboardText('file:///home/u/我的%20文档/包.zip'),
      [expected],
    );
  });

  test('Windows 盘符路径不被当成 URI 弄坏', () {
    expect(pathsFromClipboardText(r'C:\Users\u\包.zip'), [r'C:\Users\u\包.zip'],
        reason: '按 scheme 判 URI 会把盘符 C: 当协议名，路径被糟蹋成无法使用的形状');
  });

  test('同一行粘两遍 → 只留一个（不该变成两个任务）', () {
    expect(pathsFromClipboardText('/a/包.zip\n/a/包.zip'), ['/a/包.zip']);
  });

  test('不判断文件存不存在 —— 那是调用方的事（纯函数不碰磁盘）', () {
    expect(pathsFromClipboardText('/绝对不存在的/路径.zip'), ['/绝对不存在的/路径.zip']);
  });
}
