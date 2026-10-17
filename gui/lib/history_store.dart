import 'dart:convert';
import 'dart:io';

/// 一条处理记录（本地保存，不上传）
class HistoryEntry {
  final String time;
  final String input;
  final String? dest;
  final bool ok;
  final int bytes;

  HistoryEntry({
    required this.time,
    required this.input,
    required this.dest,
    required this.ok,
    required this.bytes,
  });

  Map<String, dynamic> toJson() => {
        'time': time,
        'input': input,
        'dest': dest,
        'ok': ok,
        'bytes': bytes,
      };

  factory HistoryEntry.fromJson(Map<String, dynamic> j) => HistoryEntry(
        time: j['time'] as String? ?? '',
        input: j['input'] as String? ?? '',
        dest: j['dest'] as String?,
        ok: j['ok'] == true,
        bytes: (j['bytes'] as num?)?.toInt() ?? 0,
      );

  String get fileName {
    final parts = input.split('/');
    return parts.isEmpty ? input : parts.last;
  }
}

/// 历史记录存储：`~/.local/state/yshjy/gui-history.json`
///
/// 为什么放 state 而不是 config：这是**运行产物**，不是用户的设置；
/// 按 XDG 规范属于 state（和崩溃日志同一个目录）。
/// 上限 200 条，超了从头砍 —— 纯本地文件，不联网。
class HistoryStore {
  static const int maxEntries = 200;

  static File get _file {
    final home = Platform.environment['HOME'] ?? '.';
    final state = Platform.environment['XDG_STATE_HOME'] ?? '$home/.local/state';
    return File('$state/yshjy/gui-history.json');
  }

  static Future<List<HistoryEntry>> load() async {
    try {
      if (!await _file.exists()) return [];
      final txt = await _file.readAsString();
      if (txt.trim().isEmpty) return [];
      final list = jsonDecode(txt) as List;
      return list
          .map((e) => HistoryEntry.fromJson((e as Map).cast<String, dynamic>()))
          .toList();
    } catch (_) {
      // 文件坏了就当没有历史 —— 绝不能因为"历史读不出来"让应用打不开
      return [];
    }
  }

  static Future<void> add(HistoryEntry e) async {
    try {
      final list = await load();
      list.insert(0, e);
      if (list.length > maxEntries) {
        list.removeRange(maxEntries, list.length);
      }
      await _file.parent.create(recursive: true);
      await _file.writeAsString(jsonEncode(list.map((x) => x.toJson()).toList()));
    } catch (_) {}
  }

  static Future<void> clear() async {
    try {
      if (await _file.exists()) await _file.delete();
    } catch (_) {}
  }
}
