/// 码 → 中文人话（D21 / N1）
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
library;

/// 错误码 → 人话（引擎 `ErrorCode::as_str()` + 外壳自己的几个码）
String errorText(String code) => switch (code) {
      'NOT_ARCHIVE' => '这里面没找到压缩包 —— 可能不是伪装包，或者文件下坏了',
      'MISSING_VOLUME' => '分卷不齐 —— 缺了一块，把缺的那块也下回来再试',
      'TRUNCATED' => '文件没下完（数据不完整）',
      'DATA_LOST' => '文件坏了，只能救回一部分',
      'ENCRYPTED_UNBREAKABLE' => '这个包加密了，而且没法试出解压码',
      'NEED_PASSWORD' => '这个包需要解压码',
      // 头加密：连文件清单都看不到，必须强调「清单也加密了」
      'HEAD_ENCRYPTED' => '这个包连文件清单都加密了，必须给解压码才能看里面有什么',
      'WRONG_PASSWORD' => '解压码不对',
      'INVALID_ARGUMENT' => '参数不对（选的东西可能不合适）',
      'NOT_ENOUGH_SPACE' => '磁盘空间不够了，清一点再试',
      'PERMISSION_DENIED' => '没有权限读写这个位置（换个目录试试）',
      'PATH_TOO_LONG' => '路径太长，这个位置放不下这么长的文件名',
      'DEST_IS_FILE' => '目标位置是个文件，不是文件夹（换个文件夹）',
      'PATH_NOT_FOUND' => '文件不存在（是不是被移走或改名了？）',
      // 封面视频本身坏了：解压不受影响，但错误码通道也会出现这个码
      'VIDEO_UNPLAYABLE' => '封面视频本身坏了（不影响解压）',
      // `IO` 是外壳自造的旧码（引擎 17 个码里没有，留着兼容老日志）
      'IO' => '读写文件出错了（磁盘满了 / 磁盘有问题）',
      'CANCELLED' => '已取消（已经解出来的文件都保留着）',
      'INTERNAL' => '程序内部出错了 —— 可以点「生成诊断包」发给开发者',
      // ↓ 下面几条是外壳自己造的码（引擎不会发）
      'ENGINE_EXITED' => '引擎进程退出了（重开一次界面就好）',
      'TIMEOUT' => '等太久了，没等到引擎回应',
      'BAD_PARAM' => '界面传的参数不对（这是软件的问题，麻烦报一下）',
      'UNKNOWN_COMMAND' => '引擎不认这条命令（版本可能对不上）',
      _ => '这次没成功 —— 原因不明，可以点「生成诊断包」查一下',
    };

/// 归档级诊断码 → 人话（引擎 `DiagnosticCode::as_str()`）
String diagText(String code) => switch (code) {
      'HIDDEN_AT_OFFSET' => '包藏在文件中间，已按真实位置定位',
      'REBUILT_INDEX' => '索引坏了，清单是按文件头重建的',
      'OFFSET_CORRECTED' => '偏移不对，已自动修正',
      'VOLUMES_MERGED' => '分卷已自动拼起来了',
      'LAYOUT_GUESSED' => '布局是猜出来的，没十足把握',
      'VIDEO_UNPLAYABLE' => '封面视频本身坏了（不影响解压）',
      'RISKY_ENTRY' => '解出了可执行文件，运行前请确认来源',
      'SLOW_PATH' => '走了慢路（全文件扫描），请耐心等',
      'PASSWORD_FROM_NOTES' => '解压码是从旁边的说明里找到的',
      'PARTIAL_DATA' => '数据不完整，能救多少算多少',
      'MOVABLE_NOT_VERIFIED' =>
        '目标是U盘/移动硬盘：拔盘前请先「安全弹出」，否则可能有数据还没写进盘',
      _ => '有提示（不影响使用）',
    };

/// 条目级结果 → 人话（引擎 `ItemResult::as_str()`）
String itemResultText(String code) => switch (code) {
      'OK' => '成功',
      'SKIPPED' => '跳过',
      'CRC_ERROR' => '校验不过（内容坏了）',
      'DATA_ERROR' => '数据损坏',
      'ENCRYPTED' => '加密，没解开',
      'NOT_EXTRACTED' => '没解出来',
      _ => '未完成',
    };

/// 批量结果里的任务结论 → 人话
///
/// 协议里：`ok` / `partial` / `cancelled` 是小写固定串，失败时给的是错误码
/// （老引擎给 Rust 的 Debug 名 `WrongPassword`，新引擎给 `WRONG_PASSWORD`，两种都认）。
String batchOutcomeText(String oc) {
  switch (oc) {
    case 'ok':
      return '完成';
    case 'partial':
      return '部分完成';
    case 'cancelled':
      return '已取消';
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
