import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/app_log.dart';

class NetworkInfo {
  const NetworkInfo({
    required this.status,
    required this.interfaces,
    required this.expensive,
    required this.changed,
  });

  final String status;
  final List<String> interfaces;
  final bool expensive;

  /// False for the first report after launch.
  final bool changed;

  bool get online => status == 'satisfied';

  @override
  String toString() => '$status [${interfaces.join(',')}]${expensive ? ' expensive' : ''}';
}

/// iOS services implemented in `ios/Runner/AppDelegate.swift`
/// (channel `hydra/native`). Every call degrades gracefully elsewhere.
class NativeBridge {
  NativeBridge._() {
    _channel.setMethodCallHandler(_onCall);
  }

  static final NativeBridge instance = NativeBridge._();
  static const _channel = MethodChannel('hydra/native');

  final _network = StreamController<NetworkInfo>.broadcast();
  NetworkInfo? _lastNetwork;

  Stream<NetworkInfo> get networkChanges => _network.stream;
  NetworkInfo? get lastNetwork => _lastNetwork;
  bool get isSupported => Platform.isIOS;

  Future<dynamic> _onCall(MethodCall call) async {
    final args = (call.arguments as Map?)?.cast<String, Object?>() ?? const {};
    switch (call.method) {
      case 'log':
        final level = args['level'] as String? ?? 'INFO';
        final message = args['message'] as String? ?? '';
        switch (level) {
          case 'ERROR':
            AppLog.error('ios', message);
          case 'WARN':
            AppLog.warn('ios', message);
          default:
            AppLog.info('ios', message);
        }
      case 'networkChanged':
        final info = NetworkInfo(
          status: args['status'] as String? ?? 'unknown',
          interfaces: (args['interfaces'] as List?)?.cast<String>() ?? const [],
          expensive: args['expensive'] as bool? ?? false,
          changed: args['changed'] as bool? ?? false,
        );
        _lastNetwork = info;
        AppLog.info('network', 'Network path: $info${info.changed ? ' (changed)' : ''}');
        _network.add(info);
    }
    return null;
  }

  Future<T?> _invoke<T>(String method, [Map<String, Object?>? args]) async {
    if (!isSupported) return null;
    try {
      return await _channel.invokeMethod<T>(method, args);
    } on MissingPluginException {
      return null;
    } on PlatformException catch (e) {
      AppLog.warn('native', '$method failed: ${e.code} ${e.message}');
      return null;
    }
  }

  /// Returns whether background keep-alive is now running.
  Future<bool> setKeepAlive(bool enabled) async =>
      await _invoke<bool>('setKeepAlive', {'enabled': enabled}) ?? false;

  Future<bool> keepAliveState() async => await _invoke<bool>('keepAliveState') ?? false;

  Future<bool> shareFiles(List<String> paths, {String? text}) async =>
      await _invoke<bool>('shareFiles', {'paths': paths, 'text': text}) ?? false;

  Future<bool> openUrl(String url) async => await _invoke<bool>('openUrl', {'url': url}) ?? false;

  Future<Map<String, Object?>> deviceInfo() async {
    final info = await _invoke<Map>('deviceInfo');
    return info?.cast<String, Object?>() ??
        {'system': '${Platform.operatingSystem} ${Platform.operatingSystemVersion}'};
  }
}
