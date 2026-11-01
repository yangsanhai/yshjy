// `canStartExtract` 纯函数单测（T11 键盘带出来的）
//
// 这个函数存在的唯一理由：预览页那个青色主按钮「亮不亮」和键盘 Enter/Ctrl+E
// 「响不响应」必须是**同一条判断**。以前按钮的 onPressed 里手写着一串条件，
// 键盘再写一遍就是两份真相 —— 两种典型事故：
//   ① 按钮亮着，按 Enter 没反应（用户以为键盘坏了）
//   ② 按钮灰着，按 Enter 却真去解了（用户以为程序乱来）
//
// 规则出处：`home_extract_request.dart` 的 canStartExtract 注释 + 主按钮原条件。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/engine.dart';
import 'package:yshjy_gui/pages/home_extract_request.dart';

Preview pv({bool cover = false}) => Preview(
      format: 'zip',
      complete: true,
      totalSize: 100,
      encryptedCount: 0,
      encKind: null,
      cover: cover ? {'path': '/tmp/c.mp4', 'size': 30, 'ext': 'mp4'} : null,
      entries: const [],
      hints: const [],
    );

void main() {
  test('没有预览（还没识别出包）→ 不能开始', () {
    expect(
      canStartExtract(
          preview: null, picked: {1}, wantExtract: true, wantCover: true),
      isFalse,
    );
  });

  test('勾「解压内容」但一个条目都没选 → 不能开始', () {
    expect(
      canStartExtract(
          preview: pv(), picked: {}, wantExtract: true, wantCover: false),
      isFalse,
      reason: '一个都没选还去解，引擎会报参数错；按钮那时也是灰的',
    );
  });

  test('勾「解压内容」且选了条目 → 能开始', () {
    expect(
      canStartExtract(
          preview: pv(), picked: {1}, wantExtract: true, wantCover: false),
      isTrue,
    );
  });

  test('只提封面：有封面且勾了封面 → 能开始', () {
    expect(
      canStartExtract(
          preview: pv(cover: true), picked: {}, wantExtract: false, wantCover: true),
      isTrue,
    );
  });

  test('只提封面但没勾封面 → 不能开始（那样等于什么都不做）', () {
    expect(
      canStartExtract(
          preview: pv(cover: true), picked: {}, wantExtract: false, wantCover: false),
      isFalse,
      reason: '既不提内容也不提封面，按下去只会空跑一次',
    );
  });

  test('只提封面但这个包根本没封面 → 不能开始', () {
    expect(
      canStartExtract(
          preview: pv(), picked: {1}, wantExtract: false, wantCover: true),
      isFalse,
      reason: '没有封面区却勾着封面，按下去引擎会找不到封面文件',
    );
  });

  test('只提封面时选中项不算数（那是内容，不是封面）', () {
    expect(
      canStartExtract(
          preview: pv(), picked: {1, 2, 3}, wantExtract: false, wantCover: true),
      isFalse,
    );
  });
}
