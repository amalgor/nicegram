import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/src/rust/api/vpn.dart';
import 'package:hydra_mobile/widgets/quota_widget.dart';

bool gIsVpnActive = false;

class ConnectScreen extends StatefulWidget {
  const ConnectScreen({super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  static const platform = MethodChannel('com.hydra.network/vpn');
  Map<String, dynamic>? _stats;
  Timer? _statsTimer;

  @override
  void initState() {
    super.initState();
    platform.setMethodCallHandler((call) async {
      if (call.method == 'onVpnStarted') {
        final fd = call.arguments as int;
        if (fd != -1) {
          try {
            startVpnTunnel(fd: fd);
            debugPrint("Rust VPN tunnel started on FD: $fd");
          } catch (e) {
            debugPrint("Failed to start Rust VPN tunnel: $e");
          }
        }
      }
    });
    _statsTimer = Timer.periodic(const Duration(seconds: 3), (_) => _refreshStats());
  }

  @override
  void dispose() {
    _statsTimer?.cancel();
    super.dispose();
  }

  Future<void> _refreshStats() async {
    if (!gIsVpnActive) return;
    try {
      final json = await getConnectionStats();
      if (mounted) {
        setState(() { _stats = jsonDecode(json) as Map<String, dynamic>; });
      }
    } catch (_) {}
  }

  void _toggleVpn() async {
    try {
      if (gIsVpnActive) {
        await platform.invokeMethod('stopVpn');
        stopVpnTunnel();
        setState(() { gIsVpnActive = false; _stats = null; });
      } else {
        // Node is started at app init (main.dart). Calling again is safe (idempotent).
        final dir = await getApplicationDocumentsDirectory();
        await startHydraNode(baseDir: dir.path);
        final bool? result = await platform.invokeMethod('startVpn');
        if (result == true) {
          setState(() { gIsVpnActive = true; });
        }
      }
    } on PlatformException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('VPN Error: ${e.message}')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Error: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 200,
            height: 200,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: gIsVpnActive
                  ? Colors.green.withValues(alpha: 0.2)
                  : Theme.of(context).colorScheme.primaryContainer,
            ),
            child: IconButton(
              iconSize: 100,
              icon: Icon(
                gIsVpnActive ? Icons.power_settings_new : Icons.power_settings_new_outlined,
                color: gIsVpnActive
                    ? Colors.green
                    : Theme.of(context).colorScheme.onPrimaryContainer,
              ),
              onPressed: _toggleVpn,
            ),
          ),
          const SizedBox(height: 32),
          Text(
            gIsVpnActive ? 'Connected' : 'Disconnected',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 16),
          Text(
            gIsVpnActive
                ? 'Telegram traffic routed through Hydra relay.\nOther connections pass through directly.'
                : 'Tap to start Hydra network.',
            textAlign: TextAlign.center,
          ),
          if (gIsVpnActive) ...[
            const SizedBox(height: 24),
            if (_stats != null) _buildConnectionSummary(context),
            const SizedBox(height: 16),
            const QuotaWidget(),
          ],
        ],
      ),
    );
  }

  Widget _buildConnectionSummary(BuildContext context) {
    final active = _stats!['active_count'] ?? 0;
    final proxied = _stats!['proxied_count'] ?? 0;
    final direct = active - proxied;

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _miniStat('$active', 'active', Colors.green),
          const SizedBox(width: 24),
          _miniStat('$proxied', 'via relay', Colors.blue),
          const SizedBox(width: 24),
          _miniStat('$direct', 'direct', Colors.grey),
        ],
      ),
    );
  }

  Widget _miniStat(String value, String label, Color color) {
    return Column(
      children: [
        Text(value, style: TextStyle(fontWeight: FontWeight.bold, color: color, fontSize: 18)),
        Text(label, style: const TextStyle(fontSize: 10, color: Colors.grey)),
      ],
    );
  }
}
