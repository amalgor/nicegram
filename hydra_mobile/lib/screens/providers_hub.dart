import 'package:flutter/material.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/widgets/endpoint_tile.dart';
import 'package:hydra_mobile/widgets/tx_hash_tile.dart';

class ProvidersHub extends StatefulWidget {
  const ProvidersHub({
    super.key,
    required this.repository,
    required this.activeProfile,
  });

  final HydraExchangeRepository repository;
  final WalletProfile? activeProfile;

  @override
  State<ProvidersHub> createState() => _ProvidersHubState();
}

class _ProvidersHubState extends State<ProvidersHub>
    with TickerProviderStateMixin {
  late final TabController _tabController = TabController(length: 3, vsync: this);
  ShareEarnStatus? _shareStatus;
  ProviderEarnings? _earnings;
  AgentRegistrationResult? _agentRegistration;
  RouteBookLifecycle? _lifecycle;
  List<RouteOffer> _myRouteOffers = const [];
  bool _loading = true;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void didUpdateWidget(covariant ProvidersHub oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.activeProfile?.id != widget.activeProfile?.id) {
      _refresh();
    }
  }

  Future<void> _refresh() async {
    final profile = widget.activeProfile;
    if (profile == null) {
      if (!mounted) return;
      setState(() {
        _shareStatus = null;
        _earnings = null;
        _agentRegistration = null;
        _lifecycle = null;
        _myRouteOffers = const [];
        _loading = false;
      });
      return;
    }

    try {
      final shareStatus = await widget.repository.loadShareEarnStatus(
        profileId: profile.id,
      );
      final earnings = await widget.repository.loadProviderEarnings(
        profileId: profile.id,
      );
      final agent = await widget.repository.loadAgentRegistration(
        profileId: profile.id,
      );
      final lifecycle = await widget.repository.loadRouteBookLifecycle();
      final routeOffers = await widget.repository.loadMyRouteOffers(
        profileId: profile.id,
      );

      if (!mounted) return;
      setState(() {
        _shareStatus = shareStatus;
        _earnings = earnings;
        _agentRegistration = agent;
        _lifecycle = lifecycle;
        _myRouteOffers = routeOffers;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _loading = false;
      });
      _showMessage(error.toString());
    }
  }

  Future<void> _runBusy(Future<void> Function() action) async {
    if (mounted) {
      setState(() {
        _busy = true;
      });
    }
    try {
      await action();
      await _refresh();
    } catch (error) {
      _showMessage(error.toString());
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  Future<void> _toggleShare(bool enabled) async {
    await _runBusy(() async {
      final status = await widget.repository.setShareEarnEnabled(
        enabled,
        profileId: widget.activeProfile?.id,
      );
      _showMessage(status.toggleMessage);
    });
  }

  Future<void> _editShareSettings() async {
    final current = _shareStatus?.settings;
    final priceController = TextEditingController(
      text: current?.priceOverrideRaw == null
          ? ''
          : _formatMicroUsdc(current!.priceOverrideRaw!),
    );
    final bandwidthController = TextEditingController(
      text: current?.maxBandwidthMbps?.toString() ?? '20',
    );
    bool wifiOnly = current?.wifiOnly ?? false;
    int? scheduleStart = current?.scheduleStartHour;
    int? scheduleEnd = current?.scheduleEndHour;

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setLocalState) => AlertDialog(
            title: const Text('Share settings'),
            content: SizedBox(
              width: 420,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: priceController,
                    decoration: const InputDecoration(
                      labelText: 'Price override',
                      hintText: 'Leave blank for auto pricing',
                      suffixText: 'USDC / GB',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: bandwidthController,
                    decoration: const InputDecoration(
                      labelText: 'Max bandwidth',
                      suffixText: 'Mbps',
                      border: OutlineInputBorder(),
                    ),
                    keyboardType: TextInputType.number,
                  ),
                  const SizedBox(height: 12),
                  SwitchListTile(
                    value: wifiOnly,
                    title: const Text('Wi-Fi only'),
                    onChanged: (value) => setLocalState(() => wifiOnly = value),
                  ),
                  const SizedBox(height: 12),
                  DropdownButtonFormField<int?>(
                    initialValue: scheduleStart,
                    decoration: const InputDecoration(
                      labelText: 'Start hour',
                      border: OutlineInputBorder(),
                    ),
                    items: [
                      const DropdownMenuItem<int?>(
                        value: null,
                        child: Text('Always'),
                      ),
                      ...List.generate(
                        24,
                        (index) => DropdownMenuItem<int?>(
                          value: index,
                          child: Text(index.toString().padLeft(2, '0')),
                        ),
                      ),
                    ],
                    onChanged: (value) => setLocalState(() => scheduleStart = value),
                  ),
                  const SizedBox(height: 12),
                  DropdownButtonFormField<int?>(
                    initialValue: scheduleEnd,
                    decoration: const InputDecoration(
                      labelText: 'End hour',
                      border: OutlineInputBorder(),
                    ),
                    items: [
                      const DropdownMenuItem<int?>(
                        value: null,
                        child: Text('Always'),
                      ),
                      ...List.generate(
                        24,
                        (index) => DropdownMenuItem<int?>(
                          value: index,
                          child: Text(index.toString().padLeft(2, '0')),
                        ),
                      ),
                    ],
                    onChanged: (value) => setLocalState(() => scheduleEnd = value),
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
                child: const Text('Save'),
              ),
            ],
          ),
        );
      },
    );

    if (confirmed != true) return;
    await _runBusy(() async {
      await widget.repository.updateShareSettings(
        profileId: widget.activeProfile?.id,
        priceOverrideRaw: priceController.text.trim().isEmpty
            ? null
            : _microUsdcRaw(priceController.text.trim()),
        maxBandwidthMbps: int.tryParse(bandwidthController.text.trim()),
        wifiOnly: wifiOnly,
        scheduleStartHour: scheduleStart,
        scheduleEndHour: scheduleEnd,
      );
      _showMessage('Share settings updated.');
    });
  }

  Future<void> _syncReputation() async {
    await _runBusy(() async {
      await widget.repository.syncProviderReputation(
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Pending reputation synced.');
    });
  }

  Future<void> _publishRouteOffer() async {
    final status = _shareStatus;
    if (status == null) return;
    final agentId = status.agentId ?? _agentRegistration?.agentId;
    if (agentId == null) {
      _showMessage('Register an agent or enable Share & Earn first.');
      return;
    }
    if (status.onchainActive || status.routeBookOfferId != null) {
      _showMessage('This profile already has a live on-chain route.');
      return;
    }

    final priceController = TextEditingController(
      text: status.pricePerGbDisplay,
    );
    final bandwidthController = TextEditingController(
      text: status.bandwidthMbps.toString(),
    );
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Publish Route Offer'),
        content: SizedBox(
          width: 420,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                readOnly: true,
                controller: TextEditingController(text: status.endpointUrl),
                decoration: const InputDecoration(
                  labelText: 'Endpoint',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                readOnly: true,
                controller: TextEditingController(text: status.region),
                decoration: const InputDecoration(
                  labelText: 'Region',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: priceController,
                decoration: const InputDecoration(
                  labelText: 'Price',
                  suffixText: 'USDC / GB',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: bandwidthController,
                decoration: const InputDecoration(
                  labelText: 'Bandwidth',
                  suffixText: 'Mbps',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              Text(
                'Stake requirement: 1.000000 USDC',
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
            child: const Text('Publish'),
          ),
        ],
      ),
    );

    if (confirmed != true) return;
    await _runBusy(() async {
      final result = await widget.repository.createOffer(
        agentId: agentId,
        endpointUrl: status.endpointUrl,
        protocols: const ['wss'],
        region: status.region,
        pricePerGbRaw: _microUsdcRaw(priceController.text.trim()),
        stakeAmountRaw: '1000000',
        bandwidthMbps: int.tryParse(bandwidthController.text.trim()) ?? status.bandwidthMbps,
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Route offer #${result.offerId ?? "?"} published.');
    });
  }

  Future<void> _deactivateRouteOffer(RouteOffer offer) async {
    await _runBusy(() async {
      await widget.repository.deactivateOffer(
        offer.offerId,
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Route offer deactivated.');
    });
  }

  Future<void> _withdrawStake(RouteOffer offer) async {
    final lifecycle = _lifecycle;
    if (lifecycle == null) {
      _showMessage('Route lifecycle info is unavailable.');
      return;
    }
    final readyAtMs =
        (offer.deactivatedAt + lifecycle.withdrawalDelaySecs) * 1000;
    if (offer.deactivatedAt == 0 ||
        DateTime.now().millisecondsSinceEpoch < readyAtMs) {
      _showMessage('Stake withdrawal is still locked by the delay window.');
      return;
    }
    await _runBusy(() async {
      await widget.repository.withdrawStake(
        offer.offerId,
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Stake withdrawn.');
    });
  }

  void _showMessage(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  @override
  Widget build(BuildContext context) {
    if (widget.activeProfile == null) {
      return const _InfoCard(
        title: 'Providers need a wallet profile',
        message:
            'Create or import a wallet profile from the Balance header, then enable Share & Earn or publish route offers from here.',
      );
    }

    return Column(
      children: [
        TabBar(
          controller: _tabController,
          tabs: const [
            Tab(text: 'Share & Earn'),
            Tab(text: 'Route Offers'),
            Tab(text: 'Metrics'),
          ],
        ),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : TabBarView(
                  controller: _tabController,
                  children: [
                    _buildShareTab(),
                    _buildOffersTab(),
                    _buildMetricsTab(),
                  ],
                ),
        ),
      ],
    );
  }

  Widget _buildShareTab() {
    final status = _shareStatus;
    final earnings = _earnings;
    if (status == null || earnings == null) {
      return const _InfoCard(
        title: 'Share status unavailable',
        message: 'Pull to retry the provider runtime state.',
      );
    }

    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Share & Earn',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 8),
                  Text(status.toggleMessage),
                  const SizedBox(height: 12),
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    value: status.enabled,
                    title: const Text('Enable Share & Earn'),
                    subtitle: Text(status.active ? 'Live relay session' : 'Not connected'),
                    onChanged: _busy ? null : _toggleShare,
                  ),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      Chip(
                        label: Text(status.active ? 'Connected' : 'Idle'),
                      ),
                      Chip(
                        label: Text(
                          status.onchainActive ? 'On-chain' : 'Starter mode',
                        ),
                      ),
                      if (status.sharingActiveUnderOtherProfile)
                        Chip(
                          label: Text(
                            'Pinned to ${status.runtimeProfileId ?? "another profile"}',
                          ),
                        ),
                    ],
                  ),
                  if (status.lastError.trim().isNotEmpty) ...[
                    const SizedBox(height: 8),
                    Text(
                      'Last error: ${status.lastError}',
                      style: Theme.of(
                        context,
                      ).textTheme.bodySmall?.copyWith(color: Colors.orangeAccent),
                    ),
                  ],
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      FilledButton.icon(
                        onPressed: _busy ? null : _editShareSettings,
                        icon: const Icon(Icons.tune),
                        label: const Text('Settings'),
                      ),
                      OutlinedButton.icon(
                        onPressed: _busy ? null : _syncReputation,
                        icon: const Icon(Icons.sync),
                        label: const Text('Sync reputation'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
          EndpointTile(
            label: 'Relay-backed endpoint',
            endpoint: status.endpointUrl,
            caption:
                'This is the endpoint Hydra publishes for manual provider testing.',
          ),
          if (_agentRegistration != null)
            TxHashTile(
              label: 'Agent registration tx',
              txHash: _agentRegistration!.txHash,
            ),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Provider snapshot',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 8),
                  Text('Agent ID: ${status.agentId ?? "not registered"}'),
                  Text('Region: ${status.region}'),
                  Text('Protocol: ${status.protocol}'),
                  Text('Price: ${status.pricePerGbDisplay} USDC/GB'),
                  Text('Bandwidth: ${status.bandwidthMbps} Mbps'),
                  Text('Projected earnings: ${earnings.estimatedEarningsDisplay}'),
                  Text('Settled earnings: ${earnings.settledEarningsDisplay}'),
                  Text('Local score: ${earnings.localRoutingScore.toStringAsFixed(1)}'),
                  Text('Pending reputation delta: ${earnings.pendingReputationDelta.toStringAsFixed(2)}'),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildOffersTab() {
    final status = _shareStatus;
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          if (status != null && status.agentId != null)
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Route lifecycle',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: 8),
                    Text('Agent ID: ${status.agentId}'),
                    Text('Offer ID: ${status.routeBookOfferId ?? "not published"}'),
                    Text(
                      status.onchainActive
                          ? 'On-chain route is live.'
                          : 'Current profile is still in gossip/starter mode.',
                    ),
                    const SizedBox(height: 12),
                    FilledButton.icon(
                      onPressed: _busy ? null : _publishRouteOffer,
                      icon: const Icon(Icons.publish),
                      label: const Text('Publish route offer'),
                    ),
                  ],
                ),
              ),
            ),
          if (_myRouteOffers.isEmpty)
            const _InfoCard(
              title: 'No route offers yet',
              message:
                  'Enable Share & Earn first, then publish a relay-backed route offer for manual testing.',
            )
          else
            ..._myRouteOffers.map(_buildRouteOfferCard),
        ],
      ),
    );
  }

  Widget _buildRouteOfferCard(RouteOffer offer) {
    final lifecycle = _lifecycle;
    final readyAt = lifecycle == null || offer.deactivatedAt == 0
        ? null
        : DateTime.fromMillisecondsSinceEpoch(
            (offer.deactivatedAt + lifecycle.withdrawalDelaySecs) * 1000,
          );

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Route offer #${offer.offerId}',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            Text('Agent ID: ${offer.agentId}'),
            Text('Region: ${offer.region}'),
            Text('Price: ${offer.pricePerGb} USDC/GB'),
            Text('Stake: ${offer.stakeAmount} USDC'),
            Text('Bandwidth: ${offer.bandwidthMbps} Mbps'),
            Text(
              offer.active
                  ? 'Status: active'
                  : 'Status: deactivated${readyAt == null ? '' : ' • withdraw after ${_formatDateTime(readyAt)}'}',
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                OutlinedButton.icon(
                  onPressed: _busy || !offer.active
                      ? null
                      : () => _deactivateRouteOffer(offer),
                  icon: const Icon(Icons.pause_circle_outline),
                  label: const Text('Deactivate'),
                ),
                FilledButton.icon(
                  onPressed: _busy || offer.active ? null : () => _withdrawStake(offer),
                  icon: const Icon(Icons.savings_outlined),
                  label: const Text('Withdraw stake'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildMetricsTab() {
    final earnings = _earnings;
    if (earnings == null) {
      return const _InfoCard(
        title: 'Metrics unavailable',
        message: 'Provider metrics are not ready yet.',
      );
    }
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Provider metrics',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 8),
                  Text('Sessions: ${earnings.sessionCount}'),
                  Text('Successful sessions: ${earnings.successfulSessions}'),
                  Text('Relayed bytes: ${earnings.bytesRelayed}'),
                  Text('Average latency: ${earnings.averageLatencyMs.toStringAsFixed(1)} ms'),
                  Text(
                    'Average throughput: ${earnings.averageThroughputMbps.toStringAsFixed(2)} Mbps',
                  ),
                  Text('Uptime ratio: ${(earnings.uptimeRatio * 100).toStringAsFixed(1)}%'),
                  Text('Recent failures: ${earnings.recentFailures}'),
                  Text('Pending syncs: ${earnings.pendingReputationSyncs}'),
                  Text('Last on-chain sync: ${_formatEpochSeconds(earnings.lastOnchainSyncTime)}'),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _InfoCard extends StatelessWidget {
  const _InfoCard({required this.title, required this.message});

  final String title;
  final String message;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
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
        ),
      ],
    );
  }
}

String _formatMicroUsdc(String raw) {
  final digits = raw.replaceAll(RegExp(r'[^0-9]'), '');
  if (digits.isEmpty) return '0';
  final padded = digits.padLeft(7, '0');
  final whole = padded.substring(0, padded.length - 6);
  final fraction = padded.substring(padded.length - 6).replaceFirst(RegExp(r'0+$'), '');
  return fraction.isEmpty ? whole : '$whole.$fraction';
}

String _microUsdcRaw(String value) {
  final parts = value.trim().split('.');
  final whole = parts.first.isEmpty ? '0' : parts.first;
  final fraction = parts.length > 1 ? parts[1] : '';
  return '$whole${fraction.padRight(6, '0').substring(0, 6)}';
}

String _formatDateTime(DateTime dateTime) {
  return '${dateTime.year}-${dateTime.month.toString().padLeft(2, '0')}-${dateTime.day.toString().padLeft(2, '0')} '
      '${dateTime.hour.toString().padLeft(2, '0')}:${dateTime.minute.toString().padLeft(2, '0')}';
}

String _formatEpochSeconds(int value) {
  if (value <= 0) return 'never';
  return _formatDateTime(DateTime.fromMillisecondsSinceEpoch(value * 1000));
}
