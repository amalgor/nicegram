import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/native_bridge.dart';
import 'package:hydra_mobile/app/proxy_scope.dart';
import 'package:hydra_mobile/screens/routes_screen.dart';
import 'package:hydra_mobile/screens/terminal_screen.dart';
import 'package:hydra_mobile/src/rust/api/diagnostics.dart' as diagnostics_api;

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  Map<String, Object?> _device = const {};

  @override
  void initState() {
    super.initState();
    NativeBridge.instance.deviceInfo().then((info) {
      if (mounted) setState(() => _device = info);
    });
  }

  @override
  Widget build(BuildContext context) {
    final controller = ProxyScope.of(context);
    final settings = controller.settings;
    final logDir = diagnostics_api.logDirectory();
    final version = _device['appVersion'] == null ? null : '${_device['appVersion']} (${_device['appBuild']})';

    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        children: [
          const _Header('Proxy'),
          SwitchListTile(
            secondary: const Icon(Icons.bedtime_outlined),
            title: const Text('Keep running in background'),
            subtitle: const Text(
              'Plays silent audio so iOS does not suspend the proxy while you use other apps. '
              'Uses some battery.',
            ),
            value: settings.keepAliveInBackground,
            onChanged: controller.setKeepAliveInBackground,
          ),
          SwitchListTile(
            secondary: const Icon(Icons.play_circle_outline),
            title: const Text('Start on launch'),
            subtitle: const Text('Start the proxy when the app opens and a server is selected'),
            value: settings.autoStart,
            onChanged: controller.setAutoStart,
          ),
          ListTile(
            leading: const Icon(Icons.lan_outlined),
            title: const Text('SOCKS5 address'),
            subtitle: Text('${controller.status.address}\nPort is set in hydra.toml ([network] socks5_port)'),
            isThreeLine: true,
          ),
          const _Header('Diagnostics'),
          ListTile(
            leading: const Icon(Icons.folder_outlined),
            title: const Text('Log folder'),
            subtitle: Text(logDir ?? 'File logging unavailable', maxLines: 2, overflow: TextOverflow.ellipsis),
            trailing: logDir == null
                ? null
                : IconButton(
                    tooltip: 'Copy path',
                    icon: const Icon(Icons.copy),
                    onPressed: () => Clipboard.setData(ClipboardData(text: logDir)),
                  ),
          ),
          ListTile(
            leading: const Icon(Icons.network_check),
            title: const Text('Network'),
            subtitle: Text(controller.network?.toString() ?? 'Unknown'),
          ),
          const _Header('Advanced'),
          ListTile(
            leading: const Icon(Icons.route),
            title: const Text('Routes'),
            subtitle: const Text('All route profiles, including built-in relays'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _push(context, 'Routes', const RoutesScreen()),
          ),
          ListTile(
            leading: const Icon(Icons.terminal),
            title: const Text('Network shell'),
            subtitle: const Text('Diagnostic commands on the device'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _push(context, 'Network shell', const TerminalScreen()),
          ),
          const _Header('About'),
          ListTile(
            leading: const Icon(Icons.info_outline),
            title: const Text('Hydra Proxy'),
            subtitle: Text([
              if (version != null) 'Version $version',
              if (_device['model'] != null) '${_device['model']} · ${_device['system']}',
            ].join('\n')),
          ),
          const SizedBox(height: 24),
        ],
      ),
    );
  }

  void _push(BuildContext context, String title, Widget child) {
    final controller = ProxyScope.read(context);
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => Scaffold(appBar: AppBar(title: Text(title)), body: child),
    )).then((_) => controller.refreshServers());
  }
}

class _Header extends StatelessWidget {
  const _Header(this.title);

  final String title;

  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 24, 16, 4),
        child: Text(title, style: Theme.of(context).textTheme.titleSmall?.copyWith(color: Theme.of(context).colorScheme.primary)),
      );
}
