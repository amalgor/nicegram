import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:hydra_mobile/src/rust/api/quota.dart' as quota_api;
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;
import 'package:hydra_mobile/src/rust/api/telemetry.dart' as telemetry_api;
import 'package:path_provider/path_provider.dart';

abstract class HydraPlatformGateway {
  static HydraPlatformGateway? _instance;

  static HydraPlatformGateway get instance {
    _instance ??= Platform.isIOS
        ? _IosHydraPlatformGateway()
        : _AndroidHydraPlatformGateway();
    return _instance!;
  }

  Future<void> initialize();
  Future<String> resolveBaseDir();
  Future<void> bindLogs(void Function(String log) onLog);
  Future<void> startNetworkRuntime({required String baseDir});
  Future<bool> startVpn();
  Future<void> stopVpn();
  Future<bool> getVpnActive();
  Future<String> getActiveConnections();
  Future<String> getConnectionStats();
  Future<String> getQuotaStatus();
  Future<void> setProxyMode({required String mode});
  Future<void> setConnectionProxy({
    required BigInt connId,
    required bool proxied,
  });
  void bindVpnFdHandler(Future<void> Function(int fd) handler);
}

class _AndroidHydraPlatformGateway implements HydraPlatformGateway {
  static const MethodChannel _channel = MethodChannel('com.hydra.network/vpn');
  Future<void> Function(int fd)? _vpnFdHandler;
  bool _vpnCallbackBound = false;
  String? _baseDir;

  @override
  Future<void> initialize() async {
    await _ensureVpnCallbackBound();
  }

  @override
  Future<String> resolveBaseDir() async {
    if (_baseDir != null) {
      return _baseDir!;
    }
    final dir = await getApplicationDocumentsDirectory();
    _baseDir = dir.path;
    return _baseDir!;
  }

  @override
  Future<void> bindLogs(void Function(String log) onLog) async {
    final stream = telemetry_api.createLogStream();
    await for (final log in stream) {
      onLog(log);
    }
  }

  @override
  Future<void> startNetworkRuntime({required String baseDir}) =>
      simple_api.startHydraNode(baseDir: baseDir);

  @override
  Future<bool> startVpn() async =>
      (await _channel.invokeMethod<bool>('startVpn')) ?? false;

  @override
  Future<void> stopVpn() => _channel.invokeMethod<void>('stopVpn');

  @override
  Future<bool> getVpnActive() async {
    final fd = await _channel.invokeMethod<int>('getVpnFd');
    return (fd ?? -1) >= 0;
  }

  @override
  Future<String> getActiveConnections() => simple_api.getActiveConnections();

  @override
  Future<String> getConnectionStats() => simple_api.getConnectionStats();

  @override
  Future<String> getQuotaStatus() => quota_api.getQuotaStatus();

  @override
  Future<void> setProxyMode({required String mode}) =>
      simple_api.setProxyMode(mode: mode);

  @override
  Future<void> setConnectionProxy({
    required BigInt connId,
    required bool proxied,
  }) => simple_api.setConnectionProxy(connId: connId, proxied: proxied);

  @override
  void bindVpnFdHandler(Future<void> Function(int fd) handler) {
    _vpnFdHandler = handler;
  }

  Future<void> _ensureVpnCallbackBound() async {
    if (_vpnCallbackBound) {
      return;
    }
    _vpnCallbackBound = true;
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'onVpnStarted' && _vpnFdHandler != null) {
        final fd = call.arguments as int? ?? -1;
        if (fd >= 0) {
          await _vpnFdHandler!(fd);
        }
      }
    });
  }
}

class _IosHydraPlatformGateway implements HydraPlatformGateway {
  static const MethodChannel _channel = MethodChannel('com.hydra.network/vpn');

  String? _baseDir;
  Timer? _logPollTimer;
  int _lastLogCount = 0;

  @override
  Future<void> initialize() async {
    await resolveBaseDir();
  }

  @override
  Future<String> resolveBaseDir() async {
    if (_baseDir != null) {
      return _baseDir!;
    }
    final path = await _channel.invokeMethod<String>('getSharedBaseDir');
    if (path == null || path.isEmpty) {
      throw PlatformException(
        code: 'missing_shared_base_dir',
        message: 'Shared App Group container is not available.',
      );
    }
    _baseDir = path;
    return _baseDir!;
  }

  @override
  Future<void> bindLogs(void Function(String log) onLog) async {
    await resolveBaseDir();
    _logPollTimer?.cancel();
    _logPollTimer = Timer.periodic(const Duration(seconds: 1), (_) async {
      final lines = await _readLogs();
      if (lines.isEmpty) {
        _lastLogCount = 0;
        return;
      }

      if (_lastLogCount > lines.length) {
        _lastLogCount = 0;
      }

      for (final log in lines.skip(_lastLogCount)) {
        onLog(log);
      }
      _lastLogCount = lines.length;
    });
  }

  @override
  Future<void> startNetworkRuntime({required String baseDir}) async {}

  @override
  Future<bool> startVpn() async =>
      (await _channel.invokeMethod<bool>('startVpn')) ?? false;

  @override
  Future<void> stopVpn() => _channel.invokeMethod<void>('stopVpn');

  @override
  Future<bool> getVpnActive() async =>
      (await _channel.invokeMethod<bool>('getVpnActive')) ?? false;

  @override
  Future<String> getActiveConnections() =>
      _readSharedJsonFile('active_connections.json', '[]');

  @override
  Future<String> getConnectionStats() => _readSharedJsonFile(
    'connection_stats.json',
    jsonEncode({
      'active_count': 0,
      'total_count': 0,
      'proxied_count': 0,
      'total_bytes_up': 0,
      'total_bytes_down': 0,
    }),
  );

  @override
  Future<String> getQuotaStatus() => _readSharedJsonFile(
    'quota_status.json',
    jsonEncode({'used': 0, 'limit': 0, 'remaining': 0, 'resets_at': ''}),
  );

  @override
  Future<void> setProxyMode({required String mode}) =>
      _sendControlCommand(jsonEncode({'type': 'set_proxy_mode', 'mode': mode}));

  @override
  Future<void> setConnectionProxy({
    required BigInt connId,
    required bool proxied,
  }) => _sendControlCommand(
    jsonEncode({
      'type': 'set_connection_proxy',
      'conn_id': connId.toInt(),
      'proxied': proxied,
    }),
  );

  @override
  void bindVpnFdHandler(Future<void> Function(int fd) handler) {}

  Future<void> _sendControlCommand(String json) async {
    await _channel.invokeMethod<void>('sendControlCommand', json);
  }

  Future<String> _readSharedJsonFile(String fileName, String fallback) async {
    final baseDir = await resolveBaseDir();
    final file = File('$baseDir/$fileName');
    if (!await file.exists()) {
      return fallback;
    }
    final contents = await file.readAsString();
    if (contents.trim().isEmpty) {
      return fallback;
    }
    return contents;
  }

  Future<List<String>> _readLogs() async {
    final contents = await _readSharedJsonFile('logs.json', '[]');
    final parsed = jsonDecode(contents);
    if (parsed is! List<dynamic>) {
      return const [];
    }
    return parsed.map((item) => item.toString()).toList(growable: false);
  }
}
