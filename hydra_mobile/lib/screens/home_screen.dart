import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/native_bridge.dart';
import 'package:hydra_mobile/app/proxy_controller.dart';
import 'package:hydra_mobile/app/proxy_scope.dart';
import 'package:hydra_mobile/screens/server_editor_screen.dart';

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key, required this.onOpenServers});

  final VoidCallback onOpenServers;

  @override
  Widget build(BuildContext context) {
    final controller = ProxyScope.of(context);
    final server = controller.activeServer;
    final status = controller.status;
    final error = controller.error;

    return Scaffold(
      appBar: AppBar(title: const Text('Hydra Proxy')),
      body: RefreshIndicator(
        onRefresh: controller.refreshServers,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
          children: [
            _StatusCard(controller: controller),
            if (error != null) ...[
              const SizedBox(height: 12),
              _ErrorCard(message: error),
            ],
            const SizedBox(height: 12),
            if (server == null)
              _EmptyServerCard(onAdd: () => _addServer(context))
            else
              _ServerCard(server: server, onTap: onOpenServers),
            if (status.running) ...[
              const SizedBox(height: 12),
              _AddressCard(address: status.address, port: status.port),
              const SizedBox(height: 12),
              _StatsCard(status: status),
            ],
            if (!controller.settings.keepAliveInBackground) ...[
              const SizedBox(height: 12),
              const _HintCard(
                icon: Icons.bedtime_outlined,
                text: 'Background keep-alive is off. iOS suspends the proxy a few seconds after '
                    'you switch to another app. Turn it on in Settings.',
              ),
            ],
          ],
        ),
      ),
    );
  }

  Future<void> _addServer(BuildContext context) =>
      Navigator.of(context).push(MaterialPageRoute(builder: (_) => const ServerEditorScreen()));
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({required this.controller});

  final ProxyController controller;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final text = Theme.of(context).textTheme;
    final phase = controller.phase;
    final (label, detail, color, icon) = switch (phase) {
      ProxyPhase.connected => ('Connected', 'Proxy is ready to use', Colors.green, Icons.verified_user),
      ProxyPhase.connecting => ('Connecting…', 'Opening the SSH session', Colors.orange, Icons.sync),
      ProxyPhase.degraded => ('Reconnecting', 'SSH session failed, retrying on next request', Colors.orange, Icons.sync_problem),
      ProxyPhase.starting => ('Starting…', 'Launching the local proxy', scheme.primary, Icons.hourglass_top),
      ProxyPhase.stopping => ('Stopping…', '', scheme.outline, Icons.hourglass_bottom),
      ProxyPhase.failed => ('Not running', 'The proxy could not start', scheme.error, Icons.error_outline),
      ProxyPhase.stopped => ('Off', 'Tap Start to run the proxy', scheme.outline, Icons.shield_outlined),
    };
    final running = controller.status.running;
    final canStart = controller.activeServer != null;

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          children: [
            Semantics(
              label: 'Proxy status $label',
              child: CircleAvatar(
                radius: 36,
                backgroundColor: color.withValues(alpha: 0.15),
                child: Icon(icon, size: 36, color: color),
              ),
            ),
            const SizedBox(height: 12),
            Text(label, style: text.headlineSmall),
            if (detail.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(detail, style: text.bodyMedium?.copyWith(color: scheme.onSurfaceVariant), textAlign: TextAlign.center),
            ],
            const SizedBox(height: 20),
            SizedBox(
              width: double.infinity,
              height: 52,
              child: controller.busy
                  ? const FilledButton(onPressed: null, child: SizedBox.square(dimension: 22, child: CircularProgressIndicator(strokeWidth: 2.5)))
                  : running
                      ? FilledButton.tonalIcon(onPressed: controller.stop, icon: const Icon(Icons.stop), label: const Text('Stop'))
                      : FilledButton.icon(
                          onPressed: canStart ? controller.start : null,
                          icon: const Icon(Icons.play_arrow),
                          label: Text(canStart ? 'Start' : 'Add a server first'),
                        ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ErrorCard extends StatelessWidget {
  const _ErrorCard({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Card(
      color: scheme.errorContainer,
      child: ListTile(
        leading: Icon(Icons.warning_amber_rounded, color: scheme.onErrorContainer),
        title: Text('Last error', style: TextStyle(color: scheme.onErrorContainer, fontWeight: FontWeight.w600)),
        subtitle: SelectableText(message, style: TextStyle(color: scheme.onErrorContainer)),
        trailing: IconButton(
          tooltip: 'Copy error',
          icon: Icon(Icons.copy, color: scheme.onErrorContainer),
          onPressed: () => _copy(context, message, 'Error copied'),
        ),
      ),
    );
  }
}

class _EmptyServerCard extends StatelessWidget {
  const _EmptyServerCard({required this.onAdd});

  final VoidCallback onAdd;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Add your SSH server', style: text.titleMedium),
            const SizedBox(height: 6),
            Text(
              'Hydra works like `ssh -D 1080 user@host`: apps on this phone use the SOCKS5 proxy '
              'at 127.0.0.1:1080 and traffic leaves through your server.',
              style: text.bodyMedium,
            ),
            const SizedBox(height: 16),
            FilledButton.icon(onPressed: onAdd, icon: const Icon(Icons.add), label: const Text('Add server')),
          ],
        ),
      ),
    );
  }
}

class _ServerCard extends StatelessWidget {
  const _ServerCard({required this.server, required this.onTap});

  final ServerInfo server;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final ssh = ProxyScope.of(context).status.activeSsh;
    final subtitle = [
      server.endpoint,
      if (ssh?.connectedSince != null) 'up ${formatDuration(DateTime.now().difference(ssh!.connectedSince!))}',
    ].join(' · ');
    return Card(
      child: ListTile(
        leading: const Icon(Icons.dns_outlined),
        title: Text(server.label),
        subtitle: Text(subtitle),
        trailing: const Icon(Icons.chevron_right),
        onTap: onTap,
      ),
    );
  }
}

class _AddressCard extends StatelessWidget {
  const _AddressCard({required this.address, required this.port});

  final String address;
  final int port;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('SOCKS5 proxy', style: text.labelLarge),
                      const SizedBox(height: 2),
                      SelectableText(address, style: text.titleLarge?.copyWith(fontFamily: 'Menlo')),
                    ],
                  ),
                ),
                IconButton(
                  tooltip: 'Copy address',
                  icon: const Icon(Icons.copy),
                  onPressed: () => _copy(context, address, 'Proxy address copied'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                OutlinedButton.icon(
                  icon: const Icon(Icons.send),
                  label: const Text('Use in Telegram'),
                  onPressed: () async {
                    final ok = await NativeBridge.instance.openUrl('tg://socks?server=127.0.0.1&port=$port');
                    if (!ok && context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Telegram is not installed')));
                    }
                  },
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _StatsCard extends StatelessWidget {
  const _StatsCard({required this.status});

  final ProxyStatus status;

  @override
  Widget build(BuildContext context) {
    final ssh = status.activeSsh;
    final uptime = status.startedAt == null ? '—' : formatDuration(DateTime.now().difference(status.startedAt!));
    return Card(
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 8),
        child: Row(
          children: [
            _Stat(label: 'Active', value: '${status.connectionsActive}'),
            _Stat(label: 'Total', value: '${status.connectionsTotal}'),
            _Stat(label: 'Sent', value: formatBytes(ssh?.bytesUp ?? 0)),
            _Stat(label: 'Received', value: formatBytes(ssh?.bytesDown ?? 0)),
            _Stat(label: 'Uptime', value: uptime),
          ],
        ),
      ),
    );
  }
}

class _Stat extends StatelessWidget {
  const _Stat({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Expanded(
      child: Column(
        children: [
          FittedBox(child: Text(value, style: text.titleMedium)),
          const SizedBox(height: 2),
          Text(label, style: text.labelSmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
        ],
      ),
    );
  }
}

class _HintCard extends StatelessWidget {
  const _HintCard({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Card(
      color: Theme.of(context).colorScheme.secondaryContainer,
      child: ListTile(leading: Icon(icon), title: Text(text, style: Theme.of(context).textTheme.bodyMedium)),
    );
  }
}

void _copy(BuildContext context, String value, String message) {
  Clipboard.setData(ClipboardData(text: value));
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message), duration: const Duration(seconds: 1)));
}
