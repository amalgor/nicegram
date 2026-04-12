import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:hydra_mobile/src/rust/api/vpn.dart';

bool gIsVpnActive = false;

// Classification category colors
const kCategoryColors = <String, Color>{
  'advertising': Color(0xFFEF4444),
  'analytics': Color(0xFFF97316),
  'telemetry': Color(0xFFEAB308),
  'social_tracking': Color(0xFF8B5CF6),
  'legitimate': Color(0xFF22C55E),
  'unknown': Color(0xFF94A3B8),
  'malware': Color(0xFFDC2626),
};

// Classification category short labels for UI pills
const kCategoryLabels = <String, String>{
  'advertising': 'ADS',
  'analytics': 'ANALYTICS',
  'telemetry': 'TELEMETRY',
  'social_tracking': 'SOCIAL',
  'legitimate': 'CLEAN',
  'unknown': 'UNKNOWN',
  'malware': 'MALWARE',
};

Color categoryColor(String? category) {
  if (category == null) return kCategoryColors['unknown']!;
  return kCategoryColors[category] ?? kCategoryColors['unknown']!;
}

String categoryLabel(String? category) {
  if (category == null) return 'UNKNOWN';
  return kCategoryLabels[category] ?? category.toUpperCase();
}

bool isTrackerCategory(String? category) {
  return category == 'advertising' ||
      category == 'analytics' ||
      category == 'telemetry' ||
      category == 'social_tracking' ||
      category == 'malware';
}

class _ThreatAppInfo {
  _ThreatAppInfo({required this.appName, required this.packageName});
  final String appName;
  final String packageName;
  int trackerCount = 0;
  final Set<String> companies = {};
}

class ConnectScreen extends StatefulWidget {
  const ConnectScreen({super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  static const _repository = MobileStateRepository();

  Timer? _refreshTimer;
  DateTime? _connectedAt;
  Duration _uptime = Duration.zero;
  ConnectionStatsModel _stats = const ConnectionStatsModel(
    activeCount: 0,
    totalCount: 0,
    proxiedCount: 0,
    blockedCount: 0,
    trackerCount: 0,
    totalBytesUp: 0,
    totalBytesDown: 0,
  );
  RelayUsageSummary _relayUsage = const RelayUsageSummary(
    todayBytes: 0,
    last7dBytes: 0,
    last30dBytes: 0,
  );
  double _relayCostPerGb = kDefaultRelayCostPerGb;
  List<RouteProfile> _profiles = const [];
  List<ConnectionSnapshotModel> _connections = const [];
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    HydraPlatformGateway.instance.bindVpnFdHandler((fd) async {
      if (Platform.isAndroid && fd >= 0) {
        try {
          startVpnTunnel(fd: fd);
        } catch (e) {
          debugPrint('Failed to start Rust VPN tunnel: $e');
        }
      }
    });
    _syncVpnStatus();
    _refreshDashboard();
    _refreshTimer = Timer.periodic(const Duration(seconds: 3), (_) {
      _syncVpnStatus();
      _refreshDashboard();
    });
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  Future<void> _syncVpnStatus() async {
    try {
      final active = await HydraPlatformGateway.instance.getVpnActive();
      if (!mounted) {
        return;
      }
      setState(() {
        if (active && !gIsVpnActive && _connectedAt == null) {
          _connectedAt = DateTime.now();
        }
        if (!active) {
          _connectedAt = null;
          _uptime = Duration.zero;
        } else if (_connectedAt != null) {
          _uptime = DateTime.now().difference(_connectedAt!);
        }
        gIsVpnActive = active;
      });
    } catch (e) {
      debugPrint('VPN status sync failed: $e');
    }
  }

  Future<void> _refreshDashboard() async {
    try {
      final results = await Future.wait<dynamic>([
        _repository.loadConnectionStats(),
        _repository.loadRouteProfiles(),
        _repository.loadRelayUsageSummary(),
        _repository.loadRelayCostPerGb(),
        _repository.loadConnections(),
      ]);
      if (!mounted) {
        return;
      }
      setState(() {
        _stats = results[0] as ConnectionStatsModel;
        _profiles = results[1] as List<RouteProfile>;
        _relayUsage = results[2] as RelayUsageSummary;
        _relayCostPerGb = results[3] as double;
        _connections = results[4] as List<ConnectionSnapshotModel>;
      });
    } catch (e) {
      debugPrint('Dashboard refresh failed: $e');
    }
  }

  Map<String, int> _categoryCounts() {
    final counts = <String, int>{};
    for (final conn in _connections) {
      final cat = conn.classificationCategory ?? 'unknown';
      counts.update(cat, (v) => v + 1, ifAbsent: () => 1);
    }
    return counts;
  }

  List<_ThreatAppInfo> _topThreats({int limit = 3}) {
    final apps = <String, _ThreatAppInfo>{};
    for (final conn in _connections) {
      if (!isTrackerCategory(conn.classificationCategory)) continue;
      final key = conn.packageName ?? conn.groupKey;
      if (key.isEmpty) continue;
      final info = apps.putIfAbsent(
        key,
        () => _ThreatAppInfo(
          appName: conn.appLabel ?? conn.groupKey,
          packageName: key,
        ),
      );
      info.trackerCount++;
      final org = conn.whoisOrg;
      if (org != null && org.isNotEmpty) {
        info.companies.add(org);
      }
      final src = conn.classificationSource;
      if (src != null && src.isNotEmpty && info.companies.length < 3) {
        final cat = conn.classificationCategory;
        if (cat != null) {
          info.companies.add(categoryLabel(cat));
        }
      }
    }
    final sorted = apps.values.toList()
      ..sort((a, b) => b.trackerCount.compareTo(a.trackerCount));
    return sorted.take(limit).toList();
  }

  Future<void> _toggleVpn() async {
    if (_busy) {
      return;
    }

    setState(() {
      _busy = true;
    });
    try {
      if (gIsVpnActive) {
        await HydraPlatformGateway.instance.stopVpn();
        if (Platform.isAndroid) {
          stopVpnTunnel();
        }
        setState(() {
          gIsVpnActive = false;
          _connectedAt = null;
          _uptime = Duration.zero;
        });
      } else {
        final started = await HydraPlatformGateway.instance.startVpn();
        if (started) {
          setState(() {
            gIsVpnActive = true;
            _connectedAt = DateTime.now();
            _uptime = Duration.zero;
          });
        }
      }
      await _refreshDashboard();
    } catch (e) {
      if (!mounted) {
        return;
      }
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('VPN action failed: $e')));
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final enabledProfiles = _profiles
        .where((profile) => profile.enabled)
        .length;
    final wssProfiles = _profiles.where((profile) => profile.isWss).length;
    final vlessProfiles = _profiles.where((profile) => profile.isVless).length;
    final estimatedTodayCost =
        (_relayUsage.todayBytes / (1024 * 1024 * 1024)) * _relayCostPerGb;
    final categoryCounts = _categoryCounts();
    final threats = _topThreats();

    return RefreshIndicator(
      onRefresh: _refreshDashboard,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _buildIntelligenceSummary(context, categoryCounts),
          const SizedBox(height: 12),
          _buildCompactVpnBar(context),
          const SizedBox(height: 12),
          if (threats.isNotEmpty) ...[
            _buildTopThreats(context, threats),
            const SizedBox(height: 12),
          ],
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Route Inventory',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 10),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      _metricChip(
                        context,
                        label: 'Enabled',
                        value: '$enabledProfiles',
                        color: const Color(0xFF0EA5E9),
                      ),
                      _metricChip(
                        context,
                        label: 'WSS',
                        value: '$wssProfiles',
                        color: const Color(0xFF38BDF8),
                      ),
                      _metricChip(
                        context,
                        label: 'VLESS',
                        value: '$vlessProfiles',
                        color: const Color(0xFF22C55E),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Text(
                    _profiles.isEmpty
                        ? 'Runtime is still preparing route profiles.'
                        : 'Auto mode prefers enabled WSS first, then imported VLESS profiles by priority.',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Relay Snapshot',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 10),
                  Text(
                    formatBytes(_relayUsage.todayBytes),
                    style: Theme.of(context).textTheme.headlineSmall,
                  ),
                  const SizedBox(height: 4),
                  Text(
                    'Estimated today cost: \$${formatUsd(estimatedTodayCost)} at \$${formatUsd(_relayCostPerGb)}/GB',
                    style: Theme.of(context).textTheme.bodyMedium,
                  ),
                  const SizedBox(height: 12),
                  Text(
                    'Only Cloudflare WSS relay traffic is counted here. Direct and imported VLESS traffic stay out of this estimate.',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildIntelligenceSummary(
    BuildContext context,
    Map<String, int> categoryCounts,
  ) {
    final totalAnalyzed = _connections.length;
    final blockedBytes = _connections
        .where((c) => c.routeType == 'blocked')
        .fold(0, (sum, c) => sum + c.totalBytes);

    // Build ordered category pills: tracker categories first, then clean/unknown
    final pillOrder = [
      'advertising',
      'analytics',
      'telemetry',
      'social_tracking',
      'malware',
      'legitimate',
      'unknown',
    ];

    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(20),
        gradient: const LinearGradient(
          colors: [Color(0xFF0F172A), Color(0xFF1E1B4B)],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text(
                  '$totalAnalyzed',
                  style: Theme.of(context).textTheme.headlineLarge?.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w800,
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    'connections analyzed',
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                      color: Colors.white70,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 14),
            Wrap(
              spacing: 6,
              runSpacing: 6,
              children: pillOrder
                  .where((cat) => (categoryCounts[cat] ?? 0) > 0)
                  .map((cat) {
                    final count = categoryCounts[cat] ?? 0;
                    final color = categoryColor(cat);
                    return Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 10,
                        vertical: 5,
                      ),
                      decoration: BoxDecoration(
                        color: color.withValues(alpha: 0.18),
                        borderRadius: BorderRadius.circular(999),
                        border: Border.all(
                          color: color.withValues(alpha: 0.35),
                        ),
                      ),
                      child: Text(
                        '${categoryLabel(cat)} $count',
                        style: TextStyle(
                          color: color,
                          fontSize: 11,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    );
                  })
                  .toList(),
            ),
            if (_stats.blockedCount > 0 || blockedBytes > 0) ...[
              const SizedBox(height: 12),
              Text(
                '[BLOCKED] ${_stats.blockedCount} connections, ${formatBytes(blockedBytes)} saved',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: const Color(0xFFEF4444).withValues(alpha: 0.9),
                ),
              ),
            ],
            const SizedBox(height: 10),
            Text(
              'Powered by on-device analysis',
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Colors.white38,
                fontSize: 11,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildCompactVpnBar(BuildContext context) {
    final statusColor = gIsVpnActive
        ? const Color(0xFF22C55E)
        : const Color(0xFF94A3B8);

    return Card(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Row(
          children: [
            Container(
              width: 10,
              height: 10,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: statusColor,
                boxShadow: gIsVpnActive
                    ? [
                        BoxShadow(
                          color: statusColor.withValues(alpha: 0.5),
                          blurRadius: 6,
                        ),
                      ]
                    : null,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    gIsVpnActive ? 'VPN Active' : 'VPN Idle',
                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                  if (gIsVpnActive && _connectedAt != null)
                    Text(
                      'Uptime ${formatDuration(_uptime)}  |  ${_stats.activeCount} active  |  ${formatBytes(_stats.totalBytesDown)} down',
                      style: Theme.of(context).textTheme.bodySmall,
                    )
                  else
                    Text(
                      'Start VPN to route traffic',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                ],
              ),
            ),
            SizedBox(
              height: 40,
              width: 64,
              child: _busy
                  ? const Center(
                      child: SizedBox(
                        width: 20,
                        height: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                    )
                  : Switch(
                      value: gIsVpnActive,
                      onChanged: (_) => _toggleVpn(),
                      activeThumbColor: const Color(0xFF22C55E),
                    ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildTopThreats(
    BuildContext context,
    List<_ThreatAppInfo> threats,
  ) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Top Threats',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 10),
            ...threats.map((info) {
              final companiesText = info.companies.isNotEmpty
                  ? info.companies.take(3).join(', ')
                  : 'Tracker traffic';
              return Padding(
                padding: const EdgeInsets.only(bottom: 10),
                child: Row(
                  children: [
                    Container(
                      width: 36,
                      height: 36,
                      decoration: BoxDecoration(
                        color: const Color(0xFFEF4444).withValues(alpha: 0.12),
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: const Icon(
                        Icons.apps,
                        color: Color(0xFFEF4444),
                        size: 20,
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            info.appName,
                            style: const TextStyle(
                              fontWeight: FontWeight.w600,
                              fontSize: 13,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                          Text(
                            '${info.trackerCount} tracker connections ($companiesText)',
                            style: Theme.of(context).textTheme.bodySmall,
                            overflow: TextOverflow.ellipsis,
                            maxLines: 1,
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              );
            }),
          ],
        ),
      ),
    );
  }

  Widget _metricChip(
    BuildContext context, {
    required String label,
    required String value,
    required Color color,
  }) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: color.withValues(alpha: 0.28)),
      ),
      child: Text(
        '$label $value',
        style: Theme.of(context).textTheme.bodySmall?.copyWith(color: color),
      ),
    );
  }
}
