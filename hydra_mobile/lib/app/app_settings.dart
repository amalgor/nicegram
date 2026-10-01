import 'dart:convert';
import 'dart:io';

import 'package:hydra_mobile/app/app_log.dart';

/// Small JSON-backed UI preferences (secrets never go here).
class AppSettings {
  AppSettings._(this._file, this._values);

  static const _defaults = <String, Object>{
    'keepAliveInBackground': true,
    'autoStart': true,
  };

  final File _file;
  final Map<String, Object?> _values;

  static Future<AppSettings> load(String dir) async {
    final file = File('$dir/app_settings.json');
    var values = <String, Object?>{};
    try {
      if (await file.exists()) {
        values = (jsonDecode(await file.readAsString()) as Map).cast<String, Object?>();
      }
    } catch (e) {
      AppLog.warn('settings', 'Ignoring unreadable ${file.path}: $e');
    }
    return AppSettings._(file, values);
  }

  bool _bool(String key) => _values[key] as bool? ?? _defaults[key]! as bool;

  /// Keep the app (and the proxy) running when another app is in front.
  bool get keepAliveInBackground => _bool('keepAliveInBackground');

  /// Start the proxy on launch when a server is configured.
  bool get autoStart => _bool('autoStart');

  Future<void> setKeepAliveInBackground(bool value) => _set('keepAliveInBackground', value);
  Future<void> setAutoStart(bool value) => _set('autoStart', value);

  Future<void> _set(String key, Object value) async {
    _values[key] = value;
    AppLog.info('settings', '$key = $value');
    try {
      await _file.writeAsString(jsonEncode(_values));
    } catch (e) {
      AppLog.error('settings', 'Could not save settings', e);
    }
  }
}
