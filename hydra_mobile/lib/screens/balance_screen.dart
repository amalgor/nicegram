import 'package:flutter/material.dart';
import 'package:hydra_mobile/credit/credit_repository.dart';
import 'package:hydra_mobile/credit/models.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/screens/deals_screen.dart';
import 'package:hydra_mobile/screens/marketplace_screen.dart';
import 'package:hydra_mobile/widgets/credit_status_widget.dart';

class BalanceScreen extends StatefulWidget {
  const BalanceScreen({super.key, this.repository, this.exchangeRepository});

  final CreditRepository? repository;
  final HydraExchangeRepository? exchangeRepository;

  @override
  State<BalanceScreen> createState() => _BalanceScreenState();
}

class _BalanceScreenState extends State<BalanceScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  late final CreditRepository _repository;
  late final HydraExchangeRepository _exchangeRepository;
  CreditStatus? _status;
  AssistantNudge? _nudge;
  TelegramAnchorInfo? _anchorInfo;
  ShareEarnStatus? _shareStatus;
  ProviderEarnings? _providerEarnings;
  bool _advancedMode = false;
  bool _loading = true;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _repository = widget.repository ?? CreditRepository.instance;
    _exchangeRepository =
        widget.exchangeRepository ?? HydraExchangeRepository.instance;
    _refresh();
  }

  Future<void> _refresh() async {
    try {
      final status = await _repository.loadStatus();
      final nudge = await _repository.loadNudge();
      final anchorInfo = await _repository.loadTelegramAnchorInfo();
      final advancedMode = await _repository.isAdvancedModeEnabled();
      final shareStatus = await _exchangeRepository.loadShareEarnStatus();
      final providerEarnings = await _exchangeRepository.loadProviderEarnings();
      if (!mounted) return;
      setState(() {
        _status = status;
        _nudge = nudge;
        _anchorInfo = anchorInfo;
        _advancedMode = advancedMode;
        _shareStatus = shareStatus;
        _providerEarnings = providerEarnings;
      });
    } finally {
      if (mounted) {
        setState(() {
          _loading = false;
        });
      }
    }
  }

  Future<void> _handleNudgePrimary() async {
    final nudge = _nudge;
    if (nudge == null) return;
    setState(() {
      _busy = true;
    });
    try {
      if (nudge.kind == 'trial_offer') {
        await _repository.acceptTrialRoute();
        _showMessage('Faster route enabled.');
      } else {
        _showMessage('Top up flow arrives in the next phase.');
      }
      await _refresh();
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  Future<void> _dismissNudge() async {
    final nudge = _nudge;
    if (nudge == null) return;
    await _repository.dismissNudge(nudge.id);
    if (!mounted) return;
    setState(() {
      _nudge = null;
    });
  }

  Future<void> _toggleAdvanced(bool value) async {
    await _repository.setAdvancedModeEnabled(value);
    if (!mounted) return;
    setState(() {
      _advancedMode = value;
    });
  }

  Future<void> _toggleShareEarn(bool value) async {
    setState(() {
      _busy = true;
    });
    try {
      final shareStatus = await _exchangeRepository.setShareEarnEnabled(value);
      final providerEarnings = await _exchangeRepository.loadProviderEarnings();
      if (!mounted) return;
      setState(() {
        _shareStatus = shareStatus;
        _providerEarnings = providerEarnings;
      });
      _showMessage(
        value ? 'Share & Earn is now active.' : 'Share & Earn is turned off.',
      );
    } catch (_) {
      _showMessage(
        'Finish the advanced account setup first, then try Share & Earn again.',
      );
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  void _openAdvancedTools() {
    Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const MarketplaceScreen()),
    );
  }

  void _showMessage(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final status = _status;
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('Balance', style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 8),
          const Text(
            'Hydra keeps the default path simple. Faster routes and top ups appear only when you need them.',
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (status != null) ...[
            CreditStatusWidget(
              status: status,
              onTopUpPressed: () {
                Navigator.of(context).push(
                  MaterialPageRoute(builder: (_) => const DealsScreen()),
                );
              },
            ),
            const SizedBox(height: 12),
            _buildRouteCard(status),
            const SizedBox(height: 12),
            _buildAnchorCard(),
            if (_nudge != null) ...[
              const SizedBox(height: 12),
              _buildNudgeCard(_nudge!),
            ],
            const SizedBox(height: 12),
            _buildShareEarnCard(status),
            const SizedBox(height: 12),
            _buildAdvancedCard(status),
          ] else
            const Text('Balance status is unavailable right now.'),
        ],
      ),
    );
  }

  Widget _buildRouteCard(CreditStatus status) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Route status', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(status.routeMessage),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                Chip(
                  label: Text(
                    status.premiumAllowed ? 'Faster routes enabled' : 'Free path active',
                  ),
                ),
                if (status.fallbackToFree)
                  const Chip(label: Text('Graceful fallback active')),
                if (status.authorizedTelegram)
                  Chip(label: Text('Linked to ${_anchorInfo?.userName ?? 'Telegram'}')),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildAnchorCard() {
    final anchor = _anchorInfo;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Identity anchor', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            if (anchor?.authorized == true)
              Text(
                'Telegram is linked as ${anchor!.userName}. This unlocks a larger starter balance.',
              )
            else
              const Text(
                'The starter balance works immediately. Linking Telegram later raises the limit without adding a separate sign-up flow.',
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildNudgeCard(AssistantNudge nudge) {
    final primaryLabel = switch (nudge.kind) {
      'trial_offer' => 'Try faster route',
      'memory_guard' => 'Understood',
      _ => 'Top up later',
    };

    return Card(
      color: Theme.of(context).colorScheme.primaryContainer.withValues(alpha: 0.4),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(nudge.title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(nudge.message),
            const SizedBox(height: 12),
            Row(
              children: [
                FilledButton(
                  onPressed: _busy ? null : _handleNudgePrimary,
                  child: Text(primaryLabel),
                ),
                const SizedBox(width: 8),
                TextButton(
                  onPressed: _busy ? null : _dismissNudge,
                  child: const Text('Later'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildAdvancedCard(CreditStatus status) {
    final visible = _advancedMode || status.showAdvancedTools;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Advanced tools', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            const Text(
              'Provider and power-user tools stay hidden by default so the main flow stays simple.',
            ),
            const SizedBox(height: 12),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              value: _advancedMode,
              onChanged: _toggleAdvanced,
              title: const Text('Show advanced tools'),
            ),
            if (visible)
              Align(
                alignment: Alignment.centerLeft,
                child: FilledButton.tonal(
                  onPressed: _openAdvancedTools,
                  child: const Text('Open advanced tools'),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildShareEarnCard(CreditStatus status) {
    final visible = _advancedMode || status.showAdvancedTools;
    final shareStatus = _shareStatus;
    final earnings = _providerEarnings;

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Share & Earn', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            const Text(
              'When unlocked, Hydra can share spare capacity from this device and track starter earnings for you.',
            ),
            const SizedBox(height: 12),
            if (!visible)
              const Text(
                'This appears after your first top up or when you opt in to advanced tools.',
              )
            else ...[
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                value: shareStatus?.enabled ?? false,
                onChanged: _busy ? null : _toggleShareEarn,
                title: const Text('Enable Share & Earn'),
                subtitle: Text(
                  shareStatus?.toggleMessage ??
                      'Hydra can keep a lightweight relay session ready for sharing.',
                ),
              ),
              if (shareStatus != null) ...[
                const SizedBox(height: 8),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    Chip(
                      label: Text(
                        shareStatus.active ? 'Sharing live' : 'Sharing idle',
                      ),
                    ),
                    Chip(
                      label: Text(
                        shareStatus.onchainActive ? 'Staked route' : 'Starter route',
                      ),
                    ),
                    Chip(
                      label: Text(
                        'Score ${shareStatus.localRoutingScore.toStringAsFixed(0)}',
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Text(
                  'Projected earnings: ${earnings?.estimatedEarningsDisplay ?? shareStatus.estimatedEarningsDisplay}',
                ),
                Text(
                  'Settled earnings: ${earnings?.settledEarningsDisplay ?? shareStatus.settledEarningsDisplay}',
                ),
                if (shareStatus.endpointUrl.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(
                    'Relay endpoint: ${shareStatus.endpointUrl}',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
                if (shareStatus.lastError.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(
                    shareStatus.lastError,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ],
              ],
            ],
          ],
        ),
      ),
    );
  }
}
