import 'package:flutter/material.dart';
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:shared_preferences/shared_preferences.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  String _proxyMode = 'telegram';

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  Future<void> _loadSettings() async {
    final prefs = await SharedPreferences.getInstance();
    setState(() {
      _proxyMode = prefs.getString('proxy_mode') ?? 'telegram';
    });
  }

  Future<void> _saveProxyMode(String mode) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('proxy_mode', mode);
    setState(() {
      _proxyMode = mode;
    });
    try {
      await HydraPlatformGateway.instance.setProxyMode(mode: mode);
    } catch (e) {
      debugPrint("Failed to set proxy mode in Rust: $e");
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('Proxy Mode', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        RadioGroup<String>(
          groupValue: _proxyMode,
          onChanged: (v) {
            if (v != null) _saveProxyMode(v);
          },
          child: Column(
            children: [
              _buildProxyModeRadio(
                'off',
                'Off',
                'No proxying, direct connections only',
              ),
              _buildProxyModeRadio(
                'telegram',
                'Telegram Only',
                'Route only Telegram traffic through configured transports',
              ),
              _buildProxyModeRadio(
                'full',
                'Full VPN',
                'Route all traffic through Hydra network',
              ),
            ],
          ),
        ),
        const Divider(height: 32),
        Text(
          'Marketplace',
          style: Theme.of(context).textTheme.titleLarge,
        ),
        const SizedBox(height: 8),
        const Card(
          child: Padding(
            padding: EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Hydra Route Exchange',
                  style: TextStyle(fontWeight: FontWeight.bold),
                ),
                SizedBox(height: 8),
                Text('Chain: Base Sepolia'),
                Text('Wallet custody: local secure storage'),
                SizedBox(height: 8),
                Text(
                  'Configure contract addresses and RPC endpoint in hydra.toml [crypto]. Wallet creation, agent registration, offers, and feedback live in the Marketplace tab.',
                  style: TextStyle(color: Colors.grey, fontSize: 12),
                ),
              ],
            ),
          ),
        ),

        const Divider(height: 32),
        Text('About', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        const Card(
          child: Padding(
            padding: EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Hydra Network',
                  style: TextStyle(fontWeight: FontWeight.bold),
                ),
                SizedBox(height: 4),
                Text('Personal AI Agent with resilient connectivity'),
                SizedBox(height: 8),
                Text(
                  'Version: 0.2.0-mvp',
                  style: TextStyle(color: Colors.grey, fontSize: 12),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildProxyModeRadio(String value, String title, String subtitle) {
    return ListTile(
      leading: Radio<String>(value: value),
      title: Text(title),
      subtitle: Text(subtitle, style: const TextStyle(fontSize: 12)),
      onTap: () => _saveProxyMode(value),
    );
  }
}
