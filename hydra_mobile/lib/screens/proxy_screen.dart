import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;

class ProxyScreen extends StatefulWidget {
  const ProxyScreen({super.key});

  @override
  State<ProxyScreen> createState() => _ProxyScreenState();
}

class _ProxyScreenState extends State<ProxyScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  bool _running = false;
  int _port = 1080;

  @override
  void initState() {
    super.initState();
    _refreshStatus();
  }

  void _refreshStatus() {
    final running = simple_api.isNodeRunning();
    final port = simple_api.getSocks5Port();
    if (!mounted) return;
    setState(() {
      _running = running;
      _port = port;
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final theme = Theme.of(context);
    final proxyAddr = '127.0.0.1:$_port';

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        // Status card
        Card(
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                Icon(
                  _running ? Icons.check_circle : Icons.cancel,
                  size: 64,
                  color: _running
                      ? const Color(0xFF22C55E)
                      : const Color(0xFF94A3B8),
                ),
                const SizedBox(height: 12),
                Text(
                  _running ? 'SOCKS5 Proxy Active' : 'Proxy Stopped',
                  style: theme.textTheme.headlineSmall?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const SizedBox(height: 8),
                if (_running)
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      SelectableText(
                        proxyAddr,
                        style: theme.textTheme.titleLarge?.copyWith(
                          fontFamily: 'monospace',
                          color: const Color(0xFF38BDF8),
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton(
                        icon: const Icon(Icons.copy, size: 20),
                        tooltip: 'Copy address',
                        onPressed: () {
                          Clipboard.setData(ClipboardData(text: proxyAddr));
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                              content: Text('Proxy address copied'),
                              duration: Duration(seconds: 1),
                            ),
                          );
                        },
                      ),
                    ],
                  ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 16),

        // Setup instructions
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'How to use',
                  style: theme.textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const SizedBox(height: 12),
                _instructionTile(
                  theme,
                  step: '1',
                  title: 'Configure Routes',
                  body:
                      'Go to Routes tab and add your VLESS or SSH tunnel profile.',
                ),
                _instructionTile(
                  theme,
                  step: '2',
                  title: 'Set proxy in your browser',
                  body:
                      'Firefox: Settings > Network > Manual proxy > SOCKS Host: 127.0.0.1, Port: $_port, SOCKS v5. Check "Proxy DNS".',
                ),
                _instructionTile(
                  theme,
                  step: '3',
                  title: 'Browse freely',
                  body:
                      'Traffic from the configured browser goes through your tunnel. Other apps are unaffected and see no proxy.',
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 16),

        // Firefox quick setup
        Card(
          color: const Color(0xFF1E293B),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Icon(Icons.public, color: Color(0xFFFF7139)),
                    const SizedBox(width: 8),
                    Text(
                      'Firefox Quick Setup',
                      style: theme.textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                Text(
                  'SOCKS Host:   127.0.0.1\n'
                  'Port:         $_port\n'
                  'SOCKS v5:     [x]\n'
                  'Proxy DNS:    [x]',
                  style: const TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 13,
                    height: 1.6,
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  'Other apps (Telegram, etc.) can also use SOCKS5 proxy in their settings with the same address.',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: Colors.grey,
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _instructionTile(
    ThemeData theme, {
    required String step,
    required String title,
    required String body,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: const Color(0xFF0EA5E9).withValues(alpha: 0.2),
              borderRadius: BorderRadius.circular(14),
            ),
            child: Center(
              child: Text(
                step,
                style: const TextStyle(
                  fontWeight: FontWeight.bold,
                  color: Color(0xFF0EA5E9),
                ),
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title,
                    style: theme.textTheme.bodyLarge
                        ?.copyWith(fontWeight: FontWeight.w600)),
                const SizedBox(height: 4),
                Text(body, style: theme.textTheme.bodySmall),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
