import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
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
  Duration _uptime = Duration.zero;
  DateTime? _connectedAt;

  String? _llmAnalysis;
  bool _llmLoading = false;
  Timer? _llmTimer;

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
    _statsTimer = Timer.periodic(const Duration(seconds: 2), (_) {
      _refreshStats();
      _updateUptime();
    });
    _autoStartVpn();
  }

  Future<void> _autoStartVpn() async {
    // Wait for Hydra node to be ready (started in main.dart)
    await Future.delayed(const Duration(seconds: 3));
    if (gIsVpnActive) return;
    debugPrint("Auto-starting VPN...");
    try {
      final bool? result = await platform.invokeMethod('startVpn');
      if (result == true && mounted) {
        setState(() {
          gIsVpnActive = true;
          _connectedAt = DateTime.now();
        });
        _llmTimer = Timer.periodic(const Duration(seconds: 30), (_) => _requestLlmAnalysis());
        Future.delayed(const Duration(seconds: 5), _requestLlmAnalysis);
      }
    } on PlatformException catch (e) {
      debugPrint("VPN auto-start failed (may need user consent): ${e.message}");
    } catch (e) {
      debugPrint("VPN auto-start error: $e");
    }
  }

  @override
  void dispose() {
    _statsTimer?.cancel();
    _llmTimer?.cancel();
    super.dispose();
  }

  void _updateUptime() {
    if (_connectedAt != null && gIsVpnActive && mounted) {
      setState(() { _uptime = DateTime.now().difference(_connectedAt!); });
    }
  }

  Future<void> _refreshStats() async {
    if (!gIsVpnActive) return;
    try {
      final json = await getConnectionStats();
      if (mounted) setState(() { _stats = jsonDecode(json) as Map<String, dynamic>; });
    } catch (_) {}
  }

  Future<void> _requestLlmAnalysis() async {
    if (_llmLoading) return;
    setState(() { _llmLoading = true; });
    try {
      final connsJson = await getActiveConnections();
      final result = await analyzeConnections(connectionsJson: connsJson);
      if (mounted) setState(() { _llmAnalysis = result; _llmLoading = false; });
    } catch (e) {
      if (mounted) {
        setState(() {
          _llmAnalysis = '[OK] Analysis unavailable: $e';
          _llmLoading = false;
        });
      }
    }
  }

  void _toggleVpn() async {
    try {
      if (gIsVpnActive) {
        await platform.invokeMethod('stopVpn');
        stopVpnTunnel();
        _llmTimer?.cancel();
        setState(() {
          gIsVpnActive = false;
          _stats = null;
          _connectedAt = null;
          _uptime = Duration.zero;
          _llmAnalysis = null;
        });
      } else {
        final bool? result = await platform.invokeMethod('startVpn');
        if (result == true) {
          setState(() {
            gIsVpnActive = true;
            _connectedAt = DateTime.now();
          });
          _llmTimer = Timer.periodic(const Duration(seconds: 30), (_) => _requestLlmAnalysis());
          Future.delayed(const Duration(seconds: 5), _requestLlmAnalysis);
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
    return SingleChildScrollView(
      child: Center(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
          child: Column(
            children: [
              const SizedBox(height: 16),
              _buildPowerButton(context),
              const SizedBox(height: 20),
              Text(
                gIsVpnActive ? 'Connected' : 'Disconnected',
                style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                  color: gIsVpnActive ? Colors.green : null,
                ),
              ),
              if (gIsVpnActive && _uptime.inSeconds > 0) ...[
                const SizedBox(height: 4),
                Text(_fmtDuration(_uptime),
                     style: const TextStyle(fontSize: 12, color: Colors.grey)),
              ],
              if (!gIsVpnActive)
                const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Text('Tap to start Hydra network.', textAlign: TextAlign.center,
                       style: TextStyle(color: Colors.grey)),
                ),
              if (gIsVpnActive) ...[
                const SizedBox(height: 20),
                _buildStatsGrid(context),
                const SizedBox(height: 16),
                _buildTrafficBar(context),
                const SizedBox(height: 16),
                const QuotaWidget(),
                const SizedBox(height: 16),
                _buildLlmCard(context),
              ],
              const SizedBox(height: 24),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildPowerButton(BuildContext context) {
    return GestureDetector(
      onTap: _toggleVpn,
      child: Container(
        width: 160,
        height: 160,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: gIsVpnActive
              ? Colors.green.withValues(alpha: 0.15)
              : Theme.of(context).colorScheme.primaryContainer,
          border: Border.all(
            color: gIsVpnActive ? Colors.green.withValues(alpha: 0.4) : Colors.transparent,
            width: 3,
          ),
        ),
        child: Icon(
          gIsVpnActive ? Icons.power_settings_new : Icons.power_settings_new_outlined,
          size: 80,
          color: gIsVpnActive ? Colors.green : Theme.of(context).colorScheme.onPrimaryContainer,
        ),
      ),
    );
  }

  Widget _buildStatsGrid(BuildContext context) {
    final active = _stats?['active_count'] ?? 0;
    final proxied = _stats?['proxied_count'] ?? 0;
    final direct = (active as int) - (proxied as int);
    final total = _stats?['total_count'] ?? 0;

    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceEvenly,
      children: [
        _buildStatCard('$active', 'Active', Colors.green, Icons.link),
        _buildStatCard('$proxied', 'Relayed', Colors.blue, Icons.cloud),
        _buildStatCard('$direct', 'Direct', Colors.grey, Icons.arrow_forward),
        _buildStatCard('$total', 'Total', Colors.purple, Icons.bar_chart),
      ],
    );
  }

  Widget _buildStatCard(String value, String label, Color color, IconData icon) {
    return Column(
      children: [
        Container(
          width: 52,
          height: 52,
          decoration: BoxDecoration(
            color: color.withValues(alpha: 0.12),
            borderRadius: BorderRadius.circular(12),
          ),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(icon, size: 14, color: color),
              const SizedBox(height: 2),
              Text(value, style: TextStyle(
                fontSize: 16, fontWeight: FontWeight.bold, color: color,
              )),
            ],
          ),
        ),
        const SizedBox(height: 4),
        Text(label, style: const TextStyle(fontSize: 10, color: Colors.grey)),
      ],
    );
  }

  Widget _buildTrafficBar(BuildContext context) {
    final up = _stats?['total_bytes_up'] ?? 0;
    final down = _stats?['total_bytes_down'] ?? 0;

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Row(
            children: [
              const Icon(Icons.arrow_upward, size: 14, color: Colors.teal),
              const SizedBox(width: 4),
              Text(_fmtBytes(up as int), style: const TextStyle(fontSize: 13, color: Colors.teal)),
            ],
          ),
          const Text('Traffic', style: TextStyle(fontSize: 11, color: Colors.grey)),
          Row(
            children: [
              Text(_fmtBytes(down as int), style: const TextStyle(fontSize: 13, color: Colors.purple)),
              const SizedBox(width: 4),
              const Icon(Icons.arrow_downward, size: 14, color: Colors.purple),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildLlmCard(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  _llmAnalysis != null ? _llmIcon(_llmAnalysis!) : Icons.smart_toy,
                  size: 16,
                  color: _llmAnalysis != null ? _llmColor(_llmAnalysis!) : Colors.grey,
                ),
                const SizedBox(width: 6),
                const Text('Security Analysis', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                const Spacer(),
                if (_llmLoading)
                  const SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 1.5))
                else
                  IconButton(
                    icon: const Icon(Icons.refresh, size: 16),
                    visualDensity: VisualDensity.compact,
                    onPressed: _requestLlmAnalysis,
                    tooltip: 'Re-analyze',
                  ),
              ],
            ),
            const Divider(height: 12),
            if (_llmAnalysis != null)
              Text(
                _llmAnalysis!,
                style: TextStyle(fontSize: 12, color: _llmColor(_llmAnalysis!)),
              )
            else if (_llmLoading)
              const Text('Analyzing connections...', style: TextStyle(fontSize: 12, color: Colors.grey))
            else
              const Text('Tap refresh for AI security analysis.', style: TextStyle(fontSize: 12, color: Colors.grey)),
          ],
        ),
      ),
    );
  }

  Color _llmColor(String text) {
    if (text.contains('[ALERT]')) return Colors.red;
    if (text.contains('[WARN]')) return Colors.orange;
    return Colors.green.shade300;
  }

  IconData _llmIcon(String text) {
    if (text.contains('[ALERT]')) return Icons.error_outline;
    if (text.contains('[WARN]')) return Icons.warning_amber;
    return Icons.check_circle_outline;
  }

  String _fmtBytes(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    if (bytes < 1024 * 1024 * 1024) return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }

  String _fmtDuration(Duration d) {
    final h = d.inHours;
    final m = d.inMinutes.remainder(60);
    final s = d.inSeconds.remainder(60);
    if (h > 0) return '${h}h ${m}m';
    if (m > 0) return '${m}m ${s}s';
    return '${s}s';
  }
}
