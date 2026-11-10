// 批 7c：`previewUiState` 纯函数单测
//
// 这套默认值原来在 `home_page.dart` 里写了**两遍**（`_open` 一处、`initState` 的
// `debugPreview` 注入口一处），注释自己写着「两处要一起改」。抽成纯函数后一处生效，
// 这里钉住「拿到清单时界面该长什么样」的五条契约。
//
// 反向验证（改坏看红，2026-11-01 实测）：
//   把 `if (!e.isDir)` 去掉（目录也进 picked）→ 第 2 条立刻红。
//   目录进 select 的后果：引擎按序号找不到对应文件，表现为「解出来的东西跟列表不符」。

import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/common/engine.dart';
import 'package:yshjy_gui/desktop/pages/home_preview_state.dart';

EntryItem file(int index, String path) => EntryItem(
      index: index,
      path: path,
      size: 100,
      packed: 50,
      isDir: false,
      encrypted: false,
      risky: false,
    );

EntryItem dir(int index, String path) => EntryItem(
      index: index,
      path: path,
      size: 0,
      packed: 0,
      isDir: true,
      encrypted: false,
      risky: false,
    );

Preview pv({
  Map<String, dynamic>? cover,
  List<EntryItem>? entries,
}) =>
    Preview(
      format: 'zip',
      complete: true,
      totalSize: 300,
      encryptedCount: 0,
      encKind: null,
      cover: cover,
      entries: entries ??
          [
            file(1, '甲.txt'),
            dir(2, '目录/'),
            file(3, '目录/乙.txt'),
            file(4, '丙.txt'),
          ],
      hints: const [],
    );

void main() {
  test('有封面区才默认勾「伪装封面」；普通包不勾', () {
    expect(
      previewUiState(pv(cover: const {'size': 9000, 'kind': 'mp4', 'ext': 'mp4'}))
          .wantCover,
      isTrue,
    );
    expect(previewUiState(pv()).wantCover, isFalse,
        reason: '普通 zip 连封面那一行都不显示，勾着它等于让引擎白找一圈封面');
  });

  test('默认全选只勾**文件**，目录不进 picked', () {
    final s = previewUiState(pv());

    expect(s.picked, {1, 3, 4});
    expect(s.picked.contains(2), isFalse,
        reason: '目录不是协议 select 的合法锚点（只有文件能进），带上它会让引擎找不到条目');
  });

  test('换一个包一律回到「解压内容」', () {
    // 用户上一包取消了「解压内容」只提封面，不代表这一包也不解
    expect(previewUiState(pv()).wantExtract, isTrue);
  });

  test('预览树建一次、默认全展开（下钻前先看到全貌）', () {
    final s = previewUiState(pv());

    // 树：两个顶层（甲.txt / 丙.txt 是文件，目录是一个节点）
    expect(s.tree.map((n) => n.name).toList(), ['甲.txt', '目录', '丙.txt']);
    // 目录 path 不带尾斜杠（entry_tree 的约定），展开集合里就是它
    expect(s.expanded, {'目录'},
        reason: '默认展开所有目录 —— 新手先看到全貌才知道自己在解什么');
  });

  test('空包不炸：没有条目就没有勾选、没有树', () {
    final s = previewUiState(pv(entries: []));

    expect(s.picked, isEmpty);
    expect(s.tree, isEmpty);
    expect(s.expanded, isEmpty);
    expect(s.wantCover, isFalse);
    expect(s.wantExtract, isTrue);
  });

  test('每次都返回新的可变集合（页面直接 clear/addAll，共享常量会串状态）', () {
    final a = previewUiState(pv());
    final b = previewUiState(pv());
    a.picked.add(99);

    expect(b.picked.contains(99), isFalse,
        reason: '两次调用若共享同一个 Set，页面改一个包就污染另一个包的初始状态');
  });
}
