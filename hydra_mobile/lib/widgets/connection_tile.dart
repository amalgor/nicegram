import 'package:flutter/material.dart';

class ConnectionData {
  final int id;
  final String targetHost;
  final int targetPort;
  final String routeType;
  final int bytesUp;
  final int bytesDown;
  final int durationMs;
  final bool isTelegram;
  final bool isProxied;
  final String status;
  final String? aiReason;

  ConnectionData({
    required this.id,
    required this.targetHost,
    required this.targetPort,
    required this.routeType,
    required this.bytesUp,
    required this.bytesDown,
    required this.durationMs,
    required this.isTelegram,
    required this.isProxied,
    required this.status,
    this.aiReason,
  });

  factory ConnectionData.fromJson(Map<String, dynamic> json) {
    return ConnectionData(
      id: json['id'] as int,
      targetHost: json['target_host'] as String,
      targetPort: json['target_port'] as int,
      routeType: json['route_type'] as String,
      bytesUp: json['bytes_up'] as int,
      bytesDown: json['bytes_down'] as int,
      durationMs: json['duration_ms'] as int,
      isTelegram: json['is_telegram'] as bool,
      isProxied: json['is_proxied'] as bool,
      status: json['status'] as String,
      aiReason: json['ai_reason'] as String?,
    );
  }

  String get totalBytesFormatted {
    final total = bytesUp + bytesDown;
    if (total < 1024) return '$total B';
    if (total < 1024 * 1024) return '${(total / 1024).toStringAsFixed(1)} KB';
    return '${(total / (1024 * 1024)).toStringAsFixed(1)} MB';
  }

  String get durationFormatted {
    if (durationMs < 1000) return '${durationMs}ms';
    if (durationMs < 60000) return '${(durationMs / 1000).toStringAsFixed(0)}s';
    return '${(durationMs / 60000).toStringAsFixed(1)}m';
  }
}

class ConnectionTile extends StatelessWidget {
  final ConnectionData conn;
  final ValueChanged<bool>? onProxyToggle;

  const ConnectionTile({
    super.key,
    required this.conn,
    this.onProxyToggle,
  });

  @override
  Widget build(BuildContext context) {
    final isActive = conn.status == 'active';

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      color: conn.isTelegram
          ? Colors.blue.withValues(alpha: 0.08)
          : null,
      child: ExpansionTile(
        leading: _buildLeadingIcon(context),
        title: Text(
          conn.targetHost,
          style: TextStyle(
            fontFamily: 'monospace',
            fontSize: 13,
            color: isActive ? null : Colors.grey,
          ),
          overflow: TextOverflow.ellipsis,
        ),
        subtitle: Row(
          children: [
            _buildRouteBadge(context),
            const SizedBox(width: 8),
            Text(
              conn.totalBytesFormatted,
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(width: 8),
            Text(
              conn.durationFormatted,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Colors.grey,
              ),
            ),
            if (conn.isTelegram) ...[
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
                decoration: BoxDecoration(
                  color: Colors.blue.withValues(alpha: 0.2),
                  borderRadius: BorderRadius.circular(4),
                ),
                child: const Text('TG', style: TextStyle(fontSize: 10, color: Colors.blue)),
              ),
            ],
          ],
        ),
        trailing: isActive && onProxyToggle != null
            ? Switch(
                value: conn.isProxied,
                onChanged: onProxyToggle,
                activeTrackColor: Colors.green.withValues(alpha: 0.5),
                activeThumbColor: Colors.green,
              )
            : Icon(
                isActive ? Icons.circle : Icons.circle_outlined,
                size: 10,
                color: isActive ? Colors.green : Colors.grey,
              ),
        children: [
          if (conn.aiReason != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              child: Row(
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                    decoration: BoxDecoration(
                      color: Colors.purple.withValues(alpha: 0.2),
                      borderRadius: BorderRadius.circular(4),
                    ),
                    child: const Text('AI', style: TextStyle(fontSize: 10, color: Colors.purple, fontWeight: FontWeight.bold)),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      conn.aiReason!,
                      style: const TextStyle(fontSize: 12, color: Colors.grey),
                    ),
                  ),
                ],
              ),
            ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text('Port: ${conn.targetPort}', style: const TextStyle(fontSize: 11, color: Colors.grey)),
                Text('Up: ${_formatBytes(conn.bytesUp)}', style: const TextStyle(fontSize: 11, color: Colors.grey)),
                Text('Down: ${_formatBytes(conn.bytesDown)}', style: const TextStyle(fontSize: 11, color: Colors.grey)),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildLeadingIcon(BuildContext context) {
    IconData icon;
    Color color;
    switch (conn.routeType) {
      case 'relay':
        icon = Icons.cloud;
        color = Colors.blue;
        break;
      case 'p2p':
        icon = Icons.hub;
        color = Colors.orange;
        break;
      default:
        icon = Icons.arrow_forward;
        color = Colors.grey;
    }
    return Icon(icon, color: color, size: 20);
  }

  Widget _buildRouteBadge(BuildContext context) {
    Color badgeColor;
    String label;
    switch (conn.routeType) {
      case 'relay':
        badgeColor = Colors.blue;
        label = 'RELAY';
        break;
      case 'p2p':
        badgeColor = Colors.orange;
        label = 'P2P';
        break;
      default:
        badgeColor = Colors.grey;
        label = 'DIRECT';
    }
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: badgeColor.withValues(alpha: 0.2),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(
        label,
        style: TextStyle(fontSize: 10, color: badgeColor, fontWeight: FontWeight.bold),
      ),
    );
  }

  String _formatBytes(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
}
