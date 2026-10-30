import 'package:flutter_test/flutter_test.dart';
import 'package:yshjy_gui/humanize.dart';

/// 文案表 / 判断函数的单测（2026-10-30 补）
///
/// 为什么要有这个文件：加密包在界面上"只见报错、没有输入框"这个 bug，
/// 根因就是**页面里手写了引擎不存在的码名**（`PASSWORD_REQUIRED` /
/// `PASSWORD_WRONG`，引擎实际发 `NEED_PASSWORD` / `WRONG_PASSWORD`）。
/// 当时 `flutter test` 43 条全绿，因为**一条都没测密码路径**。
///
/// 所以这里干三件事：
/// 1. 把「引擎真会发的码」写成清单，逐个断言有专属文案（将来引擎加码、界面漏文案会红）；
/// 2. 断言 `codeNeedsPassword` / `codeCanRescue` 认的是真码，
///    并且**显式断言那几个历史废码为 false**（谁再改回去当场红）；
/// 3. 驼峰名也要认（协议 golden 里 `Failed` 带的是 Rust 枚举名 `WrongPassword`）。

/// 引擎 `src/error.rs` 里 `ErrorCode::as_str()` 的全部码（17 个）
/// 引擎加码时，这里和 `humanize.dart` 的 `errorText` 要一起加。
const engineErrorCodes = <String>[
  'PATH_NOT_FOUND',
  'INVALID_ARGUMENT',
  'NOT_ENOUGH_SPACE',
  'PERMISSION_DENIED',
  'PATH_TOO_LONG',
  'DEST_IS_FILE',
  'NEED_PASSWORD',
  'WRONG_PASSWORD',
  'HEAD_ENCRYPTED',
  'NOT_ARCHIVE',
  'TRUNCATED',
  'MISSING_VOLUME',
  'DATA_LOST',
  'ENCRYPTED_UNBREAKABLE',
  'VIDEO_UNPLAYABLE',
  'CANCELLED',
  'INTERNAL',
];

/// 引擎 `src/event.rs` 里 `DiagnosticCode::as_str()` 的码（10 个）
const engineDiagCodes = <String>[
  'HIDDEN_AT_OFFSET',
  'REBUILT_INDEX',
  'OFFSET_CORRECTED',
  'VOLUMES_MERGED',
  'LAYOUT_GUESSED',
  'VIDEO_UNPLAYABLE',
  'RISKY_ENTRY',
  'SLOW_PATH',
  'PASSWORD_FROM_NOTES',
  'PARTIAL_DATA',
];

void main() {
  group('文案表覆盖引擎真实码', () {
    test('错误码 17 个都有专属人话，不落到"原因不明"', () {
      const fallback = '这次没成功 —— 原因不明，可以点「生成诊断包」查一下';
      final missing = <String>[];
      for (final code in engineErrorCodes) {
        if (errorText(code) == fallback) missing.add(code);
      }
      expect(missing, isEmpty, reason: '这些码没有文案，用户会看到"原因不明"：$missing');
    });

    test('诊断码 10 个都有专属人话', () {
      const fallback = '有提示（不影响使用）';
      final missing = <String>[];
      for (final code in engineDiagCodes) {
        if (diagText(code) == fallback) missing.add(code);
      }
      expect(missing, isEmpty, reason: '这些诊断码没有文案：$missing');
    });
  });

  group('要不要给解压码输入框（历史 bug 的回归闸）', () {
    test('引擎真发的三个码都认', () {
      expect(codeNeedsPassword('NEED_PASSWORD'), isTrue);
      expect(codeNeedsPassword('WRONG_PASSWORD'), isTrue);
      expect(codeNeedsPassword('HEAD_ENCRYPTED'), isTrue);
    });

    test('驼峰名也认（协议 golden 里给的是枚举名）', () {
      expect(codeNeedsPassword('NeedPassword'), isTrue);
      expect(codeNeedsPassword('WrongPassword'), isTrue);
      expect(codeNeedsPassword('HeadEncrypted'), isTrue);
    });

    test('🎯 历史废码必须为 false —— 引擎从不发这两个', () {
      // 2026-10-30 的 bug 就写在这里：界面查的是这两个，条件恒假
      expect(codeNeedsPassword('PASSWORD_REQUIRED'), isFalse,
          reason: '引擎发的是 NEED_PASSWORD，不是 PASSWORD_REQUIRED');
      expect(codeNeedsPassword('PASSWORD_WRONG'), isFalse,
          reason: '引擎发的是 WRONG_PASSWORD，不是 PASSWORD_WRONG');
    });

    test('其它错误码不该触发密码框', () {
      for (final code in ['NOT_ARCHIVE', 'MISSING_VOLUME', 'DATA_LOST', 'TRUNCATED']) {
        expect(codeNeedsPassword(code), isFalse, reason: code);
      }
      expect(codeNeedsPassword(null), isFalse);
      expect(codeNeedsPassword(''), isFalse);
    });
  });

  group('要不要给"试着救出来"按钮', () {
    test('引擎真能救的码给按钮', () {
      expect(codeCanRescue('DATA_LOST'), isTrue);
      expect(codeCanRescue('TRUNCATED'), isTrue);
    });

    test('认不出包时只有"其实是 ZIP"才给', () {
      expect(codeCanRescue('NOT_ARCHIVE', looksLike: 'zip'), isTrue);
      expect(codeCanRescue('NOT_ARCHIVE', looksLike: 'ZIP'), isTrue);
      expect(codeCanRescue('NOT_ARCHIVE', looksLike: 'mp4'), isFalse);
      expect(codeCanRescue('NOT_ARCHIVE'), isFalse);
    });

    test('🎯 历史废码必须为 false（一个是诊断码、一个根本不存在）', () {
      // PARTIAL_DATA 是 DIAGNOSTIC 事件的诊断码，不会作为错误码出现
      expect(codeCanRescue('PARTIAL_DATA'), isFalse,
          reason: 'PARTIAL_DATA 是诊断码，不是 ErrorCode');
      // CHECKSUM_FAILED 引擎 17 个码里没有
      expect(codeCanRescue('CHECKSUM_FAILED'), isFalse,
          reason: '引擎不存在 CHECKSUM_FAILED 这个码');
    });

    test('密码/缺卷/空码一律不给救援按钮', () {
      expect(codeCanRescue('NEED_PASSWORD'), isFalse);
      expect(codeCanRescue('WRONG_PASSWORD'), isFalse);
      expect(codeCanRescue('HEAD_ENCRYPTED'), isFalse);
      expect(codeCanRescue('MISSING_VOLUME'), isFalse);
      expect(codeCanRescue(null), isFalse);
      expect(codeCanRescue(''), isFalse);
    });
  });
}
