import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/widgets.dart';
import 'package:hydra_mobile/app/app_log.dart';
import 'package:hydra_mobile/app/app_settings.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/native_bridge.dart';
import 'package:hydra_mobile/src/rust/api/routes.dart' as routes_api;
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;

enum ProxyPhase { stopped, starting, connecting, connected, degraded, failed, stopping }

/// Owns the proxy lifecycle and everything the UI shows about it.
///
/// The Rust side is the source of truth: the controller polls
/// `getProxyStatus` and only remembers the user's intent ([wantRunning]) so it
/// can bring the proxy back after crashes, network changes and app resumes.
class ProxyController extends ChangeNotifier with WidgetsBindingObserver {
  ProxyController({required this.baseDir, required this.settings});

  final String baseDir;
  final AppSettings settings;

  static const _foregroundPoll = Duration(seconds: 1);
  static const _backgroundPoll = Duration(seconds: 10);
  static const _heartbeatEvery = Duration(seconds: 60);

  ProxyStatus _status = ProxyStatus.stopped;
  List<ServerInfo> _servers = const [];
  bool _wantRunning = false;
  bool _transition = false;
  String? _startError;
  bool _keepAliveActive = false;
  bool _foreground = true;

  Timer? _pollTimer;
  bool _polling = false;
  DateTime _lastHeartbeat = DateTime.fromMillisecondsSinceEpoch(0);
  StreamSubscription<NetworkInfo>? _networkSub;
  Timer? _restartTimer;
  int _restartAttempt = 0;
  SshState? _lastSshState;
  bool _disposed = false;

  ProxyStatus get status => _status;
  List<ServerInfo> get servers => _servers;
  List<ServerInfo> get sshServers => _servers.where((s) => s.isSsh).toList();
  bool get wantRunning => _wantRunning;
  bool get busy => _transition;
  bool get keepAliveActive => _keepAliveActive;
  NetworkInfo? get network => NativeBridge.instance.lastNetwork;

  ServerInfo? get activeServer {
    for (final s in _servers) {
      if (s.isSsh && s.active) return s;
    }
    return null;
  }

  /// Most relevant error for the user, newest source first.
  String? get error {
    if (_startError != null) return _startError;
    final ssh = _status.activeSsh;
    if (ssh != null && ssh.state == SshState.failed) return ssh.lastError;
    return _status.running ? null : _status.lastError;
  }

  ProxyPhase get phase {
    if (_transition) return _wantRunning ? ProxyPhase.starting : ProxyPhase.stopping;
    if (!_status.running) {
      return _wantRunning || _startError != null ? ProxyPhase.failed : ProxyPhase.stopped;
    }
    final ssh = _status.activeSsh;
    if (ssh == null) return ProxyPhase.connected;
    return switch (ssh.state) {
      SshState.connected => ProxyPhase.connected,
      SshState.connecting => ProxyPhase.connecting,
      SshState.failed => ProxyPhase.degraded,
      SshState.idle => ProxyPhase.connecting,
    };
  }

  Future<void> init() async {
    WidgetsBinding.instance.addObserver(this);
    _networkSub = NativeBridge.instance.networkChanges.listen(_onNetwork);
    await refreshServers();
    await _refreshStatus();
    _schedulePoll();
    AppLog.info(
      'controller',
      'Ready: servers=${sshServers.length} active=${activeServer?.endpoint ?? 'none'} '
          'autoStart=${settings.autoStart} keepAlive=${settings.keepAliveInBackground}',
    );
    if (_status.running) {
      _wantRunning = true;
      await _applyKeepAlive();
    } else if (settings.autoStart && activeServer != null) {
      AppLog.info('controller', 'Auto-starting proxy');
      await start();
    }
  }

  // ---------------------------------------------------------------- lifecycle

  Future<void> start() async {
    if (_transition) return;
    _wantRunning = true;
    _restartTimer?.cancel();
    _transition = true;
    _startError = null;
    _notify();
    final watch = Stopwatch()..start();
    AppLog.info('controller', 'Start requested (server=${activeServer?.endpoint ?? 'none'})');
    try {
      await simple_api.startHydraNode(baseDir: baseDir);
      _restartAttempt = 0;
      AppLog.info('controller', 'Proxy started in ${watch.elapsedMilliseconds} ms');
    } catch (e, st) {
      _startError = describeError(e);
      AppLog.error('controller', 'Proxy start failed after ${watch.elapsedMilliseconds} ms', _startError, st);
      _scheduleRestart();
    } finally {
      _transition = false;
      await _refreshStatus();
      await _applyKeepAlive();
      _notify();
    }
  }

  Future<void> stop() async {
    if (_transition) return;
    _wantRunning = false;
    _restartTimer?.cancel();
    _restartAttempt = 0;
    _transition = true;
    _startError = null;
    _notify();
    AppLog.info('controller', 'Stop requested');
    try {
      await simple_api.stopHydraNode();
    } catch (e, st) {
      AppLog.error('controller', 'Proxy stop failed', describeError(e), st);
    } finally {
      _transition = false;
      await _refreshStatus();
      await _applyKeepAlive();
      _notify();
    }
  }

  Future<void> reconnect(String reason) async {
    if (!_status.running) return;
    AppLog.info('controller', 'Reconnecting SSH: $reason');
    try {
      await simple_api.reconnectTransports(reason: reason);
    } catch (e, st) {
      AppLog.error('controller', 'Reconnect failed', describeError(e), st);
    }
    await _refreshStatus();
  }

  void _scheduleRestart() {
    if (!_wantRunning || _disposed) return;
    _restartTimer?.cancel();
    final seconds = math.min(60, 2 << math.min(_restartAttempt, 5));
    _restartAttempt++;
    AppLog.warn('controller', 'Will retry starting the proxy in ${seconds}s (attempt $_restartAttempt)');
    _restartTimer = Timer(Duration(seconds: seconds), () {
      if (_wantRunning && !_status.running) unawaited(start());
    });
  }

  Future<void> _applyKeepAlive() async {
    final want = _wantRunning && _status.running && settings.keepAliveInBackground;
    if (want == _keepAliveActive) return;
    _keepAliveActive = await NativeBridge.instance.setKeepAlive(want);
    AppLog.info('controller', 'Background keep-alive ${_keepAliveActive ? 'on' : 'off'}');
  }

  Future<void> setKeepAliveInBackground(bool value) async {
    await settings.setKeepAliveInBackground(value);
    await _applyKeepAlive();
    _notify();
  }

  Future<void> setAutoStart(bool value) async {
    await settings.setAutoStart(value);
    _notify();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    AppLog.info('lifecycle', 'App ${state.name} (proxy ${_status.running ? 'running' : 'stopped'})');
    final foreground = state == AppLifecycleState.resumed;
    if (foreground == _foreground) return;
    _foreground = foreground;
    _schedulePoll();
    if (foreground) unawaited(_onResume());
  }

  Future<void> _onResume() async {
    await _refreshStatus();
    if (_wantRunning && !_status.running && !_transition) {
      AppLog.warn('controller', 'Proxy is not running after resume; restarting');
      await start();
    } else if (_status.activeSsh?.state == SshState.failed) {
      await reconnect('app resumed with failed SSH session');
    }
  }

  void _onNetwork(NetworkInfo info) {
    if (!info.changed || !_status.running) return;
    if (info.online) {
      unawaited(reconnect('network changed to $info'));
    } else {
      AppLog.warn('controller', 'Network lost: $info');
    }
  }

  // ------------------------------------------------------------------ polling

  void _schedulePoll() {
    _pollTimer?.cancel();
    _pollTimer = Timer.periodic(_foreground ? _foregroundPoll : _backgroundPoll, (_) => _refreshStatus());
  }

  Future<void> _refreshStatus() async {
    if (_polling || _disposed) return;
    _polling = true;
    try {
      final next = ProxyStatus.fromJson(await simple_api.getProxyStatus());
      _observe(_status, next);
      _status = next;
      _notify();
    } catch (e, st) {
      AppLog.error('controller', 'Status poll failed', describeError(e), st);
    } finally {
      _polling = false;
    }
  }

  void _observe(ProxyStatus previous, ProxyStatus next) {
    if (previous.running && !next.running && _wantRunning && !_transition) {
      AppLog.error('controller', 'Proxy stopped unexpectedly: ${next.lastError ?? 'no error reported'}');
      _scheduleRestart();
    }
    final ssh = next.activeSsh;
    if (ssh != null && ssh.state != _lastSshState) {
      AppLog.info(
        'controller',
        'SSH ${_lastSshState?.name ?? 'unknown'} -> ${ssh.state.name} (${ssh.endpoint})'
            '${ssh.lastError != null && ssh.state == SshState.failed ? ': ${ssh.lastError}' : ''}',
      );
      _lastSshState = ssh.state;
    }
    final now = DateTime.now();
    if (now.difference(_lastHeartbeat) >= _heartbeatEvery) {
      _lastHeartbeat = now;
      AppLog.info(
        'heartbeat',
        'running=${next.running} want=$_wantRunning fg=$_foreground keepAlive=$_keepAliveActive '
            'conns=${next.connectionsActive}/${next.connectionsTotal} '
            'ssh=${ssh?.state.name ?? '-'} up=${ssh?.bytesUp ?? 0} down=${ssh?.bytesDown ?? 0} '
            'net=${network ?? 'unknown'}',
      );
    }
  }

  // ------------------------------------------------------------------ servers

  Future<void> refreshServers() async {
    try {
      _servers = ServerInfo.listFromJson(await routes_api.listServers());
      _notify();
    } catch (e, st) {
      AppLog.error('controller', 'Could not load servers', describeError(e), st);
    }
  }

  /// Throws with a user-readable message on validation errors.
  Future<ServerInfo> saveServer({
    String? id,
    required String label,
    required String host,
    required int port,
    required String username,
    required String authType,
    String? credential,
    bool activate = true,
  }) async {
    final raw = await routes_api.saveSshServer(
      id: id,
      label: label,
      host: host,
      port: port,
      username: username,
      authType: authType,
      credential: credential,
      activate: activate,
    );
    await refreshServers();
    await _refreshStatus();
    return ServerInfo.fromJson(ServerInfo.decodeObject(raw));
  }

  Future<void> activate(ServerInfo server) async {
    await routes_api.setActiveServer(id: server.id);
    await refreshServers();
    if (!_status.running && settings.autoStart) await start();
  }

  Future<void> deleteServer(ServerInfo server) async {
    await routes_api.deleteServer(id: server.id);
    await refreshServers();
  }

  Future<void> forgetHostKey(ServerInfo server) async {
    await routes_api.forgetServerHostKey(id: server.id);
    await refreshServers();
  }

  Future<SshTestResult> testServer({
    String? id,
    required String host,
    required int port,
    required String username,
    required String authType,
    String? credential,
  }) async {
    AppLog.info('controller', 'Testing $username@$host:$port ($authType)');
    final result = SshTestResult.fromJson(await routes_api.testSshServer(
      id: id,
      host: host,
      port: port,
      username: username,
      authType: authType,
      credential: credential,
    ));
    AppLog.info('controller', 'Test ${result.ok ? 'passed' : 'failed'} in ${result.elapsedMs} ms${result.error != null ? ': ${result.error}' : ''}');
    return result;
  }

  SshKeyInfo generateKey(String comment) => SshKeyInfo.fromJson(routes_api.generateSshKey(comment: comment));

  SshKeyInfo describeKey(String privateKey) => SshKeyInfo.fromJson(routes_api.describeSshKey(privateKey: privateKey));

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    WidgetsBinding.instance.removeObserver(this);
    _pollTimer?.cancel();
    _restartTimer?.cancel();
    _networkSub?.cancel();
    super.dispose();
  }
}
