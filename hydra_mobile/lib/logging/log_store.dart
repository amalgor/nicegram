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
    required this.level,
    required this.target,
    required this.message,
    required this.raw,
  });

  final LogLevel level;

  /// Originating Rust module/target, e.g. `hydra_core::transport::ssh`.
  final String target;

  /// Message body without the `[LEVEL] target:` prefix.
  final String message;

  /// Original line exactly as received (used for copy + search).
  final String raw;

  /// True when this line came from the SSH transport.
  bool get isSsh => target.contains('transport::ssh') || raw.contains('ssh_event');

  /// Rust lines arrive as `"[LEVEL] target: message"` (see api::telemetry).
  /// Anything that doesn't match is kept verbatim at [LogLevel.other].
  factory LogRecord.parse(String line) {
    LogLevel level = LogLevel.other;
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

    return LogRecord(level: level, target: target, message: message, raw: line);
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
/// Single source of truth for the Logs screen and for the activity indicators.
/// Intentionally has no disk persistence: logs live only for the app session.
class LogStore extends ChangeNotifier {
  LogStore._();
  static final LogStore instance = LogStore._();

  final List<LogRecord> _records = <LogRecord>[];

  /// Monotonic counter of total lines ever received (used by activity indicators
  /// to detect "new activity" without diffing the list).
  int _totalReceived = 0;

  /// Timestamp of the most recent SSH event (for tunnel up/down heuristics).
  DateTime? _lastSshEventAt;
  bool _sshConnected = false;

  StreamSubscription<String>? _subscription;
  bool _bound = false;

  UnmodifiableListView<LogRecord> get records =>
      UnmodifiableListView<LogRecord>(_records);
  int get totalReceived => _totalReceived;
  DateTime? get lastSshEventAt => _lastSshEventAt;
  bool get sshConnected => _sshConnected;

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

  void add(String line) {
    _ingest(line);
    notifyListeners();
  }

  void _ingest(String line) {
    final record = LogRecord.parse(line);
    _records.add(record);
    _totalReceived++;
    if (record.isSsh) {
      _lastSshEventAt = DateTime.now();
      _updateSshState(record);
    }
  }

  void _updateSshState(LogRecord record) {
    final raw = record.raw;
    if (raw.contains('authenticated') || raw.contains('channel_open')) {
      _sshConnected = true;
    } else if (raw.contains('connect_failed') ||
        raw.contains('connect_timeout') ||
        raw.contains('auth_failed') ||
        raw.contains('auth_error')) {
      _sshConnected = false;
    }
  }

  void clear() {
    _records.clear();
    notifyListeners();
  }

  @override
  void dispose() {
    _subscription?.cancel();
    super.dispose();
  }
}
