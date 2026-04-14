import 'package:flutter/material.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:hydra_mobile/screens/models_screen.dart';
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

  static const _repository = MobileStateRepository();

  final TextEditingController _relayCostController = TextEditingController();
  String _proxyMode = 'full';
  bool _savingRelayCost = false;

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  @override
  void dispose() {
    _relayCostController.dispose();
    super.dispose();
  }

  Future<void> _loadSettings() async {
    final prefs = await SharedPreferences.getInstance();
    final costPerGb = await _repository.loadRelayCostPerGb();
    if (!mounted) {
      return;
    }
    setState(() {
      _proxyMode = prefs.getString('proxy_mode') ?? 'full';
      _relayCostController.text = costPerGb.toStringAsFixed(2);
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
      debugPrint('Failed to set proxy mode in Rust: $e');
    }
  }

  Future<void> _saveRelayCost() async {
    final value = double.tryParse(_relayCostController.text.trim());
    if (value == null || value < 0) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Enter a valid USD/GB rate.')),
      );
      return;
    }

    setState(() {
      _savingRelayCost = true;
    });
    await _repository.saveRelayCostPerGb(value);
    if (!mounted) {
      return;
    }
    setState(() {
      _savingRelayCost = false;
    });
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(const SnackBar(content: Text('Relay rate saved.')));
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text('Runtime Mode', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 10),
        Card(
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: RadioGroup<String>(
              groupValue: _proxyMode,
              onChanged: (selected) {
                if (selected != null) {
                  _saveProxyMode(selected);
                }
              },
              child: Column(
                children: [
                  _modeTile(
                    value: 'off',
                    title: 'Off',
                    subtitle: 'Keep Hydra running but do not proxy traffic.',
                  ),
                  _modeTile(
                    value: 'telegram',
                    title: 'Telegram Only',
                    subtitle:
                        'Route Telegram app traffic through enabled transports.',
                  ),
                  _modeTile(
                    value: 'full',
                    title: 'Full VPN',
                    subtitle:
                        'Route all traffic through policy-selected transports.',
                  ),
                ],
              ),
            ),
          ),
        ),
        const SizedBox(height: 24),
        Text('Relay Estimate', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 10),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Cloudflare billing is not pulled from the API in v1. Set a local USD per GB rate to turn relay usage into an estimate.',
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _relayCostController,
                  keyboardType: const TextInputType.numberWithOptions(
                    decimal: true,
                  ),
                  decoration: const InputDecoration(
                    labelText: 'Estimated USD / GB',
                    hintText: '0.12',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                FilledButton.icon(
                  onPressed: _savingRelayCost ? null : _saveRelayCost,
                  icon: _savingRelayCost
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.save_outlined),
                  label: const Text('Save Estimate'),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 24),
        Text('Optional AI', style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 10),
        Card(
          child: ListTile(
            leading: const Icon(Icons.smart_toy_outlined),
            title: const Text('Manage local models'),
            subtitle: const Text(
              'Qwen 3.5 (0.8B) is bundled for on-device analysis. You can still download lighter or larger local models here.',
            ),
            trailing: const Icon(Icons.chevron_right),
            onTap: () {
              Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => Scaffold(
                    appBar: AppBar(title: const Text('Optional AI')),
                    body: const ModelsScreen(),
                  ),
                ),
              );
            },
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
                  'Hydra Network',
                  style: TextStyle(fontWeight: FontWeight.w700),
                ),
                SizedBox(height: 6),
                Text(
                  'Android MVP focused on WSS relay and imported VLESS routing.',
                ),
                SizedBox(height: 10),
                Text(
                  'Marketplace, payments, providers, and content surfaces remain in the repository but are intentionally hidden from the primary release UI.',
                  style: TextStyle(fontSize: 12, color: Colors.grey),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _modeTile({
    required String value,
    required String title,
    required String subtitle,
  }) {
    return ListTile(
      leading: Radio<String>(value: value),
      title: Text(title),
      subtitle: Text(subtitle),
      onTap: () => _saveProxyMode(value),
    );
  }
}
