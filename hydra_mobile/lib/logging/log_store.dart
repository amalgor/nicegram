import 'dart:async';
import 'dart:collection';

import 'package:flutter/foundation.dart';

/// Severity of a log line, ordered from most to least important.
///
/// `index` doubles as the verbosity rank: a UI "minimum level" of [info] shows
/// [error], [warn] and [info] but hides [debug] / [trace].
enum LogLevel { error, warn, info, debug, trace, other }

extension LogLevelMeta on LogLevel {
  String get tag {
    switch (this) {
      case LogLevel.error:
        return 'ERROR';
      case LogLevel.warn:
        return 'WARN';
      case LogLevel.info:
        return 'INFO';
      case LogLevel.debug:
        return 'DEBUG';
      case LogLevel.trace:
        return 'TRACE';
      case LogLevel.other:
        return 'OTHER';
    }
  }
}

/// A single parsed log line. Parsing happens once, at ingestion, so the UI never
/// re-parses while scrolling.
@immutable
class LogRecord {
  const LogRecord({
    this.time,
    required this.level,
    required this.target,
    required this.message,
    required this.raw,
  });

  /// `HH:MM:SS.mmm` local time stamped by the Rust logger, when present.
  final String? time;

  final LogLevel level;

  /// Originating Rust module/target, e.g. `hydra_core::transport::ssh`.
  final String target;

  /// Message body without the `[LEVEL] target:` prefix.
  final String message;

  /// Original line exactly as received (used for copy + search).
  final String raw;

  /// True when this line came from the SSH transport.
  bool get isSsh => target.contains('transport::ssh') || raw.contains('ssh_event');

  static final _timePrefix = RegExp(r'^(\d\d:\d\d:\d\d\.\d{3}) ');

  /// Rust lines arrive as `"HH:MM:SS.mmm [LEVEL] target: message"` (see
  /// `rust/src/logging.rs`); the time is optional for older lines.
  /// Anything that doesn't match is kept verbatim at [LogLevel.other].
  factory LogRecord.parse(String raw) {
    LogLevel level = LogLevel.other;
    String? time;
    var line = raw;
    final stamp = _timePrefix.firstMatch(line);
    if (stamp != null) {
      time = stamp.group(1);
      line = line.substring(stamp.end);
    }
    var rest = line;

    if (line.startsWith('[')) {
      final close = line.indexOf(']');
      if (close > 0) {
        level = _levelFromTag(line.substring(1, close));
        rest = line.substring(close + 1).trimLeft();
      }
    }

    var target = '';
    var message = rest;
    final colon = rest.indexOf(': ');
    if (colon > 0 && !rest.substring(0, colon).contains(' ')) {
      target = rest.substring(0, colon);
      message = rest.substring(colon + 2);
    }

    return LogRecord(time: time, level: level, target: target, message: message, raw: raw);
  }

  static LogLevel _levelFromTag(String tag) {
    switch (tag.toUpperCase()) {
      case 'ERROR':
      case 'PANIC':
        return LogLevel.error;
      case 'WARN':
        return LogLevel.warn;
      case 'INFO':
        return LogLevel.info;
      case 'DEBUG':
        return LogLevel.debug;
      case 'TRACE':
        return LogLevel.trace;
      default:
        return LogLevel.other;
    }
  }
}

/// Process-wide, in-memory log store fed by the Rust log stream.
///
/// Backs the Logs screen. Persistence is handled on the Rust side (rotating
/// files in the app's log directory); this keeps only the newest [maxRecords].
class LogStore extends ChangeNotifier {
  LogStore._();
  static final LogStore instance = LogStore._();

  static const maxRecords = 20000;

  final ListQueue<LogRecord> _records = ListQueue<LogRecord>();

  /// Monotonic counter of total lines ever received.
  int _totalReceived = 0;

  StreamSubscription<String>? _subscription;
  bool _bound = false;

  int get length => _records.length;
  LogRecord at(int index) => _records.elementAt(index);
  Iterable<LogRecord> get records => _records;
  int get totalReceived => _totalReceived;

  Timer? _notifyTimer;

  /// Subscribe to the Rust log stream exactly once. Safe to call repeatedly.
  void bind(Stream<String> Function() openStream) {
    if (_bound) return;
    _bound = true;
    _subscription = openStream().listen(
      add,
      onError: (Object error) =>
          add('[ERROR] hydra_mobile::logging: log stream error: $error'),
    );
  }

  /// Backfill recent history captured before the UI subscribed.
  void seed(Iterable<String> lines) {
    for (final line in lines) {
      _ingest(line);
    }
    notifyListeners();
  }

  /// Notifications are coalesced so a burst of lines costs one rebuild.
  void add(String line) {
    _ingest(line);
    _notifyTimer ??= Timer(const Duration(milliseconds: 150), () {
      _notifyTimer = null;
      notifyListeners();
    });
  }

  void _ingest(String line) {
    _records.add(LogRecord.parse(line));
    if (_records.length > maxRecords) _records.removeFirst();
    _totalReceived++;
  }

  void clear() {
    _records.clear();
    notifyListeners();
  }

  @override
  void dispose() {
    _notifyTimer?.cancel();
    _subscription?.cancel();
    super.dispose();
  }
}
