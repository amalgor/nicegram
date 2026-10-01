import 'package:flutter/material.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/proxy_scope.dart';
import 'package:hydra_mobile/screens/server_editor_screen.dart';

class ServersScreen extends StatelessWidget {
  const ServersScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final controller = ProxyScope.of(context);
    final servers = controller.sshServers;
    return Scaffold(
      appBar: AppBar(title: const Text('Servers')),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _open(context, null),
        icon: const Icon(Icons.add),
        label: const Text('Add server'),
      ),
      body: servers.isEmpty
          ? const Center(
              child: Padding(
                padding: EdgeInsets.all(32),
                child: Text(
                  'No SSH servers yet.\nAdd one to start using the proxy.',
                  textAlign: TextAlign.center,
                ),
              ),
            )
          : RefreshIndicator(
              onRefresh: controller.refreshServers,
              child: ListView.separated(
                padding: const EdgeInsets.only(bottom: 96),
                itemCount: servers.length,
                separatorBuilder: (_, __) => const Divider(height: 1, indent: 72),
                itemBuilder: (context, i) => _ServerTile(server: servers[i]),
              ),
            ),
    );
  }
}

class _ServerTile extends StatelessWidget {
  const _ServerTile({required this.server});

  final ServerInfo server;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ListTile(
      leading: IconButton(
        tooltip: server.active ? 'Active server' : 'Use this server',
        icon: Icon(
          server.active ? Icons.radio_button_checked : Icons.radio_button_unchecked,
          color: server.active ? scheme.primary : scheme.outline,
        ),
        onPressed: server.active ? null : () => _activate(context),
      ),
      title: Text(server.label),
      subtitle: Text([
        server.endpoint,
        server.authType == 'password' ? 'password' : 'key',
        if (!server.hasCredential) 'no credential',
      ].join(' · ')),
      trailing: const Icon(Icons.chevron_right),
      onTap: () => _open(context, server),
    );
  }

  Future<void> _activate(BuildContext context) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      await ProxyScope.read(context).activate(server);
      messenger.showSnackBar(SnackBar(content: Text('Using ${server.label}')));
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(describeError(e))));
    }
  }
}

Future<void> _open(BuildContext context, ServerInfo? server) => Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => ServerEditorScreen(server: server)),
    );
