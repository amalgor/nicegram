import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/exchange/explorer_links.dart';
import 'package:url_launcher/url_launcher.dart';

class TxHashTile extends StatelessWidget {
  const TxHashTile({
    super.key,
    required this.label,
    required this.txHash,
  });

  final String label;
  final String txHash;

  @override
  Widget build(BuildContext context) {
    if (txHash.trim().isEmpty) {
      return const SizedBox.shrink();
    }
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            SelectableText(
              txHash,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace'),
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                ActionChip(
                  avatar: const Icon(Icons.copy, size: 18),
                  label: const Text('Copy'),
                  onPressed: () async {
                    await Clipboard.setData(ClipboardData(text: txHash));
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Transaction hash copied.')),
                      );
                    }
                  },
                ),
                ActionChip(
                  avatar: const Icon(Icons.open_in_new, size: 18),
                  label: const Text('Explorer'),
                  onPressed: () => launchUrl(baseSepoliaTxUri(txHash)),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
