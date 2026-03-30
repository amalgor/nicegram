import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/widgets/connection_tile.dart';

class _AppGroup {
  final String appDomain;
  final List<ConnectionData> connections;
  bool isProxied;
  int totalBytesUp = 0;
  int totalBytesDown = 0;
  int activeCount = 0;
  String? llmComment;
  bool llmLoading = false;

  _AppGroup({required this.appDomain, required this.connections, required this.isProxied}) {
    for (final c in connections) {
      totalBytesUp += c.bytesUp;
      totalBytesDown += c.bytesDown;
      if (c.status == 'active') activeCount++;
    }
  }

  int get totalBytes => totalBytesUp + totalBytesDown;
  String get routeSummary {
    final relay = connections.where((c) => c.routeType == 'relay').length;
    final direct = connections.where((c) => c.routeType == 'direct').length;
    final parts = <String>[];
    if (relay > 0) parts.add('$relay relay');
    if (direct > 0) parts.add('$direct direct');
    return parts.join(', ');
  }
}

class ConnectionsScreen extends StatefulWidget {
  const ConnectionsScreen({super.key});

  @override
  State<ConnectionsScreen> createState() => _ConnectionsScreenState();
}

class _ConnectionsScreenState extends State<ConnectionsScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  Timer? _refreshTimer;
  List<ConnectionData> _connections = [];
  Map<String, dynamic>? _stats;
  bool _showClosed = false;
  final Map<String, String?> _llmCache = {};
  final Set<String> _llmLoading = {};

  @override
  void initState() {
    super.initState();
    _refresh();
    _refreshTimer = Timer.periodic(const Duration(seconds: 2), (_) => _refresh());
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    try {
      final connsJson = await getActiveConnections();
      final statsJson = await getConnectionStats();
      final conns = (jsonDecode(connsJson) as List<dynamic>)
          .map((e) => ConnectionData.fromJson(e as Map<String, dynamic>))
          .toList();
      final stats = jsonDecode(statsJson) as Map<String, dynamic>;
      if (mounted) setState(() { _connections = conns; _stats = stats; });
    } catch (_) {}
  }

  Future<void> _requestLlmComment(String domain, bool isProxied, int totalBytes) async {
    if (_llmLoading.contains(domain) || _llmCache.containsKey(domain)) return;
    setState(() { _llmLoading.add(domain); });
    try {
      final result = await analyzeHost(
        host: domain,
        port: 443,
        isProxied: isProxied,
        bytesTotal: BigInt.from(totalBytes),
      );
      if (mounted) {
        setState(() {
          _llmCache[domain] = result;
          _llmLoading.remove(domain);
        });
      }
    } catch (_) {
      if (mounted) setState(() { _llmLoading.remove(domain); });
    }
  }

  String _extractDomain(String host) {
    final parts = host.split('.');
    if (parts.length >= 2) {
      return parts.sublist(parts.length - 2).join('.');
    }
    return host;
  }

  List<_AppGroup> _buildGroups() {
    final displayed = _showClosed
        ? _connections
        : _connections.where((c) => c.status == 'active').toList();

    final Map<String, List<ConnectionData>> grouped = {};
    for (final c in displayed) {
      final domain = _extractDomain(c.targetHost);
      grouped.putIfAbsent(domain, () => []).add(c);
    }

    final groups = grouped.entries.map((e) {
      final isProxied = e.value.any((c) => c.isProxied);
      return _AppGroup(appDomain: e.key, connections: e.value, isProxied: isProxied);
    }).toList();

    groups.sort((a, b) {
      if (a.activeCount > 0 && b.activeCount == 0) return -1;
      if (a.activeCount == 0 && b.activeCount > 0) return 1;
      return b.totalBytes.compareTo(a.totalBytes);
    });
    return groups;
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final groups = _buildGroups();

    return Column(
      children: [
        if (_stats != null) _buildStatsBar(context),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          child: Row(
            children: [
              Text('${groups.length} apps', style: Theme.of(context).textTheme.titleMedium),
              const Spacer(),
              Text('Closed', style: Theme.of(context).textTheme.bodySmall),
              Switch(
                value: _showClosed,
                onChanged: (v) => setState(() { _showClosed = v; }),
              ),
            ],
          ),
        ),
        Expanded(
          child: groups.isEmpty
              ? _buildEmptyState()
              : RefreshIndicator(
                  onRefresh: _refresh,
                  child: ListView.builder(
                    itemCount: groups.length,
                    itemBuilder: (_, i) => _buildAppGroup(context, groups[i]),
                  ),
                ),
        ),
      ],
    );
  }

  Widget _buildStatsBar(BuildContext context) {
    final active = _stats!['active_count'] ?? 0;
    final proxied = _stats!['proxied_count'] ?? 0;
    final direct = active - proxied;
    final totalUp = _stats!['total_bytes_up'] ?? 0;
    final totalDown = _stats!['total_bytes_down'] ?? 0;

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: [
          _statChip('$active', 'Active', Colors.green),
          _statChip('$proxied', 'Relay', Colors.blue),
          _statChip('$direct', 'Direct', Colors.grey),
          _statChip(_fmt(totalUp), 'Up', Colors.teal),
          _statChip(_fmt(totalDown), 'Down', Colors.purple),
        ],
      ),
    );
  }

  Widget _buildAppGroup(BuildContext context, _AppGroup group) {
    final llmComment = _llmCache[group.appDomain];
    final llmLoading = _llmLoading.contains(group.appDomain);

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      color: group.isProxied ? Colors.blue.withValues(alpha: 0.06) : null,
      child: ExpansionTile(
        tilePadding: const EdgeInsets.symmetric(horizontal: 12),
        childrenPadding: EdgeInsets.zero,
        leading: _buildGroupIcon(group),
        title: Row(
          children: [
            Expanded(
              child: Text(
                group.appDomain,
                style: TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: group.activeCount > 0 ? null : Colors.grey,
                ),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            const SizedBox(width: 4),
            _badge(group.isProxied ? 'RELAY' : 'DIRECT',
                   group.isProxied ? Colors.blue : Colors.grey),
            if (group.connections.any((c) => c.isTelegram)) ...[
              const SizedBox(width: 4),
              _badge('TG', Colors.lightBlue),
            ],
          ],
        ),
        subtitle: Row(
          children: [
            Text('${group.connections.length} conn',
                 style: const TextStyle(fontSize: 10, color: Colors.grey)),
            const SizedBox(width: 8),
            Text(_fmt(group.totalBytes),
                 style: const TextStyle(fontSize: 10, color: Colors.grey)),
            const SizedBox(width: 8),
            Text(group.routeSummary,
                 style: const TextStyle(fontSize: 10, color: Colors.grey)),
          ],
        ),
        trailing: SizedBox(
          width: 40,
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text('${group.activeCount}', style: TextStyle(
                fontSize: 14,
                fontWeight: FontWeight.bold,
                color: group.activeCount > 0 ? Colors.green : Colors.grey,
              )),
              const Text('live', style: TextStyle(fontSize: 9, color: Colors.grey)),
            ],
          ),
        ),
        onExpansionChanged: (expanded) {
          if (expanded && !_llmCache.containsKey(group.appDomain)) {
            _requestLlmComment(group.appDomain, group.isProxied, group.totalBytes);
          }
        },
        children: [
          if (llmLoading)
            const Padding(
              padding: EdgeInsets.symmetric(horizontal: 16, vertical: 4),
              child: Row(
                children: [
                  SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 1.5)),
                  SizedBox(width: 8),
                  Text('Analyzing...', style: TextStyle(fontSize: 11, color: Colors.grey)),
                ],
              ),
            ),
          if (llmComment != null)
            Container(
              margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              padding: const EdgeInsets.all(8),
              decoration: BoxDecoration(
                color: _llmBgColor(llmComment),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(_llmIcon(llmComment), size: 14, color: _llmColor(llmComment)),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(llmComment,
                        style: TextStyle(fontSize: 11, color: _llmColor(llmComment))),
                  ),
                ],
              ),
            ),
          ...group.connections.map((conn) => _buildCompactConn(context, conn)),
          const SizedBox(height: 4),
        ],
      ),
    );
  }

  Widget _buildCompactConn(BuildContext context, ConnectionData conn) {
    final isActive = conn.status == 'active';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 1),
      child: Row(
        children: [
          Icon(
            isActive ? Icons.circle : Icons.circle_outlined,
            size: 6,
            color: isActive ? Colors.green : Colors.grey.shade600,
          ),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              '${conn.targetHost}:${conn.targetPort}',
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 10,
                color: isActive ? Colors.white70 : Colors.grey.shade600,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          _badge(conn.routeType.toUpperCase(),
                 conn.routeType == 'relay' ? Colors.blue : Colors.grey,
                 small: true),
          const SizedBox(width: 6),
          Text(conn.totalBytesFormatted,
               style: const TextStyle(fontSize: 9, color: Colors.grey)),
          const SizedBox(width: 6),
          Text(conn.durationFormatted,
               style: const TextStyle(fontSize: 9, color: Colors.grey)),
        ],
      ),
    );
  }

  Widget _buildGroupIcon(_AppGroup group) {
    if (group.connections.any((c) => c.isTelegram)) {
      return const Icon(Icons.send, color: Colors.lightBlue, size: 18);
    }
    if (group.isProxied) {
      return const Icon(Icons.cloud, color: Colors.blue, size: 18);
    }
    return Icon(Icons.language, color: Colors.grey.shade400, size: 18);
  }

  Widget _buildEmptyState() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.wifi_off, size: 48, color: Colors.grey.shade600),
          const SizedBox(height: 16),
          const Text('No active connections', style: TextStyle(color: Colors.grey)),
          const SizedBox(height: 8),
          const Text('Start VPN to see traffic', style: TextStyle(color: Colors.grey, fontSize: 12)),
        ],
      ),
    );
  }

  Widget _badge(String label, Color color, {bool small = false}) {
    return Container(
      padding: EdgeInsets.symmetric(horizontal: small ? 3 : 5, vertical: 0),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.2),
        borderRadius: BorderRadius.circular(3),
      ),
      child: Text(label, style: TextStyle(
        fontSize: small ? 8 : 9,
        color: color,
        fontWeight: FontWeight.bold,
      )),
    );
  }

  Widget _statChip(String value, String label, Color color) {
    return Column(
      children: [
        Text(value, style: TextStyle(fontWeight: FontWeight.bold, color: color, fontSize: 14)),
        Text(label, style: const TextStyle(fontSize: 9, color: Colors.grey)),
      ],
    );
  }

  Color _llmColor(String text) {
    if (text.contains('[ALERT]')) return Colors.red;
    if (text.contains('[WARN]')) return Colors.orange;
    return Colors.green;
  }

  Color _llmBgColor(String text) {
    if (text.contains('[ALERT]')) return Colors.red.withValues(alpha: 0.1);
    if (text.contains('[WARN]')) return Colors.orange.withValues(alpha: 0.1);
    return Colors.green.withValues(alpha: 0.08);
  }

  IconData _llmIcon(String text) {
    if (text.contains('[ALERT]')) return Icons.error_outline;
    if (text.contains('[WARN]')) return Icons.warning_amber;
    return Icons.check_circle_outline;
  }

  String _fmt(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    if (bytes < 1024 * 1024 * 1024) return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }
}
