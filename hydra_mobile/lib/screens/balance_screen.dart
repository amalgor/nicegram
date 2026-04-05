import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/credit/credit_repository.dart';
import 'package:hydra_mobile/credit/models.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/screens/marketplace_screen.dart';
import 'package:hydra_mobile/screens/payments_hub.dart';
import 'package:hydra_mobile/screens/providers_hub.dart';
import 'package:hydra_mobile/widgets/address_tile.dart';
import 'package:hydra_mobile/widgets/credit_status_widget.dart';
import 'package:hydra_mobile/widgets/receive_sheet.dart';

class BalanceScreen extends StatefulWidget {
  const BalanceScreen({super.key, this.repository, this.exchangeRepository});

  final CreditRepository? repository;
  final HydraExchangeRepository? exchangeRepository;

  @override
  State<BalanceScreen> createState() => _BalanceScreenState();
}

class _BalanceScreenState extends State<BalanceScreen>
    with AutomaticKeepAliveClientMixin, TickerProviderStateMixin {
  @override
  bool get wantKeepAlive => true;

  late final CreditRepository _repository;
  late final HydraExchangeRepository _exchangeRepository;
  late final TabController _tabController = TabController(length: 4, vsync: this);
  CreditStatus? _status;
  AssistantNudge? _nudge;
  TelegramAnchorInfo? _anchorInfo;
  WalletProfile? _activeProfile;
  List<WalletProfile> _profiles = const [];
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
      final profiles = await _exchangeRepository.listWalletProfiles();
      final activeProfile = await _exchangeRepository.loadActiveProfile();
      if (!mounted) return;
      setState(() {
        _status = status;
        _nudge = nudge;
        _anchorInfo = anchorInfo;
        _advancedMode = advancedMode;
        _profiles = profiles;
        _activeProfile = activeProfile;
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
        _tabController.animateTo(1);
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

  Future<void> _showProfileManagerSheet() async {
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (context) {
        return SafeArea(
          child: StatefulBuilder(
            builder: (context, setModalState) {
              Future<void> refreshModal() async {
                final profiles = await _exchangeRepository.listWalletProfiles();
                final active = await _exchangeRepository.loadActiveProfile();
                if (!mounted) return;
                setState(() {
                  _profiles = profiles;
                  _activeProfile = active;
                });
                setModalState(() {});
              }

              return Padding(
                padding: const EdgeInsets.fromLTRB(20, 8, 20, 24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Wallet profiles',
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: 16),
                    if (_profiles.isEmpty)
                      const Text(
                        'No wallet profiles yet. Create or import one to test payments and provider flows.',
                      )
                    else
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxHeight: 320),
                        child: ListView(
                          shrinkWrap: true,
                          children: [
                            for (final profile in _profiles)
                              ListTile(
                                leading: Icon(
                                  profile.id == _activeProfile?.id
                                      ? Icons.radio_button_checked
                                      : Icons.radio_button_off,
                                ),
                                title: Text(profile.name),
                                subtitle: Text(_shortHash(profile.address)),
                                onTap: () async {
                                  await _exchangeRepository.setActiveProfile(profile.id);
                                  await refreshModal();
                                },
                                trailing: PopupMenuButton<String>(
                                  onSelected: (value) async {
                                    if (value == 'rename') {
                                      await _renameProfile(profile);
                                    } else if (value == 'delete') {
                                      await _deleteProfile(profile);
                                    } else if (value == 'reveal') {
                                      await _revealPhrase(profile);
                                    }
                                    await refreshModal();
                                  },
                                  itemBuilder: (context) => const [
                                    PopupMenuItem(
                                      value: 'rename',
                                      child: Text('Rename'),
                                    ),
                                    PopupMenuItem(
                                      value: 'reveal',
                                      child: Text('Reveal phrase'),
                                    ),
                                    PopupMenuItem(
                                      value: 'delete',
                                      child: Text('Delete'),
                                    ),
                                  ],
                                ),
                              ),
                          ],
                        ),
                      ),
                    const SizedBox(height: 16),
                    Wrap(
                      spacing: 12,
                      runSpacing: 12,
                      children: [
                        FilledButton.icon(
                          onPressed: () async {
                            await _createProfile();
                            await refreshModal();
                          },
                          icon: const Icon(Icons.add_card),
                          label: const Text('Create'),
                        ),
                        OutlinedButton.icon(
                          onPressed: () async {
                            await _importProfile();
                            await refreshModal();
                          },
                          icon: const Icon(Icons.download_for_offline_outlined),
                          label: const Text('Import'),
                        ),
                      ],
                    ),
                  ],
                ),
              );
            },
          ),
        );
      },
    );
  }

  Future<void> _createProfile() async {
    final draft = await _exchangeRepository.createWallet();
    if (!mounted) return;
    final nameController = TextEditingController(
      text: 'Wallet ${_profiles.length + 1}',
    );
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Backup recovery phrase'),
        content: SizedBox(
          width: 420,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text(
                'Write down these 12 words. Hydra stores this profile only after you confirm the backup.',
              ),
              const SizedBox(height: 16),
              SelectableText(
                draft.mnemonic,
                style: const TextStyle(fontFamily: 'monospace', height: 1.4),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: nameController,
                decoration: const InputDecoration(
                  labelText: 'Profile name',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              Text('Address: ${draft.address}'),
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
            child: const Text('I saved it'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    await _exchangeRepository.importWalletProfile(
      draft.mnemonic,
      name: nameController.text,
      source: 'created',
      isBackedUp: true,
    );
    await _refresh();
    _showMessage('Wallet profile created.');
  }

  Future<void> _importProfile() async {
    if (!mounted) return;
    final nameController = TextEditingController();
    final phraseController = TextEditingController();
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Import wallet profile'),
        content: SizedBox(
          width: 420,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: nameController,
                decoration: const InputDecoration(
                  labelText: 'Profile name',
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: phraseController,
                minLines: 3,
                maxLines: 4,
                decoration: const InputDecoration(
                  labelText: 'Recovery phrase',
                  border: OutlineInputBorder(),
                ),
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
            child: const Text('Import'),
          ),
        ],
      ),
    );
    if (confirmed != true || phraseController.text.trim().isEmpty) return;
    await _exchangeRepository.importWalletProfile(
      phraseController.text,
      name: nameController.text,
      source: 'imported',
      isBackedUp: true,
    );
    await _refresh();
    _showMessage('Wallet profile imported.');
  }

  Future<void> _renameProfile(WalletProfile profile) async {
    final controller = TextEditingController(text: profile.name);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Rename profile'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(
            labelText: 'Profile name',
            border: OutlineInputBorder(),
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
    if (confirmed != true) return;
    await _exchangeRepository.renameProfile(profile.id, controller.text);
    await _refresh();
  }

  Future<void> _deleteProfile(WalletProfile profile) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete profile'),
        content: Text(
          'Delete ${profile.name}? This removes the locally stored recovery phrase for that profile.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    await _exchangeRepository.deleteProfile(profile.id);
    await _refresh();
  }

  Future<void> _revealPhrase(WalletProfile profile) async {
    final phrase = await _exchangeRepository.revealRecoveryPhrase(
      profileId: profile.id,
    );
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Recovery phrase: ${profile.name}'),
        content: SelectableText(
          phrase,
          style: const TextStyle(fontFamily: 'monospace', height: 1.4),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
          FilledButton(
            onPressed: () async {
              await Clipboard.setData(ClipboardData(text: phrase));
              if (context.mounted) {
                Navigator.of(context).pop();
                _showMessage('Recovery phrase copied.');
              }
            },
            child: const Text('Copy'),
          ),
        ],
      ),
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
    return Column(
      children: [
        _buildHeader(context),
        TabBar(
          controller: _tabController,
          tabs: const [
            Tab(text: 'Overview'),
            Tab(text: 'Payments'),
            Tab(text: 'Providers'),
            Tab(text: 'Advanced'),
          ],
        ),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : TabBarView(
                  controller: _tabController,
                  children: [
                    if (status != null)
                      _buildOverviewTab(status)
                    else
                      const Center(
                        child: Text('Balance status is unavailable right now.'),
                      ),
                    PaymentsHub(
                      repository: _exchangeRepository,
                      activeProfile: _activeProfile,
                    ),
                    ProvidersHub(
                      repository: _exchangeRepository,
                      activeProfile: _activeProfile,
                    ),
                    _buildAdvancedTab(),
                  ],
                ),
        ),
      ],
    );
  }

  Widget _buildHeader(BuildContext context) {
    final profile = _activeProfile;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 12),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Balance',
                          style: Theme.of(context).textTheme.headlineSmall,
                        ),
                        const SizedBox(height: 4),
                        Text(
                          profile == null
                              ? 'Create or import a wallet profile to test provider and payment flows end-to-end.'
                              : '${profile.name} • ${_shortHash(profile.address)}',
                        ),
                      ],
                    ),
                  ),
                  FilledButton.icon(
                    onPressed: _showProfileManagerSheet,
                    icon: const Icon(Icons.account_balance_wallet_outlined),
                    label: Text(profile == null ? 'Profiles' : 'Switch'),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  if (profile != null)
                    OutlinedButton.icon(
                      onPressed: () => showReceiveSheet(
                        context,
                        title: '${profile.name} receive address',
                        value: profile.address,
                        subtitle: 'Base Sepolia wallet address',
                        helperText:
                            'Use this address for both test ETH gas and Base Sepolia USDC.',
                      ),
                      icon: const Icon(Icons.qr_code_2),
                      label: const Text('Receive'),
                    ),
                  OutlinedButton.icon(
                    onPressed: _showProfileManagerSheet,
                    icon: const Icon(Icons.manage_accounts),
                    label: const Text('Manage'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildOverviewTab(CreditStatus status) {
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        children: [
          CreditStatusWidget(
            status: status,
            onTopUpPressed: () => _tabController.animateTo(1),
          ),
          const SizedBox(height: 12),
          _buildRouteCard(status),
          const SizedBox(height: 12),
          _buildAnchorCard(),
          if (_activeProfile != null) ...[
            const SizedBox(height: 12),
            AddressTile(
              label: 'Active wallet profile',
              address: _activeProfile!.address,
              caption:
                  '${_activeProfile!.name} • Base Sepolia test address for ETH and USDC',
            ),
          ],
          if (_nudge != null) ...[
            const SizedBox(height: 12),
            _buildNudgeCard(_nudge!),
          ],
          const SizedBox(height: 12),
          _buildTopLevelActions(),
          const SizedBox(height: 12),
          _buildAdvancedCard(status),
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
                    status.premiumAllowed
                        ? 'Faster routes enabled'
                        : 'Free path active',
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
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Assistant', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(nudge.title),
            const SizedBox(height: 4),
            Text(nudge.message),
            const SizedBox(height: 12),
            Row(
              children: [
                FilledButton(
                  onPressed: _busy ? null : _handleNudgePrimary,
                  child: Text(primaryLabel),
                ),
                const SizedBox(width: 12),
                TextButton(
                  onPressed: _busy ? null : _dismissNudge,
                  child: const Text('Dismiss'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildTopLevelActions() {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Manual testing cockpit',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            const Text(
              'Use Payments to top up or sell, Providers to publish relay-backed routes, and Advanced for raw on-chain tools.',
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                FilledButton.icon(
                  onPressed: () => _tabController.animateTo(1),
                  icon: const Icon(Icons.payments_outlined),
                  label: const Text('Payments'),
                ),
                FilledButton.icon(
                  onPressed: () => _tabController.animateTo(2),
                  icon: const Icon(Icons.route_outlined),
                  label: const Text('Providers'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildAdvancedCard(CreditStatus status) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Advanced tools', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(
              status.advancedUnlocked
                  ? 'Advanced tools are unlocked for route-marketplace debugging and raw on-chain actions.'
                  : 'Leave advanced tools off if you just need starter balance and one-tap route upgrades.',
            ),
            const SizedBox(height: 12),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              value: _advancedMode,
              title: const Text('Show advanced tools'),
              subtitle: const Text(
                'Marketplace, raw IDs, recovery phrase reveal, and direct contract actions.',
              ),
              onChanged: _busy ? null : _toggleAdvanced,
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildAdvancedTab() {
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
                  Text('Advanced', style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  const Text(
                    'Use this area for raw marketplace browsing, direct on-chain actions, and profile recovery operations.',
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          if (_advancedMode)
            const SizedBox(
              height: 900,
              child: MarketplaceScreen(embedded: true),
            )
          else
            const Card(
              child: Padding(
                padding: EdgeInsets.all(16),
                child: Text(
                  'Enable advanced tools from Overview to open the raw marketplace surface.',
                ),
              ),
            ),
        ],
      ),
    );
  }
}

String _shortHash(String value) {
  if (value.length <= 14) return value;
  return '${value.substring(0, 8)}...${value.substring(value.length - 6)}';
}
