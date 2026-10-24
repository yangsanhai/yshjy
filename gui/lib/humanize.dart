/// 码 → 中文人话（D21 / N1）
///
/// 规矩：**引擎只给码，外壳查文案**。这个文件就是「外壳那一份表」。
/// 两条铁律：
/// 1. 查不到的码**不给用户看原始码**（D21：缺一条就不显示），只给一句通用人话；
///    原始码仍在日志/诊断包里，需要排查时拿得到。
/// 2. 码的名字必须和引擎 `src/error.rs` 的 `as_str()` 一致（大写下划线），
///    以前界面写的是 `PASSWORD_REQUIRED`，引擎发的是 `NEED_PASSWORD` —— 对不上就漏translating。
library;

/// 错误码 → 人话（引擎 `ErrorCode::as_str()` + 外壳自己的几个码）
String errorText(String code) => switch (code) {
      'NOT_ARCHIVE' => '这里面没找到压缩包 —— 可能不是伪装包，或者文件下坏了',
      'MISSING_VOLUME' => '分卷不齐 —— 缺了一块，把缺的那块也下回来再试',
      'TRUNCATED' => '文件没下完（数据不完整）',
      'DATA_LOST' => '文件坏了，只能救回一部分',
      'ENCRYPTED_UNBREAKABLE' => '这个包加密了，而且没法试出解压码',
      'NEED_PASSWORD' => '这个包需要解压码',
      'WRONG_PASSWORD' => '解压码不对',
      'INVALID_ARGUMENT' => '参数不对（选的东西可能不合适）',
      'NOT_ENOUGH_SPACE' => '磁盘空间不够了，清一点再试',
      'PERMISSION_DENIED' => '没有权限读写这个位置（换个目录试试）',
      'PATH_NOT_FOUND' => '文件不存在（是不是被移走或改名了？）',
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

/// `WrongPassword` → `WRONG_PASSWORD`（已经是下划线式就原样返回）
String camelToCode(String s) {
  if (!s.contains(RegExp('[A-Z]')) || s.contains('_')) return s;
  final b = StringBuffer();
  for (var i = 0; i < s.length; i++) {
    final c = s[i];
    if (i > 0 && c.toUpperCase() == c && c.toLowerCase() != c) b.write('_');
    b.write(c.toUpperCase());
  }
  return b.toString();
}
