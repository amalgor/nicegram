import 'dart:async';

import 'package:flutter/material.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';

class _ConnectionGroupView {
  _ConnectionGroupView({
    required this.groupKind,
    required this.groupKey,
    required this.title,
    required this.subtitle,
    required this.connections,
    required this.savedPolicyLabel,
  });

  final String groupKind;
  final String groupKey;
  final String title;
  final String subtitle;
  final List<ConnectionSnapshotModel> connections;
  final String savedPolicyLabel;

  int get activeCount =>
      connections.where((conn) => conn.status == 'active').length;

  int get totalBytes =>
      connections.fold(0, (total, conn) => total + conn.totalBytes);

  String get routeSummary {
    final counts = <String, int>{};
    for (final connection in connections) {
      counts.update(
        connection.routeType,
        (value) => value + 1,
        ifAbsent: () => 1,
      );
    }
    final ordered = counts.entries.toList()
      ..sort((a, b) => b.value.compareTo(a.value));
    return ordered
        .map((entry) => '${entry.value} ${routeTypeLabel(entry.key)}')
        .join(' • ');
  }
}

class ConnectionsScreen extends StatefulWidget {
  const ConnectionsScreen({super.key});

  @override
  State<ConnectionsScreen> createState() => _ConnectionsScreenState();
}

class _ConnectionsScreenState extends State<ConnectionsScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  static const _repository = MobileStateRepository();

  Timer? _refreshTimer;
  bool _loading = true;
  bool _showClosed = false;
  List<ConnectionSnapshotModel> _connections = const [];
  List<RouteProfile> _profiles = const [];
  List<RoutePolicyEntry> _policies = const [];
  ConnectionStatsModel _stats = const ConnectionStatsModel(
    activeCount: 0,
    totalCount: 0,
    proxiedCount: 0,
    totalBytesUp: 0,
    totalBytesDown: 0,
  );

  @override
  void initState() {
    super.initState();
    _refresh();
    _refreshTimer = Timer.periodic(
      const Duration(seconds: 2),
      (_) => _refresh(),
    );
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    try {
      final results = await Future.wait<dynamic>([
        _repository.loadConnections(),
        _repository.loadConnectionStats(),
        _repository.loadRouteProfiles(),
        _repository.loadRoutePolicies(),
      ]);
      if (!mounted) {
        return;
      }
      setState(() {
        _connections = results[0] as List<ConnectionSnapshotModel>;
        _stats = results[1] as ConnectionStatsModel;
        _profiles = results[2] as List<RouteProfile>;
        _policies = results[3] as List<RoutePolicyEntry>;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
      });
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to refresh connections: $e')),
      );
    }
  }

  RoutePolicyEntry? _findPolicy(String groupKind, String groupKey) {
    return _policies.cast<RoutePolicyEntry?>().firstWhere(
      (entry) => entry?.groupKind == groupKind && entry?.groupKey == groupKey,
      orElse: () => null,
    );
  }

  List<_ConnectionGroupView> _buildGroups() {
    final visible = _showClosed
        ? _connections
        : _connections
              .where((connection) => connection.status == 'active')
              .toList();
    final grouped = <String, List<ConnectionSnapshotModel>>{};

    for (final connection in visible) {
      final key = '${connection.groupKind}:${connection.groupKey}';
      grouped.putIfAbsent(key, () => []).add(connection);
    }

    final views = <_ConnectionGroupView>[];
    for (final entry in grouped.entries) {
      final connections = entry.value;
      final first = connections.first;
      final savedPolicy = _findPolicy(first.groupKind, first.groupKey);
      final title = first.groupKind == 'app'
          ? (first.appLabel?.isNotEmpty == true
                ? first.appLabel!
                : first.packageName ?? first.groupKey)
          : first.groupKey;
      final subtitle = first.groupKind == 'app'
          ? (first.packageName ?? 'App-routed traffic')
          : 'Domain group';
      views.add(
        _ConnectionGroupView(
          groupKind: first.groupKind,
          groupKey: first.groupKey,
          title: title,
          subtitle: subtitle,
          connections: connections,
          savedPolicyLabel:
              savedPolicy?.action.toLabel(_profiles) ?? first.resolvedPolicy,
        ),
      );
    }

    views.sort((a, b) {
      if (a.activeCount != b.activeCount) {
        return b.activeCount.compareTo(a.activeCount);
      }
      return b.totalBytes.compareTo(a.totalBytes);
    });
    return views;
  }

  Future<void> _showPolicySheet(_ConnectionGroupView group) async {
    final wssProfiles = _profiles
        .where((profile) => profile.enabled && profile.isWss)
        .toList();
    final vlessProfiles = _profiles
        .where((profile) => profile.enabled && profile.isVless)
        .toList();

    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (context) {
        return SafeArea(
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
            children: [
              Text(group.title, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 4),
              Text(
                'Choose how new traffic in this group should be routed. Saved choices persist across restarts.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 12),
              _policyTile(
                title: 'Auto',
                subtitle: 'Use global proxy mode and route priority order.',
                onTap: () => _applyPolicy(
                  group: group,
                  action: const RoutePolicyAction(type: 'auto'),
                ),
              ),
              _policyTile(
                title: 'Direct',
                subtitle: 'Bypass WSS and VLESS for this group.',
                onTap: () => _applyPolicy(
                  group: group,
                  action: const RoutePolicyAction(type: 'direct'),
                ),
              ),
              _policyTile(
                title: 'Block',
                subtitle: 'Refuse matching traffic inside the runtime.',
                onTap: () => _applyPolicy(
                  group: group,
                  action: const RoutePolicyAction(type: 'block'),
                ),
              ),
              if (wssProfiles.isNotEmpty) ...[
                const SizedBox(height: 12),
                Text(
                  'WSS relay profiles',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 8),
                ...wssProfiles.map(
                  (profile) => _policyTile(
                    title: profile.label,
                    subtitle: profile.shortSummary,
                    onTap: () => _applyPolicy(
                      group: group,
                      action: RoutePolicyAction(
                        type: 'wss',
                        profileId: profile.id,
                      ),
                    ),
                  ),
                ),
              ],
              if (vlessProfiles.isNotEmpty) ...[
                const SizedBox(height: 12),
                Text(
                  'Imported VLESS profiles',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 8),
                ...vlessProfiles.map(
                  (profile) => _policyTile(
                    title: profile.label,
                    subtitle: profile.shortSummary,
                    onTap: () => _applyPolicy(
                      group: group,
                      action: RoutePolicyAction(
                        type: 'vless',
                        profileId: profile.id,
                      ),
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 12),
              TextButton(
                onPressed: () async {
                  Navigator.of(context).pop();
                  await _repository.clearRoutePolicy(
                    groupKind: group.groupKind,
                    groupKey: group.groupKey,
                  );
                  await _refresh();
                },
                child: const Text('Clear saved choice'),
              ),
            ],
          ),
        );
      },
    );
  }

  Future<void> _applyPolicy({
    required _ConnectionGroupView group,
    required RoutePolicyAction action,
  }) async {
    Navigator.of(context).pop();
    await _repository.setRoutePolicy(
      groupKind: group.groupKind,
      groupKey: group.groupKey,
      action: action,
    );
    await _refresh();
    if (!mounted) {
      return;
    }
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          'Saved ${action.toLabel(_profiles)} for ${group.title}. Runtime applies file changes on the next refresh cycle.',
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }

    final groups = _buildGroups();

    return Column(
      children: [
        _buildStatsBar(context),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
          child: Row(
            children: [
              Text(
                '${groups.length} groups',
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const Spacer(),
              Text('Show closed', style: Theme.of(context).textTheme.bodySmall),
              Switch(
                value: _showClosed,
                onChanged: (value) {
                  setState(() {
                    _showClosed = value;
                  });
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: groups.isEmpty
              ? Center(
                  child: Text(
                    'No connection groups yet.',
                    style: Theme.of(context).textTheme.bodyMedium,
                  ),
                )
              : RefreshIndicator(
                  onRefresh: _refresh,
                  child: ListView.builder(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                    itemCount: groups.length,
                    itemBuilder: (context, index) =>
                        _buildGroupCard(groups[index]),
                  ),
                ),
        ),
      ],
    );
  }

  Widget _buildStatsBar(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Row(
        children: [
          _stat(context, '${_stats.activeCount}', 'Active'),
          _stat(context, '${_stats.proxiedCount}', 'Proxied'),
          _stat(context, formatBytes(_stats.totalBytesUp), 'Up'),
          _stat(context, formatBytes(_stats.totalBytesDown), 'Down'),
        ],
      ),
    );
  }

  Widget _buildGroupCard(_ConnectionGroupView group) {
    final hasAppOwner = group.groupKind == 'app';

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: ExpansionTile(
        tilePadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
        childrenPadding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        leading: Container(
          width: 40,
          height: 40,
          decoration: BoxDecoration(
            color: hasAppOwner
                ? const Color(0xFF0EA5E9).withValues(alpha: 0.12)
                : const Color(0xFF94A3B8).withValues(alpha: 0.12),
            borderRadius: BorderRadius.circular(12),
          ),
          child: Icon(
            hasAppOwner ? Icons.apps : Icons.language,
            color: hasAppOwner
                ? const Color(0xFF38BDF8)
                : const Color(0xFFCBD5E1),
          ),
        ),
        title: Text(
          group.title,
          style: const TextStyle(fontWeight: FontWeight.w700),
        ),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                group.subtitle,
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 4),
              Text(
                '${group.routeSummary} • ${formatBytes(group.totalBytes)}',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
          ),
        ),
        trailing: IconButton(
          onPressed: () => _showPolicySheet(group),
          icon: const Icon(Icons.alt_route),
          tooltip: 'Route policy',
        ),
        children: [
          Align(
            alignment: Alignment.centerLeft,
            child: Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                _badge(
                  label: group.savedPolicyLabel,
                  color: const Color(0xFF0EA5E9),
                ),
                _badge(
                  label: '${group.activeCount} active',
                  color: const Color(0xFF22C55E),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          ...group.connections.map(_buildConnectionRow),
        ],
      ),
    );
  }

  Widget _buildConnectionRow(ConnectionSnapshotModel connection) {
    final routeColor = routeTypeColor(connection.routeType);

    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Theme.of(
          context,
        ).colorScheme.surfaceContainer.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(14),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  '${connection.targetHost}:${connection.targetPort}',
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
              ),
              _badge(
                label: routeTypeLabel(connection.routeType),
                color: routeColor,
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(
            '${formatBytes(connection.totalBytes)} • ${formatDuration(Duration(milliseconds: connection.durationMs))}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 4),
          Text(
            connection.transportLabel?.isNotEmpty == true
                ? connection.transportLabel!
                : 'Resolved policy: ${connection.resolvedPolicy}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }

  Widget _policyTile({
    required String title,
    required String subtitle,
    required Future<void> Function() onTap,
  }) {
    return Card(
      child: ListTile(
        title: Text(title),
        subtitle: Text(subtitle),
        trailing: const Icon(Icons.chevron_right),
        onTap: () {
          onTap();
        },
      ),
    );
  }

  Widget _badge({required String label, required Color color}) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label, style: TextStyle(color: color, fontSize: 12)),
    );
  }

  Widget _stat(BuildContext context, String value, String label) {
    return Expanded(
      child: Column(
        children: [
          Text(value, style: Theme.of(context).textTheme.titleMedium),
          Text(label, style: Theme.of(context).textTheme.bodySmall),
        ],
      ),
    );
  }
}
