import 'package:flutter/material.dart';
import 'package:hydra_mobile/credit/models.dart';

class CreditStatusWidget extends StatelessWidget {
  const CreditStatusWidget({
    super.key,
    required this.status,
    required this.onTopUpPressed,
    this.compact = false,
  });

  final CreditStatus status;
  final VoidCallback onTopUpPressed;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final fraction = status.usageFraction;
    final color = switch (fraction) {
      < 0.5 => Colors.green,
      < 0.8 => Colors.orange,
      _ => Colors.red,
    };

    final subtitle = switch (status.routeState) {
      'premium' => 'Faster routes available automatically',
      'trial_available' => 'A faster route is ready to try',
      'fallback' => 'Free routes are keeping Telegram online',
      _ => 'Hydra is on the free path',
    };

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    status.trialAccepted
                        ? 'Balance ${status.debtDisplay} / ${status.creditLimitDisplay}'
                        : 'Starter balance ${status.creditLimitDisplay}',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
                if (!compact)
                  TextButton(
                    onPressed: onTopUpPressed,
                    child: const Text('Top up'),
                  ),
              ],
            ),
            const SizedBox(height: 8),
            LinearProgressIndicator(
              value: fraction,
              minHeight: 10,
              borderRadius: BorderRadius.circular(12),
              color: color,
              backgroundColor: Colors.white12,
            ),
            const SizedBox(height: 8),
            Text(
              '${(fraction * 100).toStringAsFixed(0)}% used',
              style: TextStyle(color: color, fontWeight: FontWeight.w600),
            ),
            const SizedBox(height: 4),
            Text(subtitle),
            if (compact && !status.showAdvancedTools) ...[
              const SizedBox(height: 12),
              FilledButton.tonal(
                onPressed: onTopUpPressed,
                child: const Text('Top up'),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
