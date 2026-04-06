import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:hydra_mobile/src/rust/api/vpn.dart';

bool gIsVpnActive = false;

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
      ]);
      if (!mounted) {
        return;
      }
      setState(() {
        _stats = results[0] as ConnectionStatsModel;
        _profiles = results[1] as List<RouteProfile>;
        _relayUsage = results[2] as RelayUsageSummary;
        _relayCostPerGb = results[3] as double;
      });
    } catch (e) {
      debugPrint('Dashboard refresh failed: $e');
    }
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

    return RefreshIndicator(
      onRefresh: _refreshDashboard,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _buildHeroCard(context),
          const SizedBox(height: 16),
          _buildStatsGrid(context),
          const SizedBox(height: 16),
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
          const SizedBox(height: 16),
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
          const SizedBox(height: 16),
          Card(
            child: ListTile(
              leading: const Icon(Icons.smart_toy_outlined),
              title: const Text('Optional AI stays off by default'),
              subtitle: const Text(
                'No LLM model is bundled into this APK. Download a model later from Settings if you want local analysis.',
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildHeroCard(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(24),
        gradient: const LinearGradient(
          colors: [Color(0xFF0F172A), Color(0xFF0C4A6E)],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          children: [
            GestureDetector(
              onTap: _toggleVpn,
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 220),
                width: 152,
                height: 152,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: gIsVpnActive
                      ? const Color(0xFF22C55E).withValues(alpha: 0.18)
                      : Colors.white.withValues(alpha: 0.08),
                  border: Border.all(
                    color: gIsVpnActive
                        ? const Color(0xFF22C55E)
                        : Colors.white24,
                    width: 3,
                  ),
                ),
                child: _busy
                    ? const Center(child: CircularProgressIndicator())
                    : Icon(
                        Icons.power_settings_new,
                        size: 80,
                        color: gIsVpnActive
                            ? const Color(0xFF86EFAC)
                            : Colors.white,
                      ),
              ),
            ),
            const SizedBox(height: 18),
            Text(
              gIsVpnActive ? 'VPN Active' : 'VPN Idle',
              style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                color: gIsVpnActive ? const Color(0xFF86EFAC) : Colors.white,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(height: 6),
            Text(
              gIsVpnActive
                  ? 'Android VPN is routing traffic into the local SOCKS5 runtime.'
                  : 'Import your own VLESS credentials or use the built-in WSS relay, then start the VPN.',
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                color: Colors.white.withValues(alpha: 0.8),
              ),
            ),
            if (gIsVpnActive && _connectedAt != null) ...[
              const SizedBox(height: 10),
              Text(
                'Uptime ${formatDuration(_uptime)}',
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: Colors.white70),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildStatsGrid(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: _statCard(
            context,
            label: 'Active',
            value: '${_stats.activeCount}',
            icon: Icons.link,
            color: const Color(0xFF22C55E),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: _statCard(
            context,
            label: 'Proxied',
            value: '${_stats.proxiedCount}',
            icon: Icons.cloud_queue,
            color: const Color(0xFF38BDF8),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: _statCard(
            context,
            label: 'Uplink',
            value: formatBytes(_stats.totalBytesUp),
            icon: Icons.north,
            color: const Color(0xFFF59E0B),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: _statCard(
            context,
            label: 'Downlink',
            value: formatBytes(_stats.totalBytesDown),
            icon: Icons.south,
            color: const Color(0xFFA78BFA),
          ),
        ),
      ],
    );
  }

  Widget _statCard(
    BuildContext context, {
    required String label,
    required String value,
    required IconData icon,
    required Color color,
  }) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon, color: color, size: 18),
            const SizedBox(height: 10),
            Text(
              value,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 4),
            Text(label, style: Theme.of(context).textTheme.bodySmall),
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
