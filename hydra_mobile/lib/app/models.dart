import 'dart:convert';

/// Typed views over the JSON returned by the Rust API.

enum SshState { idle, connecting, connected, failed }

SshState _sshState(Object? value) => switch (value) {
      'connecting' => SshState.connecting,
      'connected' => SshState.connected,
      'failed' => SshState.failed,
      _ => SshState.idle,
    };

class SshStatus {
  const SshStatus({
    required this.endpoint,
    required this.state,
    this.lastError,
    this.hostKeyFingerprint,
    this.connectedSince,
    required this.channelsOpen,
    required this.channelsTotal,
    required this.channelsFailed,
    required this.bytesUp,
    required this.bytesDown,
  });

  factory SshStatus.fromJson(Map<String, Object?> json) => SshStatus(
        endpoint: json['endpoint'] as String? ?? '',
        state: _sshState(json['state']),
        lastError: json['last_error'] as String?,
        hostKeyFingerprint: json['host_key_fingerprint'] as String?,
        connectedSince: _time(json['connected_since_ms']),
        channelsOpen: _int(json['channels_open']),
        channelsTotal: _int(json['channels_total']),
        channelsFailed: _int(json['channels_failed']),
        bytesUp: _int(json['bytes_up']),
        bytesDown: _int(json['bytes_down']),
      );

  final String endpoint;
  final SshState state;
  final String? lastError;
  final String? hostKeyFingerprint;
  final DateTime? connectedSince;
  final int channelsOpen;
  final int channelsTotal;
  final int channelsFailed;
  final int bytesUp;
  final int bytesDown;
}

class ProxyStatus {
  const ProxyStatus({
    required this.running,
    required this.port,
    this.listen,
    this.startedAt,
    this.lastError,
    required this.connectionsActive,
    required this.connectionsTotal,
    required this.routeKinds,
    required this.ssh,
  });

  factory ProxyStatus.fromJson(String raw) {
    final json = (jsonDecode(raw) as Map).cast<String, Object?>();
    return ProxyStatus(
      running: json['running'] as bool? ?? false,
      port: _int(json['port'], 1080),
      listen: json['listen'] as String?,
      startedAt: _time(json['started_at_ms']),
      lastError: json['last_error'] as String?,
      connectionsActive: _int(json['connections_active']),
      connectionsTotal: _int(json['connections_total']),
      routeKinds: ((json['routes'] as List?) ?? const [])
          .map((r) => (r as Map)['kind'] as String? ?? '')
          .toList(),
      ssh: ((json['ssh'] as List?) ?? const [])
          .map((s) => SshStatus.fromJson((s as Map).cast<String, Object?>()))
          .toList(),
    );
  }

  static const stopped = ProxyStatus(
    running: false,
    port: 1080,
    connectionsActive: 0,
    connectionsTotal: 0,
    routeKinds: [],
    ssh: [],
  );

  final bool running;
  final int port;
  final String? listen;
  final DateTime? startedAt;
  final String? lastError;
  final int connectionsActive;
  final int connectionsTotal;
  final List<String> routeKinds;
  final List<SshStatus> ssh;

  String get address => listen ?? '127.0.0.1:$port';

  /// The SSH session behind the active route, if that route is SSH.
  SshStatus? get activeSsh => ssh.isEmpty ? null : ssh.last;
}

class ServerInfo {
  const ServerInfo({
    required this.id,
    required this.label,
    required this.kind,
    required this.active,
    required this.builtin,
    this.host,
    this.port,
    this.username,
    this.authType,
    required this.hasCredential,
    this.publicKey,
    this.keyFingerprint,
    this.pinnedHostKey,
  });

  factory ServerInfo.fromJson(Map<String, Object?> json) => ServerInfo(
        id: json['id'] as String,
        label: json['label'] as String? ?? '',
        kind: json['kind'] as String? ?? '',
        active: json['active'] as bool? ?? false,
        builtin: json['builtin'] as bool? ?? false,
        host: json['host'] as String?,
        port: json['port'] as int?,
        username: json['username'] as String?,
        authType: json['auth_type'] as String?,
        hasCredential: json['has_credential'] as bool? ?? false,
        publicKey: json['public_key'] as String?,
        keyFingerprint: json['key_fingerprint'] as String?,
        pinnedHostKey: json['pinned_host_key'] as String?,
      );

  static Map<String, Object?> decodeObject(String raw) => (jsonDecode(raw) as Map).cast<String, Object?>();

  static List<ServerInfo> listFromJson(String raw) => ((jsonDecode(raw) as List?) ?? const [])
      .map((s) => ServerInfo.fromJson((s as Map).cast<String, Object?>()))
      .toList();

  final String id;
  final String label;
  final String kind;
  final bool active;
  final bool builtin;
  final String? host;
  final int? port;
  final String? username;
  final String? authType;
  final bool hasCredential;
  final String? publicKey;
  final String? keyFingerprint;
  final String? pinnedHostKey;

  bool get isSsh => kind == 'ssh';

  String get endpoint => isSsh ? '$username@$host${port == 22 ? '' : ':$port'}' : label;
}

class SshTestResult {
  const SshTestResult({required this.ok, this.error, required this.elapsedMs, this.hostKeyFingerprint, this.tunnelCheck});

  factory SshTestResult.fromJson(String raw) {
    final json = (jsonDecode(raw) as Map).cast<String, Object?>();
    return SshTestResult(
      ok: json['ok'] as bool? ?? false,
      error: json['error'] as String?,
      elapsedMs: _int(json['elapsed_ms']),
      hostKeyFingerprint: json['host_key_fingerprint'] as String?,
      tunnelCheck: json['tunnel_check'] as String?,
    );
  }

  final bool ok;
  final String? error;
  final int elapsedMs;
  final String? hostKeyFingerprint;
  final String? tunnelCheck;
}

class SshKeyInfo {
  const SshKeyInfo({required this.privateKey, required this.publicKey, required this.fingerprint});

  factory SshKeyInfo.fromJson(String raw) {
    final json = (jsonDecode(raw) as Map).cast<String, Object?>();
    return SshKeyInfo(
      privateKey: json['private_openssh'] as String? ?? '',
      publicKey: json['public_openssh'] as String? ?? '',
      fingerprint: json['fingerprint'] as String? ?? '',
    );
  }

  final String privateKey;
  final String publicKey;
  final String fingerprint;
}

/// Parses `ssh [-p N] user@host[:port] [...]`, `user@host:port` or
/// `ssh://user@host:port` into its parts. Returns null when no host is found.
({String? user, String host, int? port})? parseSshTarget(String input) {
  final tokens = input.trim().split(RegExp(r'\s+')).where((t) => t.isNotEmpty).toList();
  if (tokens.isEmpty) return null;
  if (tokens.first == 'ssh') tokens.removeAt(0);
  int? port;
  String? target;
  for (var i = 0; i < tokens.length; i++) {
    final t = tokens[i];
    if (t == '-p' && i + 1 < tokens.length) {
      port = int.tryParse(tokens[++i]);
    } else if (t.startsWith('-p') && t.length > 2) {
      port = int.tryParse(t.substring(2));
    } else if (const {'-D', '-L', '-R', '-i', '-l', '-o', '-J', '-F'}.contains(t)) {
      i++;
    } else if (!t.startsWith('-')) {
      target ??= t;
    }
  }
  if (target == null) return null;
  target = target.replaceFirst(RegExp(r'^ssh://'), '');
  String? user;
  final at = target.lastIndexOf('@');
  if (at >= 0) {
    user = target.substring(0, at);
    target = target.substring(at + 1);
  }
  final bracketed = RegExp(r'^\[(.+)\](?::(\d+))?$').firstMatch(target);
  if (bracketed != null) {
    target = bracketed.group(1)!;
    port ??= int.tryParse(bracketed.group(2) ?? '');
  } else if (':'.allMatches(target).length == 1) {
    final parts = target.split(':');
    target = parts[0];
    port ??= int.tryParse(parts[1]);
  }
  if (target.isEmpty) return null;
  return (user: user == null || user.isEmpty ? null : user, host: target, port: port);
}

int _int(Object? value, [int fallback = 0]) => value is num ? value.toInt() : fallback;

DateTime? _time(Object? ms) => ms is num ? DateTime.fromMillisecondsSinceEpoch(ms.toInt()) : null;

String formatBytes(int bytes) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return unit == 0 ? '$bytes B' : '${value.toStringAsFixed(value >= 100 ? 0 : 1)} ${units[unit]}';
}

String formatDuration(Duration d) {
  if (d.inHours >= 1) return '${d.inHours}h ${d.inMinutes.remainder(60)}m';
  if (d.inMinutes >= 1) return '${d.inMinutes}m ${d.inSeconds.remainder(60)}s';
  return '${d.inSeconds}s';
}

/// Rust errors arrive as `AnyhowException(message)`; keep just the message.
String describeError(Object error) {
  final text = error.toString();
  const prefix = 'AnyhowException(';
  if (text.startsWith(prefix) && text.endsWith(')')) {
    return text.substring(prefix.length, text.length - 1).split('\n\nStack backtrace').first.trim();
  }
  return text;
}
