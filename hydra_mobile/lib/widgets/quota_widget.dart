import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/quota.dart';

class QuotaWidget extends StatefulWidget {
  const QuotaWidget({super.key});

  @override
  State<QuotaWidget> createState() => _QuotaWidgetState();
}

class _QuotaWidgetState extends State<QuotaWidget> {
  Timer? _refreshTimer;
  int _used = 0;
  int _limit = 0;

  @override
  void initState() {
    super.initState();
    _refreshQuota();
    _refreshTimer = Timer.periodic(const Duration(seconds: 5), (_) => _refreshQuota());
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  void _refreshQuota() async {
    try {
      final json = await getQuotaStatus();
      final data = jsonDecode(json) as Map<String, dynamic>;
      if (mounted) {
        setState(() {
          _used = (data['used'] as num?)?.toInt() ?? 0;
          _limit = (data['limit'] as num?)?.toInt() ?? 0;
        });
      }
    } catch (e) {
      debugPrint("Quota refresh error: $e");
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_limit == 0) return const SizedBox.shrink();

    final fraction = (_used / _limit).clamp(0.0, 1.0);

    Color progressColor;
    if (fraction < 0.6) {
      progressColor = Colors.green;
    } else if (fraction < 0.85) {
      progressColor = Colors.orange;
    } else {
      progressColor = Colors.red;
    }

    final usedMb = (_used / (1024 * 1024)).toStringAsFixed(1);
    final limitMb = (_limit / (1024 * 1024)).toStringAsFixed(0);

    return Column(
      children: [
        SizedBox(
          width: 80,
          height: 80,
          child: Stack(
            fit: StackFit.expand,
            children: [
              CircularProgressIndicator(
                value: fraction,
                strokeWidth: 6,
                backgroundColor: Colors.grey.shade800,
                color: progressColor,
              ),
              Center(
                child: Text(
                  '${(fraction * 100).toStringAsFixed(0)}%',
                  style: TextStyle(
                    color: progressColor,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        Text(
          '$usedMb MB / $limitMb MB today',
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ],
    );
  }
}
