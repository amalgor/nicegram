import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  int _port = 1080;

  @override
  void initState() {
    super.initState();
    _port = simple_api.getSocks5Port();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('Proxy', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 10),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'SOCKS5 listen port: $_port',
                  style: Theme.of(context).textTheme.bodyLarge,
                ),
                const SizedBox(height: 8),
                Text(
                  'Port is configured in hydra.toml ([network] socks5_port). Restart the app to apply changes.',
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Colors.grey,
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 24),
        Text('About', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 10),
        const Card(
          child: Padding(
            padding: EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Hydra Proxy',
                  style: TextStyle(fontWeight: FontWeight.w700),
                ),
                SizedBox(height: 6),
                Text(
                  'Local SOCKS5 proxy with VLESS and SSH tunnel support. '
                  'Configure your browser or app to use this proxy to route '
                  'traffic through your tunnels.',
                ),
                SizedBox(height: 10),
                Text(
                  'No VPN required. Other apps see no proxy activity.',
                  style: TextStyle(fontSize: 12, color: Colors.grey),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}
