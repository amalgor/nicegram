import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/exchange/explorer_links.dart';
import 'package:hydra_mobile/widgets/receive_sheet.dart';
import 'package:share_plus/share_plus.dart';
import 'package:url_launcher/url_launcher.dart';

class AddressTile extends StatelessWidget {
  const AddressTile({
    super.key,
    required this.label,
    required this.address,
    this.caption,
  });

  final String label;
  final String address;
  final String? caption;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            SelectableText(
              address,
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
                    await Clipboard.setData(ClipboardData(text: address));
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Address copied.')),
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
                    value: address,
                    subtitle: caption,
                    helperText: 'Base Sepolia uses the same address for ETH and USDC.',
                  ),
                ),
                ActionChip(
                  avatar: const Icon(Icons.ios_share, size: 18),
                  label: const Text('Share'),
                  onPressed: () => SharePlus.instance.share(
                    ShareParams(text: address, title: label),
                  ),
                ),
                ActionChip(
                  avatar: const Icon(Icons.open_in_new, size: 18),
                  label: const Text('Explorer'),
                  onPressed: () => launchUrl(baseSepoliaAddressUri(address)),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
