import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/widgets/receive_sheet.dart';
import 'package:share_plus/share_plus.dart';

class EndpointTile extends StatelessWidget {
  const EndpointTile({
    super.key,
    required this.label,
    required this.endpoint,
    this.caption,
  });

  final String label;
  final String endpoint;
  final String? caption;

  @override
  Widget build(BuildContext context) {
    if (endpoint.trim().isEmpty) {
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
              endpoint,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace'),
            ),
            if (caption != null) ...[
              const SizedBox(height: 8),
              Text(caption!, style: Theme.of(context).textTheme.bodySmall),
            ],
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                ActionChip(
                  avatar: const Icon(Icons.copy, size: 18),
                  label: const Text('Copy'),
                  onPressed: () async {
                    await Clipboard.setData(ClipboardData(text: endpoint));
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Endpoint copied.')),
                      );
                    }
                  },
                ),
                ActionChip(
                  avatar: const Icon(Icons.qr_code_2, size: 18),
                  label: const Text('QR'),
                  onPressed: () => showReceiveSheet(
                    context,
                    title: label,
                    value: endpoint,
                    subtitle: caption,
                    helperText: 'Share this relay-backed endpoint with another Hydra test client.',
                  ),
                ),
                ActionChip(
                  avatar: const Icon(Icons.ios_share, size: 18),
                  label: const Text('Share'),
                  onPressed: () => SharePlus.instance.share(
                    ShareParams(text: endpoint, title: label),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
