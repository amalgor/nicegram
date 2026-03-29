import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/widgets/connection_tile.dart';

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
  bool _showClosedConnections = false;

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

      if (mounted) {
        setState(() {
          _connections = conns;
          _stats = stats;
        });
      }
    } catch (e) {
      // Node not started yet — silently ignore
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);

    final displayed = _showClosedConnections
        ? _connections
        : _connections.where((c) => c.status == 'active').toList();

    displayed.sort((a, b) {
      if (a.status == 'active' && b.status != 'active') return -1;
      if (a.status != 'active' && b.status == 'active') return 1;
      return b.durationMs.compareTo(a.durationMs);
    });

    return Column(
      children: [
        if (_stats != null) _buildStatsBar(context),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Connections',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              Row(
                children: [
                  Text(
                    'Show closed',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  Switch(
                    value: _showClosedConnections,
                    onChanged: (v) => setState(() { _showClosedConnections = v; }),
                  ),
                ],
              ),
            ],
          ),
        ),
        Expanded(
          child: displayed.isEmpty
              ? Center(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(Icons.wifi_off, size: 48, color: Colors.grey.shade600),
                      const SizedBox(height: 16),
                      const Text('No active connections', style: TextStyle(color: Colors.grey)),
                      const SizedBox(height: 8),
                      const Text(
                        'Start VPN to see traffic here',
                        style: TextStyle(color: Colors.grey, fontSize: 12),
                      ),
                    ],
                  ),
                )
              : RefreshIndicator(
                  onRefresh: _refresh,
                  child: ListView.builder(
                    itemCount: displayed.length,
                    itemBuilder: (context, index) {
                      final conn = displayed[index];
                      return ConnectionTile(
                        conn: conn,
                        onProxyToggle: conn.status == 'active'
                            ? (proxied) async {
                                try {
                                  await setConnectionProxy(
                                    connId: conn.id,
                                    proxied: proxied,
                                  );
                                  await _refresh();
                                } catch (e) {
                                  if (mounted) {
                                    ScaffoldMessenger.of(context).showSnackBar(
                                      SnackBar(content: Text('Error: $e')),
                                    );
                                  }
                                }
                              }
                            : null,
                      );
                    },
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
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: [
          _statChip(context, '$active', 'Active', Colors.green),
          _statChip(context, '$proxied', 'Proxied', Colors.blue),
          _statChip(context, '$direct', 'Direct', Colors.grey),
          _statChip(context, _formatBytes(totalUp + totalDown), 'Total', Colors.purple),
        ],
      ),
    );
  }

  Widget _statChip(BuildContext context, String value, String label, Color color) {
    return Column(
      children: [
        Text(value, style: TextStyle(fontWeight: FontWeight.bold, color: color, fontSize: 16)),
        Text(label, style: const TextStyle(fontSize: 10, color: Colors.grey)),
      ],
    );
  }

  String _formatBytes(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
}
