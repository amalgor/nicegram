import 'package:flutter/material.dart';
import 'package:hydra_mobile/logging/log_store.dart';

/// Compact row of live status indicators driven by the in-memory [LogStore]:
/// - Activity: pulses briefly whenever new log lines arrive.
/// - SSH: green when an SSH tunnel is up, grey when down/idle.
///
/// Lightweight by design (Phase 1): derived from the log stream rather than a
/// full connection registry, which will replace these heuristics later.
class ActivityIndicators extends StatefulWidget {
  const ActivityIndicators({super.key});

  @override
  State<ActivityIndicators> createState() => _ActivityIndicatorsState();
}

class _ActivityIndicatorsState extends State<ActivityIndicators>
    with SingleTickerProviderStateMixin {
  final LogStore _store = LogStore.instance;
  late final AnimationController _pulse;
  int _lastSeenCount = 0;

  @override
  void initState() {
    super.initState();
    _pulse = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 450),
      lowerBound: 0.25,
      upperBound: 1.0,
      value: 0.25,
    );
    _lastSeenCount = _store.totalReceived;
    _store.addListener(_onStoreChanged);
  }

  @override
  void dispose() {
    _store.removeListener(_onStoreChanged);
    _pulse.dispose();
    super.dispose();
  }

  void _onStoreChanged() {
    if (!mounted) return;
    if (_store.totalReceived != _lastSeenCount) {
      _lastSeenCount = _store.totalReceived;
      _pulse.forward(from: 1.0);
      _pulse.reverse();
    }
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final sshUp = _store.sshConnected;
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        FadeTransition(
          opacity: _pulse,
          child: _dot(const Color(0xFF38BDF8)),
        ),
        const SizedBox(width: 6),
        const Text('Activity', style: TextStyle(fontSize: 12)),
        const SizedBox(width: 20),
        _dot(sshUp ? const Color(0xFF22C55E) : const Color(0xFF475569)),
        const SizedBox(width: 6),
        Text(
          sshUp ? 'SSH up' : 'SSH down',
          style: TextStyle(
            fontSize: 12,
            color: sshUp ? const Color(0xFF22C55E) : const Color(0xFF94A3B8),
          ),
        ),
      ],
    );
  }

  Widget _dot(Color color) {
    return Container(
      width: 10,
      height: 10,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
  }
}
