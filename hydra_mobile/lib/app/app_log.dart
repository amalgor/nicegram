import 'package:flutter/foundation.dart';
import 'package:hydra_mobile/logging/log_store.dart';
import 'package:hydra_mobile/src/rust/api/diagnostics.dart' as diagnostics_api;

/// Dart-side logging. Lines go into the Rust log pipeline (live view + log
/// files) so app lifecycle, UI actions and Flutter errors share one timeline
/// with the proxy runtime. Before Rust is loaded, lines are shown in the live
/// view and replayed into the files once [attachRust] is called.
class AppLog {
  AppLog._();

  static bool _rustReady = false;
  static final List<(String, String, String)> _pending = [];

  static void attachRust() {
    if (_rustReady) return;
    _rustReady = true;
    for (final (level, target, message) in _pending) {
      _send(level, target, '(before Rust init) $message');
    }
    _pending.clear();
  }

  static void debug(String target, String message) => _log('DEBUG', target, message);
  static void info(String target, String message) => _log('INFO', target, message);
  static void warn(String target, String message) => _log('WARN', target, message);

  static void error(String target, String message, [Object? error, StackTrace? stack]) {
    final buffer = StringBuffer(message);
    if (error != null) buffer.write(': $error');
    if (stack != null) {
      final frames = stack.toString().trim().split('\n').take(12).join(' | ');
      buffer.write(' stack=[$frames]');
    }
    _log('ERROR', target, buffer.toString());
  }

  static void _log(String level, String target, String message) {
    if (kDebugMode) debugPrint('[$level] dart::$target: $message');
    if (_rustReady) {
      _send(level, target, message);
    } else {
      _pending.add((level, target, message));
      LogStore.instance.add('[$level] dart::$target: $message');
    }
  }

  static void _send(String level, String target, String message) {
    try {
      diagnostics_api.logMessage(level: level, target: target, message: message);
    } catch (e) {
      debugPrint('AppLog: Rust logMessage failed: $e');
    }
  }
}
