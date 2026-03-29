import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  String _proxyMode = 'telegram';
  final List<String> _relayEndpoints = [];
  bool _cryptoEnabled = false;

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  Future<void> _loadSettings() async {
    final prefs = await SharedPreferences.getInstance();
    setState(() {
      _proxyMode = prefs.getString('proxy_mode') ?? 'telegram';
      _relayEndpoints.clear();
      _relayEndpoints.addAll(prefs.getStringList('relay_endpoints') ?? []);
      _cryptoEnabled = prefs.getBool('crypto_enabled') ?? false;
    });
  }

  Future<void> _saveProxyMode(String mode) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('proxy_mode', mode);
    setState(() { _proxyMode = mode; });
    try {
      await setProxyMode(mode: mode);
    } catch (e) {
      debugPrint("Failed to set proxy mode in Rust: $e");
    }
  }

  Future<void> _toggleCrypto(bool value) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('crypto_enabled', value);
    setState(() { _cryptoEnabled = value; });
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
          onChanged: (v) { if (v != null) _saveProxyMode(v); },
          child: Column(
            children: [
              _buildProxyModeRadio('off', 'Off', 'No proxying, direct connections only'),
              _buildProxyModeRadio('telegram', 'Telegram Only', 'Route only Telegram traffic through relay (default)'),
              _buildProxyModeRadio('full', 'Full VPN', 'Route all traffic through Hydra network'),
            ],
          ),
        ),

        const Divider(height: 32),
        Text('Relay Endpoints', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        if (_relayEndpoints.isEmpty)
          const Card(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'No relay endpoints configured.\nEndpoints will be discovered via P2P network.',
                style: TextStyle(color: Colors.grey),
              ),
            ),
          )
        else
          ...(_relayEndpoints.map((ep) => Card(
            child: ListTile(
              leading: const Icon(Icons.cloud_outlined),
              title: Text(ep, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
              trailing: const Icon(Icons.check_circle, color: Colors.green, size: 16),
            ),
          ))),

        const Divider(height: 32),
        Text('Crypto Settlement', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        SwitchListTile(
          title: const Text('Enable USDC Settlement'),
          subtitle: const Text('Arbitrum Sepolia testnet'),
          value: _cryptoEnabled,
          onChanged: _toggleCrypto,
        ),
        if (_cryptoEnabled) ...[
          const Card(
            child: Padding(
              padding: EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Wallet Status', style: TextStyle(fontWeight: FontWeight.bold)),
                  SizedBox(height: 8),
                  Text('Chain: Arbitrum Sepolia'),
                  Text('Balance: -- USDC (testnet)'),
                  SizedBox(height: 8),
                  Text(
                    'Configure Circle API key in hydra.toml [crypto] section.',
                    style: TextStyle(color: Colors.grey, fontSize: 12),
                  ),
                ],
              ),
            ),
          ),
        ],

        const Divider(height: 32),
        Text('About', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 8),
        const Card(
          child: Padding(
            padding: EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Hydra Network', style: TextStyle(fontWeight: FontWeight.bold)),
                SizedBox(height: 4),
                Text('Personal AI Agent with resilient connectivity'),
                SizedBox(height: 8),
                Text('Version: 0.2.0-mvp', style: TextStyle(color: Colors.grey, fontSize: 12)),
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
