// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
import 'dart:async';
import 'dart:io';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import '../humanize.dart';
import '../key_action_scope.dart';
import '../keyboard.dart';
import '../l10n.dart';
import '../notify.dart';
import '../pack_naming.dart';
import '../platform_paths.dart';

/// 打包页：把文件/文件夹做成"能播放的伪装包"
///
/// 走引擎的 `pack` 命令（协议 v1）。图片/文本封面会自动用「尾部追加」布局，
/// 原因见 PROTOCOL.md / pack/cover.rs：free 原子是 MP4 的 ISO 结构，塞进图片会写坏封面。
class PackPage extends StatefulWidget {
  final ValueChanged<String>? onStatus;

  /// 测试用：预设输入 / 输出 / 外壳（生产代码永远传 null）。
  ///
  /// 为什么留这个口子：「勾了「防网盘扫描」到底有没有把 `decoy` 传给引擎」只有
  /// 真点一次「开始打包」才验得到，而真机上加文件要走系统文件对话框 ——
  /// widget 测试点不到那个对话框，这条接线（界面 → 协议字段名）就没人守。
  /// 与 HomePage 的 `debugPreview` / `debugScanItems` 同一个理由：只影响测试。
  final List<String>? debugInputs;
  final String? debugOut;
  final String? debugCover;

  const PackPage({
    super.key,
    this.onStatus,
    this.debugInputs,
    this.debugOut,
    this.debugCover,
    this.acceptDrop = true,
  });

  /// 当前页是否接受拖放。
  ///
  /// IndexedStack 里所有页都挂在树上：首页那个 `DropTarget` 铺满整个窗口，
  /// 不关掉的话在打包页拖文件会被**首页抢走**（变成解压/预览）。
  final bool acceptDrop;

  @override
  State<PackPage> createState() => _PackPageState();
}

class _PackPageState extends State<PackPage> {
  final EngineClient _client = EngineClient.instance;

  final List<String> _inputs = [];
  /// 输出文件（完整路径）。改名输入框的文本由 setter 同步 ——
  /// 只维护**一份**状态（`_out`），输入框里显示 `baseName(_out)`；
  /// 另记一份「名字」迟早两边对不上（T18-2 做显示名时的定法）。
  String? _outRaw;
  final TextEditingController _nameCtrl = TextEditingController();
  String? get _out => _outRaw;
  set _out(String? v) {
    _outRaw = v;
    final want = v == null ? '' : baseName(v);
    if (_nameCtrl.text != want) {
      _nameCtrl.value = TextEditingValue(
        text: want,
        selection: TextSelection.collapsed(offset: want.length),
      );
    }
  }
  /// 用户自己挑过输出位置？（挑过就不再自动改名字，尊重用户选择）
  ///
  /// 以前 `_out` 用 `??=` 赋值，结果是「只建议一次」，换输入文件后输出名还停在上一个，
  /// 文件按旧名生成 → 用户按界面上的新名去找，当然找不到（2026-10-30 用户报的）。
  bool _outUserPicked = false;
  /// 打包模式（GUI-PLAN §5.2②）：普通压缩是默认，伪装是特殊功能
  String _mode = 'plain';
  /// 归档格式（T17-4）：zip / tar.gz / tar.zst
  ///
  /// **只有普通压缩能选** —— 伪装包只能是 zip（封面 + tar 系的包定位不出来，
  /// 见引擎 `pack::PackFormat` 的探针注释）。所以选 tar 时密码 / 分卷 / 防扫描
  /// 全部置灰并写明原因，而不是「点了才报错」。
  String _format = 'zip';
  bool get _isTar => _format != 'zip';
  /// 用户挑的伪装文件（外壳）：任意类型，用户选什么就是什么
  String? _coverPath;
  int _coverSize = 0;
  /// 外壳扩展名 —— 决定输出文件的扩展名（必须一致，不然一认文件头就露馅）
  String _coverExt = '';
  /// 自解压（第 23 批）：生成双击就解压的包，不需要外壳文件
  bool _sfx = false;
  /// 自解压种类（P23-5）：windows = exe、linux = .sh
  String _sfxKind = 'windows';
  final TextEditingController _pwCtrl = TextEditingController();
  /// 解压码是否显示明文（眼睛图标）
  bool _showPw = false;
  /// 交付选项（DESIGN §17.8）：分卷 / 校验和 / 说明书
  /// 压缩级别（P23-4）：0 = 仅存储最快 / 6 = 标准 / 9 = 最小体积
  int _level = 6;
  bool _split = false;
  bool _checksum = true;
  /// 防炸链（T18-3）：打乱条目时间戳，让网盘判不成同一个文件（默认关）
  bool _tweakTime = false;
  bool _readme = true;
  // ── 干扰数据（E1 / 协议 pack.decoy*）──
  // 目的：让网盘/在线扫描以为这是普通视频，而不是「一个带 ZIP 尾巴的文件」。
  // 段数和每段大小走默认（4 段 × 16 KB ≈ 64 KB，用户看不出体积变化），
  // 界面只给「开 / 关」和一个更保险的子开关 —— 新手不需要懂「段」是什么。
  bool _decoy = false;
  bool _decoySignatures = false;
  static const int _decoySegments = 4;

  /// 只有「伪装成文件、且不是自解压」才允许撒干扰数据。
  /// 引擎里 decoy 与 `mode:"plain"`、`sfx` 互斥，会回 BAD_PARAM ——
  /// 所以这里直接禁用并说明，而不是做一个点了才报错的假开关。
  bool get _canDecoy => _mode == 'disguise' && !_sfx;
  // ── 数据藏在哪（协议 `pack.layout`；`tail` 就是老写法 `pack.tail` 的那一档）──
  //   free = 藏进封面的 free 原子（默认：视频能播，8 种工具全通）
  //   tail = 封面一个字节都不改，数据接在最后（最保险）
  //   head = 放进文件头部（T18-1 的 L3）：ZIP 包成 free 原子塞在 ftyp 后面
  //   mid  = 插进视频数据中间（T18-1 的 L4）：视频照常能播，包藏得比尾巴深
  //   two  = 一个文件里放两个包（T18-1 的 L7）：封面 + 本次输入 + 第二个现成 zip
  String _layout = 'free';
  /// L7 的第二个包（现成 zip 的绝对路径）。只在这一档用，切走不清空 ——
  /// 用户切来切去别把辛苦挑的文件弄丢。
  String? _secondPath;
  /// 本次打包「请求的」布局（结果页拿它和引擎实际用的对账，降级了要告诉用户）
  String? _askedLayout;
  /// 没有封面就谈不上布局（普通压缩、自解压）
  bool get _canTail => _mode == 'disguise' && !_sfx;
  /// 「插在视频中间」「放在文件头部」只对视频外壳成立。
  ///
  /// `_coverExt` 是**按文件头魔数**认出来的（选封面时 `shellExtFromHead`），
  /// 不是看用户起的名字 —— 把 PNG 改名成 .mp4 也骗不过去。
  /// 引擎那边还会再核一次，认不出就降级（结果页会说明）。
  bool get _canMid {
    if (!_canTail) return false;
    final e = _coverExt.isEmpty ? 'mp4' : _coverExt;
    return e == 'mp4' || e == 'mov' || e == 'm4v';
  }
  final _splitCtrl = TextEditingController(text: '2G');
  bool _busy = false;
  String? _error;
  Map<String, dynamic>? _result;
  /// 打包进度（PACK_PROGRESS，N2）：打几 GB 的包时界面不能是死的
  Progress? _progress;
  String? _current;
  StreamSubscription<EngineEvent>? _sub;
  // ── 打包产物的落盘检验（引擎的 FLUSH_PROGRESS / FLUSH_FINISHED）──
  // 打包到 U 盘时，屏幕报「完成」那一刻数据可能还在缓存里；
  // 而且它紧接着的自检读的也是缓存 —— 所以这一步要显示出来，别让用户以为卡死。
  bool _flushing = false;
  int _flushPending = 0;
  int _flushSecs = 0;

  @override
  void initState() {
    super.initState();
    // 引擎事件是广播流：打包页订阅自己关心的（解压页订阅它自己的）
    _sub = _client.events.listen(_onEvent);
    // 测试注入口：灌一份「已经选好输入 + 外壳」的状态（生产永远 null）。
    // 与 HomePage.debugPreview 同一个理由 —— 只影响测试，不改任何行为。
    final in0 = widget.debugInputs;
    if (in0 != null) {
      _inputs.addAll(in0);
      _out = widget.debugOut;
      final cover = widget.debugCover;
      if (cover != null) {
        _mode = 'disguise';
        _coverPath = cover;
        final ext = extOf(cover);
        _coverExt = ext.isEmpty ? 'bin' : ext;
        // 与 _pickCover 里的「输出名跟着外壳走」保持一致，
        // 否则测试验的是另一套初始状态（两处要一起改）。
        if (_out != null) _out = reExt(_out!, _coverExt);
      }
    }
  }

  /// 只关心打包进度和它的落盘检验，别的 kind（解压进度/诊断…）不归这页管
  void _onEvent(EngineEvent e) {
    if (!mounted) return;
    switch (e.kind) {
      case 'PACK_PROGRESS':
        setState(() {
          _progress = e.progress;
          _current = e.current;
        });
      case 'FLUSH_PROGRESS':
        setState(() {
          _flushing = true;
          _flushPending = (e.data['pending'] as num?)?.toInt() ?? 0;
          _flushSecs = ((e.data['elapsed_ms'] as num?)?.toInt() ?? 0) ~/ 1000;
        });
      case 'FLUSH_FINISHED':
        setState(() {
          _flushing = false;
        });
      default:
        break;
    }
  }

  @override
  void dispose() {
    _sub?.cancel();
    _pwCtrl.dispose();
    _nameCtrl.dispose();
    super.dispose();
  }

  /// 拖进来的文件/文件夹 → 加进输入列表（与「添加文件/文件夹」同一套去重与命名规则）
  void _addDropped(Iterable<String> paths) {
    if (_busy) return;
    var added = false;
    for (final p in paths) {
      if (p.isEmpty || _inputs.contains(p)) continue;
      _inputs.add(p);
      added = true;
    }
    if (!added) return;
    if (!_outUserPicked) _out = suggestOut(_inputs.first, _ext);
    setState(() {});
  }

  Future<void> _addFiles() async {
    final group = XTypeGroup(label: t('pack.filter.any'));
    final files = await openFiles(acceptedTypeGroups: [group]);
    if (files.isEmpty) return;
    setState(() {
      for (final f in files) {
        if (!_inputs.contains(f.path)) _inputs.add(f.path);
      }
      // 名字跟着当前输入走；用户手动挑过输出位置就不再覆盖
      if (!_outUserPicked) _out = suggestOut(_inputs.first, _ext);
    });
  }

  Future<void> _addFolder() async {
    final dir = await getDirectoryPath();
    if (dir == null) return;
    setState(() {
      if (!_inputs.contains(dir)) _inputs.add(dir);
      if (!_outUserPicked) _out = suggestOut(dir, _ext);
    });
  }

  /// 当前模式对应的输出扩展名
  /// （普通 = .zip；自解压 = .exe / .sh；伪装 = 外壳的扩展名；L8 = .mkv）
  ///
  /// L8（MKV 附件）放在最前面判：那套布局的产物是个**真的 MKV**（ffmpeg 重封装），
  /// 跟封面原本是 mp4 还是 png 无关 —— 名字必须 .mkv，不然播放器按扩展名打开会懵。
  String get _ext => _mode == 'plain'
      ? _format
      : (_sfx
          ? (_sfxKind == 'linux' ? 'sh' : 'exe')
          : (_layout == 'mkv'
              ? 'mkv'
              : (_coverExt.isEmpty ? 'mp4' : _coverExt)));


  /// 挑「第二个包」（L7）：只收 .zip。
  ///
  /// 界面按扩展名先筛一道，让用户在选文件时就看得出「这里要的是压缩包」；
  /// 真正的判断在引擎（读文件头 `PK\x03\x04`）—— 扩展名可以骗人，文件头不行。
  Future<void> _pickSecond() async {
    final group = XTypeGroup(label: t('pack.filter.zip'), extensions: const ['zip']);
    final f = await openFile(acceptedTypeGroups: [group]);
    if (f == null) return;
    setState(() => _secondPath = f.path);
  }

  /// 挑伪装文件：任意类型都行（不设白名单）。
  ///
  /// 用户选什么，产物就是「这个文件的完整内容 + 尾部压缩数据」。
  /// 输出扩展名自动跟着外壳走 —— 名字和内容不一致会一认文件头就露馅。
  Future<void> _pickCover() async {
    // 不传 extensions / mimeTypes = 允许所有文件（任意类型都能当外壳）
    final group = XTypeGroup(label: t('pack.filter.all'));
    final f = await openFile(acceptedTypeGroups: [group]);
    if (f == null) return;
    final size = File(f.path).lengthSync();
    // 外壳类型按**文件头魔数**认（与 CLI 同一套）：用户把 PNG 存成「封面.dat」也认得出。
    // 认不出才退回文件名扩展名 —— 后缀必须和里面的真实内容一致，否则一认文件头就露馅。
    List<int> head = const <int>[];
    try {
      final raf = File(f.path).openSync();
      head = raf.readSync(64);
      raf.closeSync();
    } catch (_) {
      // 读不了头就当认不出（走扩展名兜底），不拦着用户选文件
    }
    final magic = shellExtFromHead(head);
    setState(() {
      _coverPath = f.path;
      _coverSize = size;
      final ext = magic ?? extOf(f.path);
      _coverExt = ext.isEmpty ? 'bin' : ext;
      if (_out != null) {
        _out = reExt(_out!, _coverExt);
      } else if (_inputs.isNotEmpty) {
        _out = suggestOut(_inputs.first, _ext);
      }
    });
  }

  Future<void> _pickOut() async {
    final loc = await getSaveLocation(
        suggestedName: _out?.split('/').last ?? 'packed.$_ext');
    if (loc == null) return;
    setState(() {
      _outUserPicked = true;
      if (_mode == 'plain') {
        // 普通压缩产物就是选的那种归档，名字必须对得上（用户手打 .7z 也不让）
        _out = reExt(loc.path, _format);
      } else {
        _out = reExt(loc.path, _ext);
      }
    });
  }

  /// 分卷大小的实时校验（看不懂就红字，绝不静默变成「不分卷」）
  String? get _splitSizeError {
    if (!_split) return null;
    final text = _splitCtrl.text.trim();
    if (text.isEmpty) return t('pack.size.hint');
    if (_parseSize(text) == null) return t('pack.size.error');
    return null;
  }

  /// 改名输入框（T18-2）：只改**文件名**，目录沿用当前输出目录。
  ///
  /// 这里**不走 `_out` 的 setter**：setter 会用 `baseName` 回写输入框，而用户打了
  /// `a/b.mp4` 时 `baseName` 只剩 `b.mp4` —— 违规的斜杠被抹掉、校验形同虚设，
  /// 输入框还会自己跳字（widget 测试撞到）。直接改底层字段，输入框文本归用户所有。
  void _onNameChanged(String text) {
    setState(() {
      _outRaw = '${_out == null ? '' : dirOf(_out!)}$text';
      // 用户亲手改过名字 = 挑过位置，换输入文件时不再自动覆盖（与「另存为」同一语义）
      _outUserPicked = true;
    });
  }

  /// 输入框下面的红字：空 / 非法名。`_out == null` 时不算错误（用户还没选东西）。
  ///
  /// **校验的是输入框原文，不是 `baseName(_out)`** —— 用户打 `a/b.mp4` 时 `_out` 会被拼成
  /// `/tmp/a/b.mp4`，`baseName` 只剩 `b.mp4`，违规的斜杠正好被抹掉、看着像没事（widget 测试撞到）。
  String? get _nameErrorText {
    if (_out == null) return null;
    final e = nameError(_nameCtrl.text);
    if (e == null) return null;
    return e == 'NAME_EMPTY' ? t('pack.output.nameEmpty') : t('pack.output.nameInvalid');
  }

  /// 错误码 → 人话（界面不糊 code；引擎码查 `humanize.dart`，界面自己的码在这补）
  String _errorText(String code) => switch (code) {
        'NO_COVER' => t('pack.err.needCover'),
        'NO_SECOND' => t('pack.err.needSecond'),
        'SPLIT_SIZE_INVALID' => t('pack.err.badVolume'),
        'BAD_NAME' => t('pack.err.badName'),
        _ => errorText(code),
      };

  Future<void> _pack() async {
    if (_inputs.isEmpty || _out == null) return;
    // 显示名（T18-2）：界面即时红字已经在输入框下面了，这里再兜一道 ——
    // 空的 / 带分隔符的名字不能发出去（引擎那头是路径，别让「改名」变成「换位置」）。
    final nameErr = nameError(_nameCtrl.text);
    if (nameErr != null) {
      setState(() => _error = 'BAD_NAME');
      return;
    }
    final isSfx = _mode == 'disguise' && _sfx;
    if (!isSfx && _mode == 'disguise' && _coverPath == null) {
      setState(() => _error = 'NO_COVER');
      return;
    }
    // L7：选了「两个包」却没挑第二个包 —— 引擎会拒（BAD_PARAM），但在这里拦能立刻红字
    if (_canTail && _layout == 'two' && (_secondPath == null || _secondPath!.isEmpty)) {
      setState(() => _error = 'NO_SECOND');
      return;
    }
    final splitErr = _splitSizeError;
    if (splitErr != null) {
      setState(() => _error = 'SPLIT_SIZE_INVALID');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
      _result = null;
      _progress = null;
      _current = null;
      // 记下「这次要的是哪种布局」—— 引擎可能降级（封面根本插不进中间），
      // 结果页拿这个和返回值对账，降级了要明说，不能让用户以为选中的生效了
      _askedLayout = _canTail ? _layout : null;
    });
    widget.onStatus?.call(t('pack.busy'));
    try {
      // 普通压缩产物永远是 ZIP；伪装产物扩展名必须和外壳一致。
      // 用 reExt 而不是 withExt：用户手写的 `电影.2026.1080p` 要补成 `.mp4`，
      // 不能被理解成「把 1080p 换成 mp4」（T18-2）。
      final out = _mode == 'plain'
          ? reExt(_out!, _format)
          : reExt(_out!, _ext);
      final r = await _client.pack(
        inputs: _inputs,
        out: out,
        // 归档格式（T17-4）：只有普通压缩能指定，伪装包只能是 zip
        format: _mode == 'plain' ? _format : null,
        // tar 系没有加密 —— 界面上那格已经置灰，这里再兜一道
        // （用户先前在 zip 下填过密码、切到 tar 后那格是灰的，别把看不见的值偷偷传下去）
        password: _isTar
            ? null
            : (_pwCtrl.text.trim().isEmpty ? null : _pwCtrl.text.trim()),
        cover: _mode == 'disguise' && !isSfx ? _coverPath : null,
        sfxKind: isSfx ? _sfxKind : null,
        splitBytes: !isSfx && !_isTar && _split ? _parseSize(_splitCtrl.text) : null,
        level: _level,
        // 防网盘扫描（E1）：只有伪装模式、非自解压才传 —— 引擎里 decoy 与
        // mode:"plain"、sfx 互斥（回 BAD_PARAM），不做「点了才报错」的假开关。
        decoy: _canDecoy && _decoy ? _decoySegments : null,
        decoySignatures: _canDecoy && _decoy && _decoySignatures,
        // 数据藏在哪（E2 的「不改封面」是这里的一档；T18-1 又加了 head / mid / two）。
        // 不传 = 老行为：引擎按封面文件头自己选。
        layout: _canTail ? _layout : null,
        // L7 的第二个包：只在「两个包」那一档才传（别的档传了引擎会拒）
        second: _layout == 'two' ? _secondPath : null,
        checksum: _checksum,
        // 防炸链（T18-3）：默认关，开着才传
        tweakTime: _tweakTime,
        // 普通压缩不写「这不是视频」的说明书（用户验收 2026-10-17）；自解压也不需要
        readme: _mode == 'disguise' && !isSfx ? _readme : false,
        mode: _mode,
      );
      if (!mounted) return;
      setState(() => _result = r);
      widget.onStatus?.call(t('pack.done'));
      Notifier.send(t('pack.done'), t('pack.done.hint'));
    } on EngineError catch (e) {
      if (!mounted) return;
      setState(() => _error = e.code);
      // 五态审计：失败态要「有话说」——裸错误码（SPLIT_SIZE_INVALID）用户看不懂
      widget.onStatus?.call(t('pack.failed', {'reason': errorText(e.code)}));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// 解析「2G / 500M / 1048576」为字节数（看不懂返回 null = 不分卷）
  int? _parseSize(String s) {
    final m = RegExp(r'^([0-9.]+)\s*(GB|G|MB|M|KB|K|B)?$')
        .firstMatch(s.trim().toUpperCase());
    if (m == null) return null;
    final v = double.tryParse(m.group(1)!);
    if (v == null || v <= 0) return null;
    final mult = switch (m.group(2)) {
      'GB' || 'G' => 1024 * 1024 * 1024,
      'MB' || 'M' => 1024 * 1024,
      'KB' || 'K' => 1024,
      _ => 1,
    };
    return (v * mult).toInt();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return DropTarget(
      // 打包页也能直接把文件/文件夹拖进来（新手第一反应就是拖）
      enable: widget.acceptDrop && !_busy,
      onDragDone: (detail) {
        if (detail.files.isEmpty) return;
        _addDropped(detail.files.map((f) => f.path));
      },
      child: KeyActionScope(
      // ★ takeFocus 必须 false：这一页和首页在 IndexedStack 里同时挂着，
      //   谁抢焦点谁出事 —— 它会静止地把首页那屏的 Enter 抢走。
      //   代价是「刚切过来什么都没点、直接按 Enter」不生效；用户要打包必然
      //   先在本页点过东西（焦点已在页内），正常流程不受影响。
      takeFocus: false,
      // 键位条件与按钮 onPressed **同一个表达式**，不做第二份判断：
      // 按钮灰着按 Enter 也不该打包，按钮亮着按 Enter 就该打包。
      handlers: {
        KeyAction.primary: _busy || _inputs.isEmpty ? null : () => _pack(),
      },
      child: Padding(
        padding: const EdgeInsets.all(16),
      // 整页可滚动：伪装模式下光「外壳说明」就五六行，再加「防网盘扫描」两行，
      // 850x736 的默认窗口会 RenderFlex 溢出（真机截图里那条
      // BOTTOM OVERFLOWED BY 20 PIXELS 就是它）。列表改成受控高度的滚动区，
      // 外层整体可滚 —— 窗口再矮也只是要滚，不会再挤爆。
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              _mode == 'plain' ? t('pack.mode.zip') : t('pack.mode.disguise'),
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 4),
          Text(
            _mode == 'plain'
                ? t('pack.mode.zip.desc')
                : t('pack.mode.disguise.desc'),
            style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
          ),
          const SizedBox(height: 12),
          // 模式二选一（GUI-PLAN §5.2②）：普通压缩是默认，伪装是特殊功能
          SegmentedButton<String>(
            segments: [
              ButtonSegment(
                value: 'plain',
                icon: const Icon(Icons.folder_zip_outlined, size: 16),
                label: Text(t('pack.mode.zip.title')),
              ),
              ButtonSegment(
                value: 'disguise',
                icon: const Icon(Icons.movie_outlined, size: 16),
                label: Text(t('pack.mode.disguise.title')),
              ),
            ],
            selected: {_mode},
            onSelectionChanged: _busy
                ? null
                : (s) {
                    setState(() {
                      _mode = s.first;
                      // 输出名跟着换扩展名（普通 = .zip；伪装 = 外壳的扩展名）
                      if (_out != null) _out = reExt(_out!, _ext);
                    });
                  },
          ),
          // 归档格式（T17-4）：只有普通压缩能选 —— 伪装包只能是 zip
          if (_mode == 'plain') ...[
            const SizedBox(height: 6),
            Row(
              children: [
                Text(t('pack.format.title'),
                    style: const TextStyle(fontSize: 13)),
                const SizedBox(width: 10),
                DropdownButton<String>(
                  key: const ValueKey('pack-format'),
                  value: _format,
                  isDense: true,
                  style: const TextStyle(fontSize: 12),
                  items: [
                    DropdownMenuItem(
                        value: 'zip', child: Text(t('pack.format.zip'))),
                    DropdownMenuItem(
                        value: 'tar.gz', child: Text(t('pack.format.targz'))),
                    DropdownMenuItem(
                        value: 'tar.zst', child: Text(t('pack.format.tarzst'))),
                  ],
                  onChanged: _busy
                      ? null
                      : (v) => setState(() {
                            _format = v ?? 'zip';
                            // tar 没有分卷 / 加密：亮着的开关要跟着关掉，
                            // 别留一个「看着开着、其实不生效」的假开关
                            if (_isTar) _split = false;
                            if (_out != null) _out = reExt(_out!, _ext);
                          }),
                ),
              ],
            ),
            Padding(
              padding: const EdgeInsets.only(top: 3),
              child: Text(
                _isTar ? t('pack.format.hintTar') : t('pack.format.hintZip'),
                style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant),
              ),
            ),
          ],
          if (_mode == 'disguise') ...[
            const SizedBox(height: 4),
            SwitchListTile(
              key: const ValueKey('sfx-toggle'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _sfx,
              onChanged: _busy
                  ? null
                  : (v) => setState(() {
                        _sfx = v;
                        if (_out != null) _out = reExt(_out!, _ext);
                      }),
              secondary: const Icon(Icons.window_outlined, size: 20),
              title: Text(t('pack.mode.sfx.title'), style: const TextStyle(fontSize: 13)),
              subtitle: Text(t('pack.mode.sfx.desc'),
                  style: const TextStyle(fontSize: 11)),
            ),
            if (_sfx)
              Padding(
                padding: const EdgeInsets.only(left: 40, right: 4, bottom: 6),
                child: Row(
                  children: [
                    Text(t('pack.targets'), style: const TextStyle(fontSize: 12)),
                    DropdownButton<String>(
                      key: const ValueKey('sfx-kind'),
                      value: _sfxKind,
                      isDense: true,
                      style: const TextStyle(fontSize: 12),
                      items: const [
                        DropdownMenuItem(
                            value: 'windows', child: Text('Windows（.exe）')),
                        DropdownMenuItem(
                            value: 'linux', child: Text('Linux（.sh）')),
                      ],
                      onChanged: _busy
                          ? null
                          : (v) => setState(() {
                                _sfxKind = v ?? 'windows';
                                if (_out != null) _out = reExt(_out!, _ext);
                              }),
                    ),
                  ],
                ),
              ),
          ],
          const SizedBox(height: 8),
          Row(
            children: [
              OutlinedButton.icon(
                onPressed: _busy ? null : _addFolder,
                icon: const Icon(Icons.create_new_folder_outlined, size: 18),
                label: Text(t('pack.addDir')),
              ),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                onPressed: _busy ? null : _addFiles,
                icon: const Icon(Icons.insert_drive_file_outlined, size: 18),
                label: Text(t('pack.addFiles')),
              ),
              const Spacer(),
              if (_inputs.isNotEmpty)
                TextButton(
                  onPressed: _busy ? null : () => setState(_inputs.clear),
                  child: Text(t('pack.clear')),
                ),
            ],
          ),
          const SizedBox(height: 8),
          SizedBox(
            height:
                (MediaQuery.sizeOf(context).height * 0.3).clamp(140.0, 360.0),
            child: _inputs.isEmpty
                ? Center(
                    child: Text(
                      t('pack.empty'),
                      style: TextStyle(color: cs.onSurfaceVariant),
                    ),
                  )
                : Card(
                    clipBehavior: Clip.antiAlias,
                    child: ListView.separated(
                      itemCount: _inputs.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, i) {
                        final p = _inputs[i];
                        final isDir = Directory(p).existsSync();
                        return ListTile(
                          dense: true,
                          leading: Icon(
                            isDir ? Icons.folder_outlined : Icons.insert_drive_file_outlined,
                            size: 20,
                          ),
                          title: Text(baseName(p), style: const TextStyle(fontSize: 13)),
                          subtitle: Text(
                            isDir ? t('pack.dirEntry') : p,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(fontSize: 11),
                          ),
                          trailing: IconButton(
                            icon: const Icon(Icons.close, size: 16),
                            // T11：纯图标按钮没有 tooltip，读屏软件只能念「按钮」，
                            // 键盘用户也看不到它是什么（这条原来在 3 个裸图标之一）
                            tooltip: t('pack.removeInput'),
                            onPressed: _busy ? null : () => setState(() => _inputs.removeAt(i)),
                          ),
                        );
                      },
                    ),
                  ),
          ),
          const SizedBox(height: 12),
          // 伪装文件（外壳）：用户想选什么就选什么，没有任何类型限制
          if (_mode == 'disguise' && !_sfx) ...[
            Row(
              children: [
                Expanded(
                  child: Text(t('pack.cover'),
                      style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
                ),
                if (_coverPath != null)
                  TextButton(
                    onPressed: _busy ? null : _pickCover,
                    child: Text(t('pack.change'), style: const TextStyle(fontSize: 12)),
                  ),
              ],
            ),
            const SizedBox(height: 4),
            if (_coverPath == null)
              OutlinedButton.icon(
                key: const ValueKey('pick-cover'),
                onPressed: _busy ? null : _pickCover,
                icon: const Icon(Icons.folder_open_outlined, size: 18),
                label: Text(t('pack.cover.pick')),
                style: OutlinedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                ),
              )
            else
              Card(
                margin: EdgeInsets.zero,
                clipBehavior: Clip.antiAlias,
                child: ListTile(
                  dense: true,
                  leading: const Icon(Icons.insert_drive_file_outlined, size: 20),
                  title: Text(
                    _coverPath!.split('/').last,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 13),
                  ),
                  subtitle: Text(
                    '${_size(_coverSize)} · ${_coverExt.isEmpty ? t('pack.noExt') : '.$_coverExt'}',
                    style: const TextStyle(fontSize: 11),
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.close, size: 16),
                    tooltip: t('pack.cover.clear'),
                    onPressed: _busy
                        ? null
                        : () => setState(() {
                              _coverPath = null;
                              _coverSize = 0;
                              _coverExt = '';
                            }),
                  ),
                ),
              ),
            const SizedBox(height: 6),
            Text(
              t('pack.cover.hint'),
              style: TextStyle(fontSize: 11, color: cs.onSurfaceVariant, height: 1.5),
            ),
          ],
          const SizedBox(height: 10),
          // 显示名（T18-2）：**文件名可以直接改**。
          // 以前这里是一段只读文字，理由是「每帧 new controller 会丢状态」——
          // 把 controller 放进 State 字段就不丢了，那不是不能改的理由。
          // 目录不在这里：右边那个按钮负责选位置，一条规则只有一个出口。
          TextField(
            key: const ValueKey('output-name'),
            controller: _nameCtrl,
            enabled: !_busy,
            style: const TextStyle(fontSize: 12),
            decoration: InputDecoration(
              labelText: t('pack.output'),
              isDense: true,
              border: const OutlineInputBorder(),
              helperText: t('pack.output.nameHint'),
              errorText: _nameErrorText,
              suffixIcon: IconButton(
                icon: const Icon(Icons.folder_open, size: 18),
                // T11：同上 —— 一个孤零零的文件夹图标，不说清是「选输出位置」
                tooltip: t('pack.pickOutput'),
                onPressed: _busy ? null : _pickOut,
              ),
            ),
            onChanged: _onNameChanged,
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _pwCtrl,
            obscureText: !_showPw,
            // tar 系没有加密（引擎层不支持，见 PackFormat）—— 置灰并说明，
            // 不是让用户输了密码才发现没生效
            enabled: !_isTar,
            decoration: InputDecoration(
              // 第 23 批 P23-5：自解压也支持解压码了（存根会提示输入）
              labelText: t('pack.password'),
              helperText: _isTar ? t('pack.format.noPassword') : null,
              isDense: true,
              border: const OutlineInputBorder(),
              prefixIcon: const Icon(Icons.lock_outline, size: 18),
              suffixIcon: IconButton(
                icon: Icon(
                  _showPw ? Icons.visibility_off : Icons.visibility,
                  size: 18,
                ),
                tooltip: _showPw ? t('pack.password.hide') : t('pack.password.show'),
                onPressed: () => setState(() => _showPw = !_showPw),
              ),
            ),
          ),
          const SizedBox(height: 12),
          // 压缩级别（P23-4）：以前只在引擎里有，CLI/GUI 都没暴露
          ListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.speed, size: 18),
            title: Text(t('pack.level'), style: const TextStyle(fontSize: 13)),
            subtitle: Text(t('pack.level.hint'),
                style: const TextStyle(fontSize: 11)),
            trailing: DropdownButton<int>(
              value: _level,
              isDense: true,
              style: const TextStyle(fontSize: 12),
              items: [
                DropdownMenuItem(value: 0, child: Text(t('pack.level.store'))),
                DropdownMenuItem(value: 1, child: Text(t('pack.level.fast'))),
                DropdownMenuItem(value: 6, child: Text(t('pack.level.normal'))),
                DropdownMenuItem(value: 9, child: Text(t('pack.level.smallest'))),
              ],
              onChanged: _busy ? null : (v) => setState(() => _level = v ?? 6),
            ),
          ),
          // 数据藏在哪（E2 的 `pack.tail` / T18-1 的 L4，统一成一个下拉）：
          //   free = 藏进封面的 free 原子（默认，视频能播）
          //   tail = 封面一个字节都不改，数据接在最后（最保险）
          //   mid  = 插进视频数据中间（T18-1 · L4）—— 视频照常能播，包藏得比尾巴深
          // 三档只在这一处选：开关 + 下拉并存就是两套优先级，说不清谁赢。
          ListTile(
            key: const ValueKey('layout-select'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.merge_type, size: 20),
            title: Text(
              _canTail ? t('pack.layout.title') : t('pack.layout.append.short'),
              style: const TextStyle(fontSize: 13),
            ),
            subtitle: Text(
              !_canTail
                  ? t('pack.layout.none')
                  : _layout == 'tail'
                      ? t('pack.layout.append.desc')
                      : _layout == 'head'
                          ? t('pack.layout.head.desc')
                          : _layout == 'two'
                              ? t('pack.layout.two.desc')
                              : _layout == 'mkv'
                                  ? t('pack.layout.mkv.desc')
                                  : _layout == 'mid'
                                      ? t('pack.layout.mid.desc')
                                      : t('pack.layout.free'),
              style: const TextStyle(fontSize: 11),
            ),
            trailing: DropdownButton<String>(
              key: const ValueKey('layout-dropdown'),
              value: _layout,
              isDense: true,
              style: const TextStyle(fontSize: 12),
              onChanged: (_busy || !_canTail)
                  ? null
                  : (v) => setState(() {
                        _layout = v ?? 'free';
                        // 输出后缀跟着布局走（L8 的产物是 .mkv、别的跟外壳）：
                        // 用户自己在「输出文件」里改过名就不动他的（_outUserPicked）。
                        if (!_outUserPicked && _inputs.isNotEmpty) {
                          _out = suggestOut(_inputs.first, _ext);
                        }
                      }),
              items: [
                DropdownMenuItem(
                  value: 'free',
                  child: Text(t('pack.layout.choice.free')),
                ),
                DropdownMenuItem(
                  value: 'tail',
                  child: Text(t('pack.layout.choice.tail')),
                ),
                // 封面不是视频时这两档点不了（引擎按魔数再核一次，认不出会降级）
                DropdownMenuItem(
                  value: 'head',
                  enabled: _canMid,
                  child: Text(t('pack.layout.choice.head')),
                ),
                DropdownMenuItem(
                  value: 'mid',
                  enabled: _canMid,
                  child: Text(t('pack.layout.choice.mid')),
                ),
                // L7 不挑封面类型（只是把两个包接在封面后面），有封面就能选
                DropdownMenuItem(
                  value: 'two',
                  child: Text(t('pack.layout.choice.two')),
                ),
                // L8 要系统 ffmpeg；没有的话引擎会自动降级并在结果里说明（界面不拦）
                DropdownMenuItem(
                  value: 'mkv',
                  child: Text(t('pack.layout.choice.mkv')),
                ),
              ],
            ),
          ),
          // L7（一个文件两个包）：第二个包必须是**现成的** zip，只在这一档出现。
          // 这里不做「必须选」的强校验（打包按钮那关会拦），免得用户还在挑文件就被红字催。
          if (_canTail && _layout == 'two')
            ListTile(
              key: const ValueKey('second-package'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.library_add_outlined, size: 20),
              title: Text(
                t('pack.layout.two.pick'),
                style: const TextStyle(fontSize: 13),
              ),
              subtitle: Text(
                _secondPath == null
                    ? t('pack.layout.two.none')
                    : '${baseName(_secondPath!)} · ${_size(File(_secondPath!).lengthSync())}',
                style: const TextStyle(fontSize: 11),
                overflow: TextOverflow.ellipsis,
              ),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextButton(
                    key: const ValueKey('second-pick'),
                    onPressed: _busy ? null : _pickSecond,
                    child: Text(t('pack.layout.two.choose')),
                  ),
                  if (_secondPath != null)
                    IconButton(
                      key: const ValueKey('second-clear'),
                      tooltip: t('pack.layout.two.clear'),
                      onPressed: _busy ? null : () => setState(() => _secondPath = null),
                      icon: const Icon(Icons.close, size: 18),
                    ),
                ],
              ),
            ),
          // 防网盘扫描（E1 / 协议 pack.decoy*）：往包里撒几段乱码，
          // 让网盘/在线扫描把它当普通视频，而不是「带 ZIP 尾巴的可疑文件」。
          //
          // 为什么只给「开/关」：段数、每段大小是引擎调过的默认（4 段 × 16 KB），
          // 摆出来只会让新手纠结；真正懂行的人用命令行 --decoy N --decoy-size。
          SwitchListTile(
            key: const ValueKey('decoy-toggle'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _decoy && _canDecoy,
            onChanged: (_busy || !_canDecoy)
                ? null
                : (v) => setState(() {
                      _decoy = v;
                      // 关了总开关就把子开关一起放下，避免下次开了又莫名其妙带着假特征码
                      if (!v) _decoySignatures = false;
                    }),
            secondary: const Icon(Icons.blur_on, size: 20),
            title: Text(
              _canDecoy ? t('pack.scan') : t('pack.scan.short'),
              style: const TextStyle(fontSize: 13),
            ),
            subtitle: Text(
              t('pack.scan.desc'),
              style: const TextStyle(fontSize: 11),
            ),
          ),
          if (_decoy && _canDecoy)
            SwitchListTile(
              key: const ValueKey('decoy-sig-toggle'),
              dense: true,
              contentPadding: const EdgeInsets.only(left: 40),
              value: _decoySignatures,
              onChanged:
                  _busy ? null : (v) => setState(() => _decoySignatures = v),
              title: Text(t('pack.fingerprint'),
                  style: const TextStyle(fontSize: 12)),
              subtitle: Text(t('pack.fingerprint.desc'),
                  style: const TextStyle(fontSize: 11)),
            ),
          // 交付选项：默认「校验和 + 说明书」开（DESIGN §14.2），分卷按需
          SwitchListTile(
            key: const ValueKey('split-toggle'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _split,
            onChanged: _busy || _sfx || _isTar ? null : (v) => setState(() => _split = v),
            title: Text(
                _sfx ? t('pack.volumes') : t('pack.volumes.short'),
                style: const TextStyle(fontSize: 13)),
            subtitle: _split
                ? Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: TextField(
                      controller: _splitCtrl,
                      onChanged: (_) => setState(() {}),
                      decoration: InputDecoration(
                        labelText: t('pack.volumeSize'),
                        hintText: '2G / 500M',
                        errorText: _splitSizeError,
                        isDense: true,
                        border: const OutlineInputBorder(),
                      ),
                    ),
                  )
                : null,
          ),
          SwitchListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _checksum,
            onChanged: _busy ? null : (v) => setState(() => _checksum = v),
            title: Text(t('pack.sha'), style: const TextStyle(fontSize: 13)),
            subtitle: Text(t('pack.sha.desc'),
                style: const TextStyle(fontSize: 11)),
          ),
          // 防炸链（T18-3）：打乱条目时间戳，让网盘判不成同一个文件。
          // 默认关 —— 自己备份时时间戳有用，别默默改掉。
          SwitchListTile(
            key: const ValueKey('tweak-time-toggle'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            value: _tweakTime,
            onChanged: _busy ? null : (v) => setState(() => _tweakTime = v),
            title: Text(t('pack.tweak'),
                style: const TextStyle(fontSize: 13)),
            subtitle: Text(t('pack.tweak.desc'),
                style: const TextStyle(fontSize: 11)),
          ),
          // 说明书是给「伪装包」用的（告诉对方这不是视频）；普通压缩和自解压不需要
          if (_mode == 'disguise' && !_sfx)
            SwitchListTile(
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _readme,
              onChanged: _busy ? null : (v) => setState(() => _readme = v),
              title: Text(t('pack.notes'), style: const TextStyle(fontSize: 13)),
              subtitle: Text(t('pack.notes.desc'),
                  style: const TextStyle(fontSize: 11)),
            ),
          const SizedBox(height: 8),
          // 打包进度（N2）：有读数就画一条，打大包时能看见在动
          if (_busy)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  LinearProgressIndicator(
                    // 检验阶段不看百分比：这条进度条画的是"打包"，检验是另一件事
                    value: _flushing ? null : _progress?.fraction,
                    minHeight: 6,
                    borderRadius: BorderRadius.circular(3),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    _flushing
                        ? t('pack.flush.progress', {
                            'size': _size(_flushPending),
                            'secs': '$_flushSecs',
                          })
                        : _progress == null
                            ? t('pack.preparing')
                            : '${((_progress!.fraction ?? 0) * 100).toStringAsFixed(1)}% · '
                                '${_size(_progress!.doneBytes)} / ${_size(_progress!.totalBytes)}'
                                '${_current == null ? '' : ' · $_current'}',
                    style: TextStyle(
                      fontSize: 11,
                      color: _flushing ? cs.primary : cs.onSurfaceVariant,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ),
            ),
          if (_error != null)
            Text(
              _errorText(_error!),
              style: TextStyle(color: cs.error, fontSize: 12),
            ),
          if (_result != null) _resultCard(cs),
          const SizedBox(height: 8),
          // 键位就地提示（T11）：Enter = 开始打包（焦点不在输入框里时）
          Tooltip(
            message: 'Enter',
            child: FilledButton.icon(
              onPressed: _busy || _inputs.isEmpty ? null : _pack,
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.archive),
            label: Text(
              _busy
                  ? (_mode == 'plain' ? t('pack.working.zip') : (_sfx ? t('pack.working.sfx') : t('pack.working.disguise')))
                  : (_mode == 'plain'
                      ? t('pack.start.zip')
                      : (_sfx ? t('pack.start.sfx') : t('pack.start.disguise'))),
            ),
            style: FilledButton.styleFrom(padding: const EdgeInsets.symmetric(vertical: 14)),
          ),
          ),
          ],
        ),
      ),
      ),
      ),
    );
  }

  Widget _resultCard(ColorScheme cs) {
    final r = _result!;
    final size = (r['total_size'] as num?)?.toInt() ?? 0;
    final entries = (r['entries'] as num?)?.toInt() ?? 0;
    final layout = r['layout']?.toString() ?? '';
    final mode = r['mode']?.toString() ?? 'disguise';
    final plain = mode == 'plain';
    final cover = r['cover']?.toString();
    final coverName = cover?.split('/').last;
    final sfx = r['sfx'] == true;
    // 引擎返回的**真实落地路径**（2026-10-30 起 pack 命令带 out 字段）
    final outPath = r['out']?.toString();
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: cs.primaryContainer.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
        sfx
            ? t('pack.result.sfx', {'size': _size(size), 'entries': '$entries'})
            : plain
                ? t('pack.result.zip', {'size': _size(size), 'entries': '$entries'})
                : t('pack.result.disguise', {
                    'size': _size(size),
                    'entries': '$entries',
                    'layout': layout == 'FreeAtom'
                        ? t('pack.layout.free.name')
                        : layout == 'HeadFreeAtom'
                            ? t('pack.layout.head.name')
                            : layout == 'MkvAttachment'
                                ? t('pack.layout.mkv.name')
                                : layout == 'TwoPackages'
                                    ? t('pack.layout.two.name')
                                    : layout == 'MidChunk'
                                        ? t('pack.layout.mid.name')
                                        : t('pack.layout.append.name'),
                    'cover': coverName == null
                        ? ''
                        : t('pack.result.coverLine', {'name': coverName}),
                  }),
            style: const TextStyle(fontSize: 12, height: 1.5),
          ),
          // 用户选了「插在视频中间」但引擎做不到（封面不是标准 MP4 / 没有 chunk 表 /
          // 包太大溢出）→ 引擎会降级成别的布局。这里必须明说，
          // 否则用户以为选中的那一档生效了，实际拿到的是另一种包。
          if (_askedLayout == 'mid' && layout != 'MidChunk') ...[
            const SizedBox(height: 6),
            Text(
              t('pack.layout.mid.failed'),
              style: TextStyle(fontSize: 12, color: cs.error),
            ),
          ],
          // 同一个坑，L3 也有（封面不是 MP4 / 找不到 moov 或偏移表 / 包太大溢出）
          if (_askedLayout == 'head' && layout != 'HeadFreeAtom') ...[
            const SizedBox(height: 6),
            Text(
              t('pack.layout.head.failed'),
              style: TextStyle(fontSize: 12, color: cs.error),
            ),
          ],
          // 用户 2026-10-30 报「显示输出位置但打开没看到文件」：
          // 之前卡片只说「完成」，不告诉人文件在哪，也没有一键打开 —— 补上。
          if (outPath != null && outPath.isNotEmpty) ...[
            const SizedBox(height: 8),
            SelectableText(
              t('pack.result.path', {'path': outPath}),
              style: TextStyle(fontSize: 12, color: cs.onSurfaceVariant),
            ),
            const SizedBox(height: 4),
            Wrap(
              spacing: 4,
              children: [
                TextButton.icon(
                  key: const ValueKey('open-out-dir'),
                  onPressed: () => openDirectory(parentOf(outPath)),
                  icon: const Icon(Icons.folder_open, size: 16),
                  label: Text(t('pack.result.openFolder'), style: const TextStyle(fontSize: 12)),
                ),
                TextButton.icon(
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: outPath));
                    Notifier.send(t('pack.notify.copied'), t('pack.notify.copiedHint'));
                  },
                  icon: const Icon(Icons.copy, size: 16),
                  label: Text(t('pack.copyPath'), style: const TextStyle(fontSize: 12)),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }

  String _size(int bytes) {
    const k = 1024;
    if (bytes >= k * k * k) return '${(bytes / (k * k * k)).toStringAsFixed(2)} GB';
    if (bytes >= k * k) return '${(bytes / (k * k)).toStringAsFixed(1)} MB';
    if (bytes >= k) return '${(bytes / k).toStringAsFixed(0)} KB';
    return '$bytes B';
  }
}
