import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart' show Color;
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:hydra_mobile/screens/connect_screen.dart' show categoryLabel;
import 'package:shared_preferences/shared_preferences.dart';

const String kMobileRoutesFile = 'mobile_routes.json';
const String kRoutePoliciesFile = 'route_policies.json';
const String kRelayUsageFile = 'relay_usage.json';
const String kRelayCostPerGbPref = 'relay_cost_per_gb_usd';
const double kDefaultRelayCostPerGb = 0.12;
const String kRelaySupportUrl = 'https://hydra-net.work';

class RouteProfile {
  const RouteProfile({
    required this.id,
    required this.label,
    required this.kind,
    required this.mode,
    required this.enabled,
    required this.priority,
    required this.source,
    required this.config,
  });

  final String id;
  final String label;
  final String kind;
  final String mode;
  final bool enabled;
  final int priority;
  final String source;
  final Map<String, dynamic> config;

  factory RouteProfile.fromJson(Map<String, dynamic> json) {
    return RouteProfile(
      id: json['id'] as String? ?? '',
      label: json['label'] as String? ?? '',
      kind: json['kind'] as String? ?? 'vless',
      mode: json['mode'] as String? ?? 'all',
      enabled: json['enabled'] as bool? ?? true,
      priority: (json['priority'] as num?)?.toInt() ?? 0,
      source: json['source'] as String? ?? 'imported_raw',
      config: Map<String, dynamic>.from(
        json['config'] as Map? ?? const <String, dynamic>{},
      ),
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'label': label,
      'kind': kind,
      'mode': mode,
      'enabled': enabled,
      'priority': priority,
      'source': source,
      'config': config,
    };
  }

  bool get isBuiltin => source == 'builtin';
  bool get isWss => kind == 'wss';
  bool get isVless => kind == 'vless';
  bool get isSsh => kind == 'ssh';

  String get endpointSummary {
    if (isWss) {
      final endpoints = (config['endpoints'] as List<dynamic>? ?? const [])
          .cast<dynamic>()
          .map((value) => value.toString())
          .where((value) => value.isNotEmpty)
          .toList();
      return endpoints.isEmpty ? 'No endpoint' : endpoints.join('\n');
    }
    if (isSsh) {
      final host = config['host'] as String? ?? '';
      final port = config['port'] ?? 22;
      final username = config['username'] as String? ?? '';
      return '$username@$host:$port';
    }
    return config['url'] as String? ?? '';
  }

  String get shortSummary {
    if (isWss) {
      final endpoints = (config['endpoints'] as List<dynamic>? ?? const [])
          .cast<dynamic>()
          .map((value) => value.toString())
          .where((value) => value.isNotEmpty)
          .toList();
      return endpoints.isEmpty ? 'No endpoint' : endpoints.first;
    }
    if (isSsh) {
      final host = config['host'] as String? ?? '';
      final port = config['port'] ?? 22;
      final username = config['username'] as String? ?? '';
      final authType = config['key_path'] != null
          ? 'key_file'
          : config['key_pem'] != null
              ? 'key_pem'
              : 'password';
      return '$username@$host:$port [$authType]';
    }
    final url = config['url'] as String? ?? '';
    if (url.isEmpty) {
      return 'No VLESS URI';
    }
    final uri = Uri.tryParse(url);
    if (uri == null) {
      return url;
    }
    final sni = uri.queryParameters['sni'];
    if (uri.hasPort) {
      return sni != null && sni.isNotEmpty
          ? '${uri.host}:${uri.port} ($sni)'
          : '${uri.host}:${uri.port}';
    }
    return uri.host;
  }

  RouteProfile copyWith({
    String? label,
    String? mode,
    bool? enabled,
    int? priority,
    Map<String, dynamic>? config,
  }) {
    return RouteProfile(
      id: id,
      label: label ?? this.label,
      kind: kind,
      mode: mode ?? this.mode,
      enabled: enabled ?? this.enabled,
      priority: priority ?? this.priority,
      source: source,
      config: config ?? this.config,
    );
  }
}

class RoutePolicyAction {
  const RoutePolicyAction({required this.type, this.profileId});

  final String type;
  final String? profileId;

  factory RoutePolicyAction.fromJson(Map<String, dynamic> json) {
    return RoutePolicyAction(
      type: json['type'] as String? ?? 'auto',
      profileId: json['profile_id'] as String?,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'type': type,
      if (profileId != null && profileId!.isNotEmpty) 'profile_id': profileId,
    };
  }

  String toLabel(List<RouteProfile> profiles) {
    switch (type) {
      case 'direct':
        return 'Direct';
      case 'block':
        return 'Block';
      case 'wss':
        return _profileLabel('WSS', profiles);
      case 'vless':
        return _profileLabel('VLESS', profiles);
      default:
        return 'Auto';
    }
  }

  String _profileLabel(String prefix, List<RouteProfile> profiles) {
    if (profileId == null || profileId!.isEmpty) {
      return prefix;
    }
    final match = profiles.cast<RouteProfile?>().firstWhere(
      (profile) => profile?.id == profileId,
      orElse: () => null,
    );
    return match == null ? '$prefix ($profileId)' : '$prefix (${match.label})';
  }
}

class RoutePolicyEntry {
  const RoutePolicyEntry({
    required this.groupKind,
    required this.groupKey,
    required this.action,
    required this.updatedAt,
  });

  final String groupKind;
  final String groupKey;
  final RoutePolicyAction action;
  final int updatedAt;

  factory RoutePolicyEntry.fromJson(Map<String, dynamic> json) {
    return RoutePolicyEntry(
      groupKind: json['group_kind'] as String? ?? 'domain',
      groupKey: json['group_key'] as String? ?? '',
      action: RoutePolicyAction.fromJson(
        Map<String, dynamic>.from(
          json['action'] as Map? ?? const <String, dynamic>{},
        ),
      ),
      updatedAt: (json['updated_at'] as num?)?.toInt() ?? 0,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'group_kind': groupKind,
      'group_key': groupKey,
      'action': action.toJson(),
      'updated_at': updatedAt,
    };
  }
}

class RelayUsageSample {
  const RelayUsageSample({required this.bucketStart, required this.bytes});

  final DateTime bucketStart;
  final int bytes;

  factory RelayUsageSample.fromJson(Map<String, dynamic> json) {
    final bucketStart = (json['bucket_start'] as num?)?.toInt() ?? 0;
    return RelayUsageSample(
      bucketStart: DateTime.fromMillisecondsSinceEpoch(
        bucketStart * 1000,
        isUtc: true,
      ).toLocal(),
      bytes: (json['bytes'] as num?)?.toInt() ?? 0,
    );
  }
}

class RelayUsageSummary {
  const RelayUsageSummary({
    required this.todayBytes,
    required this.last7dBytes,
    required this.last30dBytes,
  });

  final int todayBytes;
  final int last7dBytes;
  final int last30dBytes;
}

class ImportReport {
  const ImportReport({
    required this.imported,
    required this.skipped,
    required this.totalCandidates,
  });

  final List<RouteProfile> imported;
  final List<String> skipped;
  final int totalCandidates;
}

enum GroupingMode { app, category, country }

class ConnectionGroupModel {
  const ConnectionGroupModel({
    required this.key,
    required this.label,
    required this.count,
    required this.bytes,
    required this.trackers,
    required this.mode,
  });

  final String key;
  final String label;
  final int count;
  final int bytes;
  final int trackers;
  final GroupingMode mode;

  factory ConnectionGroupModel.fromAppJson(Map<String, dynamic> json) {
    final app = json['app'] as String? ?? 'unknown';
    final label = json['label'] as String?;
    return ConnectionGroupModel(
      key: app,
      label: label ?? app,
      count: (json['count'] as num?)?.toInt() ?? 0,
      bytes: (json['bytes'] as num?)?.toInt() ?? 0,
      trackers: (json['trackers'] as num?)?.toInt() ?? 0,
      mode: GroupingMode.app,
    );
  }

  factory ConnectionGroupModel.fromCategoryJson(Map<String, dynamic> json) {
    final category = json['category'] as String? ?? 'unknown';
    return ConnectionGroupModel(
      key: category,
      label: categoryLabel(category),
      count: (json['count'] as num?)?.toInt() ?? 0,
      bytes: (json['bytes'] as num?)?.toInt() ?? 0,
      trackers: 0,
      mode: GroupingMode.category,
    );
  }

  factory ConnectionGroupModel.fromCountryJson(Map<String, dynamic> json) {
    final country = json['country'] as String? ?? 'unknown';
    return ConnectionGroupModel(
      key: country,
      label: country == 'unknown' ? 'Unknown' : country,
      count: (json['count'] as num?)?.toInt() ?? 0,
      bytes: (json['bytes'] as num?)?.toInt() ?? 0,
      trackers: (json['trackers'] as num?)?.toInt() ?? 0,
      mode: GroupingMode.country,
    );
  }
}

class ConnectionSnapshotModel {
  const ConnectionSnapshotModel({
    required this.id,
    required this.targetHost,
    required this.targetPort,
    required this.routeType,
    required this.bytesUp,
    required this.bytesDown,
    required this.durationMs,
    required this.isProxied,
    required this.status,
    required this.groupKind,
    required this.groupKey,
    required this.resolvedPolicy,
    this.aiReason,
    this.appLabel,
    this.packageName,
    this.appUid,
    this.reverseDns,
    this.whoisOrg,
    this.whoisAsn,
    this.whoisCountry,
    this.classificationCategory,
    this.classificationConfidence,
    this.classificationSource,
    this.classificationExplanation,
    this.transportLabel,
  });

  final int id;
  final String targetHost;
  final int targetPort;
  final String routeType;
  final int bytesUp;
  final int bytesDown;
  final int durationMs;
  final bool isProxied;
  final String status;
  final String groupKind;
  final String groupKey;
  final String resolvedPolicy;
  final String? aiReason;
  final String? appLabel;
  final String? packageName;
  final int? appUid;
  final String? reverseDns;
  final String? whoisOrg;
  final int? whoisAsn;
  final String? whoisCountry;
  final String? classificationCategory;
  final double? classificationConfidence;
  final String? classificationSource;
  final String? classificationExplanation;
  final String? transportLabel;

  factory ConnectionSnapshotModel.fromJson(Map<String, dynamic> json) {
    return ConnectionSnapshotModel(
      id: (json['id'] as num?)?.toInt() ?? 0,
      targetHost: json['target_host'] as String? ?? '',
      targetPort: (json['target_port'] as num?)?.toInt() ?? 0,
      routeType: json['route_type'] as String? ?? 'direct',
      bytesUp: (json['bytes_up'] as num?)?.toInt() ?? 0,
      bytesDown: (json['bytes_down'] as num?)?.toInt() ?? 0,
      durationMs: (json['duration_ms'] as num?)?.toInt() ?? 0,
      isProxied: json['is_proxied'] as bool? ?? false,
      status: json['status'] as String? ?? 'closed',
      groupKind: json['group_kind'] as String? ?? 'domain',
      groupKey: json['group_key'] as String? ?? '',
      resolvedPolicy: json['resolved_policy'] as String? ?? 'Auto',
      aiReason: json['ai_reason'] as String?,
      appLabel: json['app_label'] as String?,
      packageName: json['package_name'] as String?,
      appUid: (json['app_uid'] as num?)?.toInt(),
      reverseDns: json['reverse_dns'] as String?,
      whoisOrg: json['whois_org'] as String?,
      whoisAsn: (json['whois_asn'] as num?)?.toInt(),
      whoisCountry: json['whois_country'] as String?,
      classificationCategory: json['classification_category'] as String?,
      classificationConfidence: (json['classification_confidence'] as num?)
          ?.toDouble(),
      classificationSource: json['classification_source'] as String?,
      classificationExplanation: json['classification_explanation'] as String?,
      transportLabel: json['transport_label'] as String?,
    );
  }

  int get totalBytes => bytesUp + bytesDown;

  bool get hasAppAttribution => appUid != null && appUid! >= 0;
}

class ConnectionStatsModel {
  const ConnectionStatsModel({
    required this.activeCount,
    required this.totalCount,
    required this.proxiedCount,
    required this.blockedCount,
    required this.trackerCount,
    required this.totalBytesUp,
    required this.totalBytesDown,
  });

  final int activeCount;
  final int totalCount;
  final int proxiedCount;
  final int blockedCount;
  final int trackerCount;
  final int totalBytesUp;
  final int totalBytesDown;

  factory ConnectionStatsModel.fromJson(Map<String, dynamic> json) {
    return ConnectionStatsModel(
      activeCount: (json['active_count'] as num?)?.toInt() ?? 0,
      totalCount: (json['total_count'] as num?)?.toInt() ?? 0,
      proxiedCount: (json['proxied_count'] as num?)?.toInt() ?? 0,
      blockedCount: (json['blocked_count'] as num?)?.toInt() ?? 0,
      trackerCount: (json['tracker_count'] as num?)?.toInt() ?? 0,
      totalBytesUp: (json['total_bytes_up'] as num?)?.toInt() ?? 0,
      totalBytesDown: (json['total_bytes_down'] as num?)?.toInt() ?? 0,
    );
  }
}

class MobileStateRepository {
  const MobileStateRepository();

  Future<List<RouteProfile>> loadRouteProfiles() async {
    final raw = await _readJsonList(kMobileRoutesFile);
    final profiles = raw
        .map((entry) => RouteProfile.fromJson(Map<String, dynamic>.from(entry)))
        .toList();
    profiles.sort((a, b) => a.priority.compareTo(b.priority));
    return profiles;
  }

  Future<void> saveRouteProfiles(List<RouteProfile> profiles) async {
    final normalized = <RouteProfile>[];
    for (var index = 0; index < profiles.length; index++) {
      normalized.add(profiles[index].copyWith(priority: index));
    }
    await _writeJson(
      kMobileRoutesFile,
      normalized.map((profile) => profile.toJson()).toList(),
    );
  }

  Future<void> updateRouteProfile(RouteProfile profile) async {
    final profiles = await loadRouteProfiles();
    final index = profiles.indexWhere((entry) => entry.id == profile.id);
    if (index == -1) {
      throw StateError('Unknown route profile: ${profile.id}');
    }
    profiles[index] = profile;
    await saveRouteProfiles(profiles);
  }

  Future<void> deleteRouteProfile(String profileId) async {
    final profiles = await loadRouteProfiles();
    final profile = profiles.cast<RouteProfile?>().firstWhere(
      (entry) => entry?.id == profileId,
      orElse: () => null,
    );
    if (profile == null) {
      return;
    }
    if (profile.isBuiltin) {
      throw StateError('Built-in route profiles cannot be deleted.');
    }
    profiles.removeWhere((entry) => entry.id == profileId);
    await saveRouteProfiles(profiles);
  }

  Future<ImportReport> importProfiles({
    required String payload,
    required String format,
  }) async {
    final normalizedFormat = format.trim().toLowerCase();
    final candidates = normalizedFormat == 'subscription'
        ? _parseSubscription(payload)
        : _parseRaw(payload);
    final profiles = await loadRouteProfiles();
    final imported = <RouteProfile>[];
    final skipped = <String>[];
    var nextPriority = profiles.length;

    for (var index = 0; index < candidates.length; index++) {
      final candidate = candidates[index];
      if (!candidate.startsWith('vless://')) {
        skipped.add('Skipped unsupported entry: $candidate');
        continue;
      }

      if (!_isValidVless(candidate)) {
        skipped.add('Skipped invalid VLESS entry: $candidate');
        continue;
      }

      final duplicate = profiles.any(
        (profile) => profile.isVless && profile.config['url'] == candidate,
      );
      if (duplicate) {
        skipped.add('Skipped duplicate VLESS entry: $candidate');
        continue;
      }

      final uri = Uri.parse(candidate);
      final profile = RouteProfile(
        id: 'imported-vless-${DateTime.now().millisecondsSinceEpoch}-$index',
        label: _labelForVless(uri),
        kind: 'vless',
        mode: 'all',
        enabled: true,
        priority: nextPriority,
        source: normalizedFormat == 'subscription'
            ? 'imported_subscription'
            : 'imported_raw',
        config: {'type': 'vless', 'url': candidate, 'mode': 'all'},
      );
      profiles.add(profile);
      imported.add(profile);
      nextPriority += 1;
    }

    await saveRouteProfiles(profiles);
    return ImportReport(
      imported: imported,
      skipped: skipped,
      totalCandidates: candidates.length,
    );
  }

  Future<List<RoutePolicyEntry>> loadRoutePolicies() async {
    final raw = await _readJsonList(kRoutePoliciesFile);
    return raw
        .map(
          (entry) =>
              RoutePolicyEntry.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList();
  }

  Future<void> setRoutePolicy({
    required String groupKind,
    required String groupKey,
    required RoutePolicyAction action,
  }) async {
    final policies = await loadRoutePolicies();
    final updatedAt = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final entry = RoutePolicyEntry(
      groupKind: groupKind,
      groupKey: groupKey,
      action: action,
      updatedAt: updatedAt,
    );
    final index = policies.indexWhere(
      (policy) => policy.groupKind == groupKind && policy.groupKey == groupKey,
    );
    if (index >= 0) {
      policies[index] = entry;
    } else {
      policies.add(entry);
    }
    await _writeJson(
      kRoutePoliciesFile,
      policies.map((policy) => policy.toJson()).toList(),
    );
  }

  Future<void> clearRoutePolicy({
    required String groupKind,
    required String groupKey,
  }) async {
    final policies = await loadRoutePolicies();
    policies.removeWhere(
      (policy) => policy.groupKind == groupKind && policy.groupKey == groupKey,
    );
    await _writeJson(
      kRoutePoliciesFile,
      policies.map((policy) => policy.toJson()).toList(),
    );
  }

  Future<List<RelayUsageSample>> loadRelayUsageSamples() async {
    final raw = await _readJsonList(kRelayUsageFile);
    return raw
        .map(
          (entry) =>
              RelayUsageSample.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList()
      ..sort((a, b) => a.bucketStart.compareTo(b.bucketStart));
  }

  Future<RelayUsageSummary> loadRelayUsageSummary() async {
    final samples = await loadRelayUsageSamples();
    final now = DateTime.now();
    final todayStart = DateTime(now.year, now.month, now.day);
    final weekStart = todayStart.subtract(const Duration(days: 6));
    final monthStart = todayStart.subtract(const Duration(days: 29));
    var todayBytes = 0;
    var last7dBytes = 0;
    var last30dBytes = 0;

    for (final sample in samples) {
      if (!sample.bucketStart.isBefore(todayStart)) {
        todayBytes += sample.bytes;
      }
      if (!sample.bucketStart.isBefore(weekStart)) {
        last7dBytes += sample.bytes;
      }
      if (!sample.bucketStart.isBefore(monthStart)) {
        last30dBytes += sample.bytes;
      }
    }

    return RelayUsageSummary(
      todayBytes: todayBytes,
      last7dBytes: last7dBytes,
      last30dBytes: last30dBytes,
    );
  }

  Future<double> loadRelayCostPerGb() async {
    final prefs = await SharedPreferences.getInstance();
    return prefs.getDouble(kRelayCostPerGbPref) ?? kDefaultRelayCostPerGb;
  }

  Future<void> saveRelayCostPerGb(double value) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setDouble(kRelayCostPerGbPref, value);
  }

  Future<List<ConnectionSnapshotModel>> loadConnections() async {
    final jsonText = await HydraPlatformGateway.instance.getActiveConnections();
    final decoded = jsonDecode(jsonText) as List<dynamic>;
    return decoded
        .map(
          (entry) => ConnectionSnapshotModel.fromJson(
            Map<String, dynamic>.from(entry as Map),
          ),
        )
        .toList();
  }

  Future<ConnectionStatsModel> loadConnectionStats() async {
    final jsonText = await HydraPlatformGateway.instance.getConnectionStats();
    final decoded = jsonDecode(jsonText) as Map<String, dynamic>;
    return ConnectionStatsModel.fromJson(decoded);
  }

  Future<List<ConnectionGroupModel>> loadConnectionsByApp() async {
    final jsonText = await HydraPlatformGateway.instance.getConnectionsByApp();
    final decoded = jsonDecode(jsonText) as Map<String, dynamic>;
    final groups = decoded['groups'] as List<dynamic>? ?? [];
    return groups
        .map((e) => ConnectionGroupModel.fromAppJson(Map<String, dynamic>.from(e as Map)))
        .toList();
  }

  Future<List<ConnectionGroupModel>> loadConnectionsByCategory() async {
    final jsonText = await HydraPlatformGateway.instance.getConnectionsByCategory();
    final decoded = jsonDecode(jsonText) as Map<String, dynamic>;
    final groups = decoded['groups'] as List<dynamic>? ?? [];
    return groups
        .map((e) => ConnectionGroupModel.fromCategoryJson(Map<String, dynamic>.from(e as Map)))
        .toList();
  }

  Future<List<ConnectionGroupModel>> loadConnectionsByCountry() async {
    final jsonText = await HydraPlatformGateway.instance.getConnectionsByCountry();
    final decoded = jsonDecode(jsonText) as Map<String, dynamic>;
    final groups = decoded['groups'] as List<dynamic>? ?? [];
    return groups
        .map((e) => ConnectionGroupModel.fromCountryJson(Map<String, dynamic>.from(e as Map)))
        .toList();
  }

  List<String> _parseRaw(String payload) {
    return payload
        .split('\n')
        .map((line) => line.trim())
        .where((line) => line.isNotEmpty)
        .toList();
  }

  List<String> _parseSubscription(String payload) {
    final compact = payload.split('\n').map((line) => line.trim()).join();
    final normalized = base64.normalize(compact);

    try {
      final decoded = utf8.decode(base64.decode(normalized));
      return _parseRaw(decoded);
    } catch (_) {
      final decoded = utf8.decode(base64Url.decode(normalized));
      return _parseRaw(decoded);
    }
  }

  bool _isValidVless(String value) {
    final uri = Uri.tryParse(value);
    return uri != null &&
        uri.scheme == 'vless' &&
        uri.host.isNotEmpty &&
        uri.hasPort;
  }

  String _labelForVless(Uri uri) {
    final tag = uri.fragment.trim();
    if (tag.isNotEmpty) {
      return Uri.decodeComponent(tag);
    }
    final sni = uri.queryParameters['sni'] ?? uri.queryParameters['host'];
    if (sni != null && sni.isNotEmpty) {
      return '${uri.host}:${uri.port} ($sni)';
    }
    return '${uri.host}:${uri.port}';
  }

  Future<List<dynamic>> _readJsonList(String fileName) async {
    final file = await _file(fileName);
    if (!await file.exists()) {
      return const [];
    }
    final raw = await file.readAsString();
    if (raw.trim().isEmpty) {
      return const [];
    }
    final decoded = jsonDecode(raw);
    return decoded is List<dynamic> ? decoded : const [];
  }

  Future<void> _writeJson(String fileName, Object value) async {
    final file = await _file(fileName);
    await file.parent.create(recursive: true);
    final encoded = const JsonEncoder.withIndent('  ').convert(value);
    await file.writeAsString(encoded);
  }

  Future<File> _file(String fileName) async {
    final baseDir = await HydraPlatformGateway.instance.resolveBaseDir();
    return File('$baseDir/$fileName');
  }
}

String formatBytes(int bytes) {
  if (bytes < 1024) {
    return '$bytes B';
  }
  if (bytes < 1024 * 1024) {
    return '${(bytes / 1024).toStringAsFixed(1)} KB';
  }
  if (bytes < 1024 * 1024 * 1024) {
    return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
  return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(2)} GB';
}

String formatUsd(double value) {
  return value.toStringAsFixed(value >= 10 ? 2 : 3);
}

String formatDuration(Duration duration) {
  if (duration.inHours > 0) {
    return '${duration.inHours}h ${duration.inMinutes.remainder(60)}m';
  }
  if (duration.inMinutes > 0) {
    return '${duration.inMinutes}m ${duration.inSeconds.remainder(60)}s';
  }
  return '${duration.inSeconds}s';
}

String routeTypeLabel(String routeType) {
  switch (routeType) {
    case 'wss':
      return 'WSS';
    case 'vless':
      return 'VLESS';
    case 'p2p':
      return 'P2P';
    case 'blocked':
      return 'BLOCK';
    default:
      return 'DIRECT';
  }
}

Color routeTypeColor(String routeType) {
  switch (routeType) {
    case 'wss':
      return const Color(0xFF38BDF8);
    case 'vless':
      return const Color(0xFF22C55E);
    case 'blocked':
      return const Color(0xFFEF4444);
    case 'ssh':
      return const Color(0xFFA78BFA);
    case 'p2p':
      return const Color(0xFFF59E0B);
    default:
      return const Color(0xFF94A3B8);
  }
}
