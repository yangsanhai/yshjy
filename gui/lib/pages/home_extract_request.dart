// 解压请求参数 —— 界面状态 → 协议参数的**唯一翻译处**（批 7b）
//
// 为什么单独一个文件（2026-11-01）：
//   这段逻辑原来长在 `_HomePageState._extract` 里，78 行里有 28 处判断：
//   「只检查时封面必须关」「换候选时不带勾选序号」「全选等于不传 select」……
//   一条测试都没有 —— 要测就得起假引擎走一整条 NDJSON 链路，那验的是协议不是判断。
//   抽成纯函数后，这些组合是 6 条单测就能钉死的东西。
//
// ★ 每条判断都不是想当然，来源写在各自的注释里（UI-SPEC / 协议 / 真机踩过的坑）。
//
// ⚠️ `_tryOtherCandidates`（队列行换候选解压）和 `_retryWithPassword`（失败页用新密码重试）
//    仍然**手写**各自的参数，故意不动它们：
//   - 那两处传给引擎的是**另一组**参数（都不传 cover / wantExtract，靠引擎默认值 true），
//     套进这个函数会顺手把「重试时封面跟不跟界面勾选」这条行为改掉；
//   - 本批的目标是「让最密的那处判断可测」，不是顺手统一三处 —— 统一要先有测试证明三处行为一致。
//   真要统一时：先给那两处补行为测试，再合。

import '../engine.dart';

/// 一次解压请求的全部参数（字段与 `EngineApi.extract` 的具名参数一一对应）
class ExtractRequest {
  final String path;
  final String? dest;
  final String? password;
  final bool cover;
  final bool wantExtract;
  final List<int>? select;
  final String? encoding;
  final bool force;
  final int? candidate;
  final bool verifyOnly;
  final bool? smartDir;

  const ExtractRequest({
    required this.path,
    this.dest,
    this.password,
    required this.cover,
    required this.wantExtract,
    this.select,
    this.encoding,
    this.force = false,
    this.candidate,
    this.verifyOnly = false,
    this.smartDir,
  });

  /// 发给引擎。**页面只用这个方法**，别手抄一遍 11 个字段 ——
  /// 手抄迟早漏一个，而漏掉的表现是「界面上勾了、引擎没收到」（0.9.47 jobs 假开关同款）。
  Future<ExtractResult> send(EngineApi api) => api.extract(
        path,
        dest: dest,
        password: password,
        cover: cover,
        wantExtract: wantExtract,
        select: select,
        encoding: encoding,
        force: force,
        candidate: candidate,
        verifyOnly: verifyOnly,
        smartDir: smartDir,
      );
}

/// 把「界面此刻的状态」翻成一条解压请求。
///
/// [preview] 为 null 表示还没有清单（极端情况，比如清单请求失败后用户硬点）——
/// 这时按「全选」处理：不传 `select` 让引擎解全部，总比传个空列表解出零个文件强。
ExtractRequest buildExtractRequest({
  required String path,
  required Preview? preview,
  required Set<int> picked,
  required bool wantCover,
  required bool wantExtract,
  required String? password,
  required String? outDir,
  required String? encoding,
  required bool? smartDir,
  bool force = false,
  int? candidate,
  bool verifyOnly = false,
}) {
  // 勾的是不是「全部文件」（目录不算 —— 目录本来就不用单独勾）
  final pickedAll =
      preview == null || picked.length == preview.entries.where((e) => !e.isDir).length;

  return ExtractRequest(
    path: path,
    // 「只检查」一个字节都不写：给了 dest 引擎可能顺手把目录建出来（CLI --test 实测零字节）
    dest: verifyOnly ? null : outDir,
    password: password,
    // 切封面是**另一条真写盘的路径**。只检查时开着它就与「不动硬盘」矛盾（实测 CLI --test 是零字节）
    cover: verifyOnly ? false : wantCover,
    // 「只检查」断的是「整包完不完好」，不跟界面的「只解清单」勾选走
    wantExtract: verifyOnly ? true : wantExtract,
    // 四种情况**不传** select（= 引擎全解，与老行为一致）：
    //   ① 只检查：校验的是整包
    //   ② 用户没勾「解压内容」（只提封面）
    //   ③ 全选：多传一个数组不仅没用，序号对不上还会漏解
    //   ④ 换候选：另一段里的条目跟当前预览完全可能不同，带旧序号等于用 A 包的清单挑 B 包的文件
    select: (verifyOnly || !wantExtract || pickedAll || candidate != null)
        ? null
        : picked.toList(),
    encoding: encoding,
    // S5「继续试试」：跳过空间预检（引擎侧早支持，界面以前一个参数都没传）
    force: force,
    // S10：只解用户点的那一处（`locate_at_base` 精确切，对不上报 NOT_ARCHIVE，绝不偷偷解默认那处）
    candidate: candidate,
    verifyOnly: verifyOnly,
    // E5 智能解压：只有不给 dest 时才看得出区别（给了 dest 引擎一律铺在 dest 下）
    smartDir: smartDir,
  );
}

/// 底部主按钮「该不该亮」的唯一判断（T11 键盘要用，和按钮共用同一条规矩）
///
/// 为什么抽出来：预览页那个青色主按钮的 `onPressed` 里写着
/// `(_wantExtract && _picked.isEmpty) || (!_wantExtract && !(pv.hasCover && _wantCover)) ? null : _extract`。
/// 键盘接 Enter / Ctrl+E 时要判断同一件事 —— 复制一份就是「按钮亮了但键不响应」
/// 或更糟的「按钮灰着，按 Enter 却真去解了」。
///
/// 规则（与按钮逐条对齐）：
///   · 勾了「解压内容」→ 至少选中一个条目才能开始
///   · 只提封面（没勾解压内容）→ 必须有封面区、且用户勾了封面
bool canStartExtract({
  required Preview? preview,
  required Set<int> picked,
  required bool wantExtract,
  required bool wantCover,
}) {
  if (preview == null) return false;
  if (wantExtract) return picked.isNotEmpty;
  return preview.hasCover && wantCover;
}
