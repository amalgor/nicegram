import 'package:flutter/material.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';

class MarketplaceScreen extends StatefulWidget {
  const MarketplaceScreen({super.key, this.repository});

  final HydraExchangeRepository? repository;

  @override
  State<MarketplaceScreen> createState() => _MarketplaceScreenState();
}

class _MarketplaceScreenState extends State<MarketplaceScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  late final HydraExchangeRepository _repository;

  MarketplaceConfigStatus? _configStatus;
  WalletIdentity? _wallet;
  WalletBalances? _balances;
  AgentRegistrationResult? _agentRegistration;
  List<RouteOffer> _offers = const [];
  bool _loading = true;
  bool _loadingOffers = true;
  bool _busy = false;
  String _region = '';
  String _protocol = 'vless';
  String? _offersError;

  @override
  void initState() {
    super.initState();
    _repository = widget.repository ?? HydraExchangeRepository.instance;
    _initialize();
  }

  Future<void> _initialize() async {
    try {
      final filters = await _repository.loadFilters(
        defaultRegion: '',
      );
      _region = filters['region'] ?? '';
      _protocol = filters['protocol'] ?? 'vless';
      await _loadConfigStatus();
      await _loadWalletState();
      if (_marketplaceReady) {
        await _loadOffers();
      } else if (mounted) {
        setState(() {
          _offers = const [];
          _offersError = null;
          _loadingOffers = false;
        });
      }
    } finally {
      if (mounted) {
        setState(() {
          _loading = false;
        });
      }
    }
  }

  bool get _marketplaceReady => _configStatus?.ready ?? false;

  bool get _feedbackReady =>
      _marketplaceReady && (_configStatus?.reputationEnabled ?? false);

  Future<void> _loadConfigStatus() async {
    final status = await _repository.loadConfigStatus();
    if (!mounted) return;
    setState(() {
      _configStatus = status;
    });
  }

  Future<void> _loadWalletState() async {
    final hasWallet = await _repository.hasWallet();
    if (!hasWallet) {
      if (!mounted) return;
      setState(() {
        _wallet = null;
        _balances = null;
        _agentRegistration = null;
      });
      return;
    }

    final wallet = await _repository.loadWalletAddress();
    final balances = _marketplaceReady ? await _repository.loadBalances() : null;
    final registration = await _repository.loadAgentRegistration();

    if (!mounted) return;
    setState(() {
      _wallet = wallet;
      _balances = balances;
      _agentRegistration = registration;
    });
  }

  Future<void> _loadOffers() async {
    if (!_marketplaceReady) {
      if (!mounted) return;
      setState(() {
        _loadingOffers = false;
        _offers = const [];
        _offersError = null;
      });
      return;
    }

    if (mounted) {
      setState(() {
        _loadingOffers = true;
        _offersError = null;
      });
    }

    try {
      await _repository.saveFilters(region: _region, protocol: _protocol);
      final offers = await _repository.fetchOffers(
        region: _region,
        protocol: _protocol,
      );
      if (!mounted) return;
      setState(() {
        _offers = offers;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _offersError = 'Failed to load offers from Base Sepolia. Pull to retry.';
        _offers = const [];
      });
    } finally {
      if (mounted) {
        setState(() {
          _loadingOffers = false;
        });
      }
    }
  }

  Future<void> _refresh() async {
    await _loadConfigStatus();
    await _loadWalletState();
    await _loadOffers();
  }

  Future<void> _createWallet() async {
    final draft = await _runBusy(() => _repository.createWallet());
    if (draft == null || !mounted) {
      return;
    }

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Backup Recovery Phrase'),
          content: SizedBox(
            width: 420,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Write down these 12 words. Hydra will store the wallet only after you confirm the backup.',
                ),
                const SizedBox(height: 16),
                SelectableText(
                  draft.mnemonic,
                  style: const TextStyle(fontFamily: 'monospace', height: 1.4),
                ),
                const SizedBox(height: 16),
                Text(
                  'Derived address: ${_shortHash(draft.address)}',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: const Text('I Saved It'),
            ),
          ],
        );
      },
    );

    if (confirmed != true) {
      return;
    }

    final imported = await _runBusy(() => _repository.importWallet(draft.mnemonic));
    if (imported == null || !mounted) {
      return;
    }

    await _refresh();
    _showMessage('Wallet ready at ${_shortHash(imported.address)}.');
  }

  Future<void> _importWallet() async {
    final controller = TextEditingController();
    final phrase = await showDialog<String>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Import Recovery Phrase'),
          content: TextField(
            controller: controller,
            autofocus: true,
            maxLines: 4,
            minLines: 3,
            decoration: const InputDecoration(
              hintText: 'Enter the 12-word BIP-39 phrase',
              border: OutlineInputBorder(),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(controller.text),
              child: const Text('Import'),
            ),
          ],
        );
      },
    );

    if (phrase == null || phrase.trim().isEmpty) {
      return;
    }

    final imported = await _runBusy(() => _repository.importWallet(phrase));
    if (imported == null || !mounted) {
      return;
    }

    await _refresh();
    _showMessage('Imported wallet ${_shortHash(imported.address)}.');
  }

  Future<void> _registerAgent() async {
    if (!_marketplaceReady) {
      _showMessage(_configStatus?.message ?? 'Marketplace config is incomplete.');
      return;
    }

    final result = await _runBusy(() => _repository.registerAgent());
    if (result == null || !mounted) {
      return;
    }

    setState(() {
      _agentRegistration = result;
    });
    _showMessage('Registered agent #${result.agentId}.');
  }

  Future<void> _submitFeedback(RouteOffer offer, bool positive) async {
    if (_wallet == null) {
      _showMessage('Create or import a wallet first.');
      return;
    }
    if (!_marketplaceReady) {
      _showMessage(_configStatus?.message ?? 'Marketplace config is incomplete.');
      return;
    }
    if (!_feedbackReady) {
      _showMessage(
        'Feedback is unavailable until [crypto].reputation_registry_address is configured.',
      );
      return;
    }

    final tag = await showDialog<String>(
      context: context,
      builder: (context) {
        return SimpleDialog(
          title: const Text('Feedback Tag'),
          children: [
            for (final tag in const [
              'availability',
              'latency',
              'throughput',
              'trust',
            ])
              SimpleDialogOption(
                onPressed: () => Navigator.of(context).pop(tag),
                child: Text(tag),
              ),
          ],
        );
      },
    );

    if (tag == null) {
      return;
    }

    final result = await _runBusy(
      () => _repository.submitFeedback(
        agentId: offer.agentId,
        positive: positive,
        tag1: tag,
      ),
    );

    if (result == null || !mounted) {
      return;
    }

    _showMessage('Feedback submitted: ${_shortHash(result.txHash)}');
  }

  Future<T?> _runBusy<T>(Future<T> Function() action) async {
    if (mounted) {
      setState(() {
        _busy = true;
      });
    }

    try {
      return await action();
    } catch (e) {
      if (mounted) {
        _showMessage(e.toString());
      }
      return null;
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  void _showMessage(String message) {
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(message)));
  }

  String _shortHash(String value) {
    if (value.length <= 14) {
      return value;
    }
    return '${value.substring(0, 8)}…${value.substring(value.length - 6)}';
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);

    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }

    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _buildWalletCard(context),
          const SizedBox(height: 16),
          _buildAgentCard(context),
          const SizedBox(height: 16),
          _buildFiltersCard(context),
          const SizedBox(height: 16),
          _buildOffersSection(context),
        ],
      ),
    );
  }

  Widget _buildWalletCard(BuildContext context) {
    final status = _configStatus;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text(
                  'Wallet',
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const Spacer(),
                if (_busy) const SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2)),
              ],
            ),
            const SizedBox(height: 12),
            if (_wallet == null) ...[
              const Text(
                'Marketplace uses a local Base Sepolia wallet stored in platform secure storage.',
              ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  FilledButton.icon(
                    onPressed: _busy ? null : _createWallet,
                    icon: const Icon(Icons.add_card),
                    label: const Text('Create Wallet'),
                  ),
                  OutlinedButton.icon(
                    onPressed: _busy ? null : _importWallet,
                    icon: const Icon(Icons.download),
                    label: const Text('Import Phrase'),
                  ),
                ],
              ),
            ] else ...[
              Text('Address: ${_wallet!.address}'),
              if (_balances != null) ...[
                const SizedBox(height: 8),
                Text('Chain: ${_balances!.chain}'),
                Text('Gas balance: ${_balances!.ethBalance} ETH'),
                Text('USDC balance: ${_balances!.usdcBalance} USDC'),
              ] else if (status != null && !status.ready) ...[
                const SizedBox(height: 8),
                Text(
                  status.message,
                  style: Theme.of(
                    context,
                  ).textTheme.bodySmall?.copyWith(color: Colors.orangeAccent),
                ),
              ],
              const SizedBox(height: 12),
              const Text(
                'Backup warning: Hydra cannot recover this wallet if the recovery phrase is lost.',
                style: TextStyle(color: Colors.orangeAccent),
              ),
              const SizedBox(height: 12),
              OutlinedButton.icon(
                onPressed: _busy ? null : _importWallet,
                icon: const Icon(Icons.swap_horiz),
                label: const Text('Replace Wallet'),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildAgentCard(BuildContext context) {
    final status = _configStatus;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Agent', style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 12),
            if (status != null && !status.ready)
              Text(status.message)
            else if (_wallet == null)
              const Text('Registering an ERC-8004 agent requires a local wallet.')
            else if (_agentRegistration == null) ...[
              const Text('Register this device wallet as an ERC-8004 agent.'),
              const SizedBox(height: 12),
              FilledButton.icon(
                onPressed: _busy ? null : _registerAgent,
                icon: const Icon(Icons.how_to_reg),
                label: const Text('Register Agent'),
              ),
            ] else ...[
              Text('Agent ID: ${_agentRegistration!.agentId}'),
              Text('Last tx: ${_agentRegistration!.txHash}'),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildFiltersCard(BuildContext context) {
    final status = _configStatus;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Offer Filters', style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: TextFormField(
                    initialValue: _region,
                    decoration: const InputDecoration(
                      labelText: 'Region',
                      border: OutlineInputBorder(),
                    ),
                    textCapitalization: TextCapitalization.characters,
                    onChanged: (value) => _region = value,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: TextFormField(
                    initialValue: _protocol,
                    decoration: const InputDecoration(
                      labelText: 'Protocol',
                      border: OutlineInputBorder(),
                    ),
                    onChanged: (value) => _protocol = value,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            if (status != null && !status.ready) ...[
              Text(
                status.message,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: Colors.orangeAccent),
              ),
              const SizedBox(height: 12),
            ],
            FilledButton.icon(
              onPressed: _busy || !_marketplaceReady ? null : _loadOffers,
              icon: const Icon(Icons.search),
              label: const Text('Refresh Offers'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildOffersSection(BuildContext context) {
    final status = _configStatus;
    if (status != null && status.isDisabled) {
      return _buildInfoCard(
        context,
        title: 'Marketplace Disabled',
        message: status.message,
      );
    }

    if (status != null && status.isIncomplete) {
      return _buildInfoCard(
        context,
        title: 'Marketplace Config Incomplete',
        message: status.message,
      );
    }

    if (_loadingOffers) {
      return const Card(
        child: Padding(
          padding: EdgeInsets.all(24),
          child: Center(child: CircularProgressIndicator()),
        ),
      );
    }

    if (_offersError != null) {
      return _buildInfoCard(
        context,
        title: 'Offer Load Failed',
        message: _offersError!,
      );
    }

    if (_offers.isEmpty) {
      return _buildInfoCard(
        context,
        title: 'No Offers',
        message: 'No active offers matched the selected region and protocol.',
      );
    }

    return Column(
      children: _offers.map((offer) => _buildOfferCard(context, offer)).toList(),
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

  Widget _buildOfferCard(BuildContext context, RouteOffer offer) {
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
                    'Offer #${offer.offerId}',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
                Chip(label: Text(offer.region)),
              ],
            ),
            const SizedBox(height: 8),
            Text('Provider: ${offer.provider}'),
            Text('Agent ID: ${offer.agentId}'),
            Text('Protocols: ${offer.protocols.join(', ')}'),
            Text('Price / GB: ${offer.pricePerGb} USDC'),
            Text('Stake: ${offer.stakeAmount} USDC'),
            Text('Bandwidth: ${offer.bandwidthMbps} Mbps'),
            const SizedBox(height: 8),
            if (offer.reputation != null)
              Text(
                'Reputation: ${offer.reputation!.formattedValue} (${offer.reputation!.feedbackCount} feedback)',
              )
            else
              const Text('Reputation: unavailable'),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                OutlinedButton.icon(
                  onPressed: _busy ? null : () => _submitFeedback(offer, true),
                  icon: const Icon(Icons.thumb_up_alt_outlined),
                  label: const Text('Positive'),
                ),
                OutlinedButton.icon(
                  onPressed: _busy ? null : () => _submitFeedback(offer, false),
                  icon: const Icon(Icons.thumb_down_alt_outlined),
                  label: const Text('Negative'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
