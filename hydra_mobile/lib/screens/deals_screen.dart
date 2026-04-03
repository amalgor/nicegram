import 'package:flutter/material.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';

class DealsScreen extends StatefulWidget {
  const DealsScreen({super.key, this.repository});

  final HydraExchangeRepository? repository;

  @override
  State<DealsScreen> createState() => _DealsScreenState();
}

class _DealsScreenState extends State<DealsScreen> {
  late final HydraExchangeRepository _repository;
  List<DealOffer> _offers = const [];
  bool _loading = true;
  bool _busy = false;
  String _currency = 'RUB';
  String? _error;

  @override
  void initState() {
    super.initState();
    _repository = widget.repository ?? HydraExchangeRepository.instance;
    _loadOffers();
  }

  Future<void> _loadOffers() async {
    if (mounted) {
      setState(() {
        _loading = true;
        _error = null;
      });
    }

    try {
      final offers = await _repository.fetchDealOffers(currency: _currency);
      if (!mounted) return;
      setState(() {
        _offers = offers;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _offers = const [];
      });
    } finally {
      if (mounted) {
        setState(() {
          _loading = false;
        });
      }
    }
  }

  Future<void> _acceptDeal(DealOffer offer) async {
    final hasWallet = await _repository.hasWallet();
    if (!hasWallet) {
      _showMessage('Create or import a wallet first in Advanced Tools.');
      return;
    }

    final amountController = TextEditingController(text: offer.minAmount);
    final confirmed = await showDialog<String>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: Text('Accept Deal #${offer.offerId}'),
          content: SizedBox(
            width: 360,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Dealer: ${_shortAddress(offer.dealer)}'),
                Text('Rate: ${offer.rate} ${offer.currency}/USDC'),
                Text('Range: ${offer.minAmount} - ${offer.maxAmount} USDC'),
                Text('Payment: ${offer.paymentMethods.join(", ")}'),
                if (offer.reputation != null)
                  Text(
                    'Reputation: ${offer.reputation!.formattedValue} (${offer.reputation!.feedbackCount} feedback)',
                  ),
                const SizedBox(height: 16),
                TextField(
                  controller: amountController,
                  decoration: const InputDecoration(
                    labelText: 'USDC Amount',
                    border: OutlineInputBorder(),
                    suffixText: 'USDC',
                  ),
                  keyboardType:
                      const TextInputType.numberWithOptions(decimal: true),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () =>
                  Navigator.of(context).pop(amountController.text),
              child: const Text('Accept & Lock USDC'),
            ),
          ],
        );
      },
    );

    if (confirmed == null || confirmed.trim().isEmpty) return;

    await _runBusy(() async {
      final result = await _repository.acceptDeal(
        offerId: offer.offerId,
        usdcAmount: confirmed.trim(),
      );
      _showMessage(
        'Deal accepted. Escrow #${result.escrowId ?? "?"} created. Tx: ${_shortAddress(result.txHash)}',
      );
    });
  }

  Future<void> _runBusy(Future<void> Function() action) async {
    if (mounted) {
      setState(() {
        _busy = true;
      });
    }
    try {
      await action();
    } catch (e) {
      if (mounted) {
        _showMessage(e.toString());
      }
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  void _showMessage(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  String _shortAddress(String value) {
    if (value.length <= 14) return value;
    return '${value.substring(0, 8)}...${value.substring(value.length - 6)}';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('P2P Deals'),
        actions: [
          if (_busy)
            const Padding(
              padding: EdgeInsets.all(12),
              child: SizedBox.square(
                dimension: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            ),
        ],
      ),
      body: RefreshIndicator(
        onRefresh: _loadOffers,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            _buildFilterCard(context),
            const SizedBox(height: 16),
            if (_loading)
              const Center(child: CircularProgressIndicator())
            else if (_error != null)
              _buildInfoCard(
                context,
                title: 'Load Failed',
                message: _error!,
              )
            else if (_offers.isEmpty)
              _buildInfoCard(
                context,
                title: 'No Deals',
                message:
                    'No active fiat-to-USDC deals for $_currency. Pull to retry.',
              )
            else
              ..._offers
                  .map((offer) => _buildDealCard(context, offer))
                  .toList(),
          ],
        ),
      ),
    );
  }

  Widget _buildFilterCard(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Fiat Currency',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(
                  child: TextFormField(
                    initialValue: _currency,
                    decoration: const InputDecoration(
                      labelText: 'Currency (ISO 4217)',
                      hintText: 'e.g. RUB, USD, EUR',
                      border: OutlineInputBorder(),
                    ),
                    textCapitalization: TextCapitalization.characters,
                    onChanged: (value) => _currency = value,
                  ),
                ),
                const SizedBox(width: 12),
                FilledButton.icon(
                  onPressed: _busy ? null : _loadOffers,
                  icon: const Icon(Icons.search),
                  label: const Text('Search'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildInfoCard(
    BuildContext context, {
    required String title,
    required String message,
  }) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(message),
          ],
        ),
      ),
    );
  }

  Widget _buildDealCard(BuildContext context, DealOffer offer) {
    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    'Deal #${offer.offerId}',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
                Chip(label: Text(offer.currency)),
              ],
            ),
            const SizedBox(height: 8),
            Text('Dealer: ${_shortAddress(offer.dealer)}'),
            Text('Agent ID: ${offer.agentId}'),
            Text('Rate: ${offer.rate} ${offer.currency}/USDC'),
            Text('Range: ${offer.minAmount} - ${offer.maxAmount} USDC'),
            Text('Payment: ${offer.paymentMethods.join(", ")}'),
            const SizedBox(height: 8),
            if (offer.reputation != null)
              Text(
                'Reputation: ${offer.reputation!.formattedValue} (${offer.reputation!.feedbackCount} feedback)',
              )
            else
              const Text('Reputation: unavailable'),
            const SizedBox(height: 12),
            FilledButton.icon(
              onPressed: _busy ? null : () => _acceptDeal(offer),
              icon: const Icon(Icons.handshake),
              label: const Text('Accept Deal'),
            ),
          ],
        ),
      ),
    );
  }
}
