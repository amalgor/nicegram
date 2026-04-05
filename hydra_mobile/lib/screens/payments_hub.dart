import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/widgets/address_tile.dart';
import 'package:hydra_mobile/widgets/tx_hash_tile.dart';
import 'package:url_launcher/url_launcher.dart';

class PaymentsHub extends StatefulWidget {
  const PaymentsHub({
    super.key,
    required this.repository,
    required this.activeProfile,
  });

  final HydraExchangeRepository repository;
  final WalletProfile? activeProfile;

  @override
  State<PaymentsHub> createState() => _PaymentsHubState();
}

class _PaymentsHubState extends State<PaymentsHub>
    with TickerProviderStateMixin {
  late final TabController _tabController = TabController(length: 3, vsync: this);
  List<DealOffer> _buyOffers = const [];
  List<DealOffer> _myDeals = const [];
  List<DealEscrowView> _outgoingEscrows = const [];
  List<DealEscrowView> _incomingEscrows = const [];
  WalletBalances? _balances;
  AgentRegistrationResult? _agentRegistration;
  DealBoardAllowance? _allowance;
  DealerProfile? _dealerProfile;
  bool _loading = true;
  bool _busy = false;
  String _currency = 'RUB';

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void didUpdateWidget(covariant PaymentsHub oldWidget) {
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
        _buyOffers = const [];
        _myDeals = const [];
        _outgoingEscrows = const [];
        _incomingEscrows = const [];
        _balances = null;
        _agentRegistration = null;
        _allowance = null;
        _dealerProfile = null;
        _loading = false;
      });
      return;
    }

    try {
      final buyOffers = await widget.repository.fetchDealOffers(currency: _currency);
      final myDeals = await widget.repository.fetchMyDealOffers(profileId: profile.id);
      final outgoing = await widget.repository.loadMyEscrows(
        role: 'buyer',
        profileId: profile.id,
      );
      final incoming = await widget.repository.loadMyEscrows(
        role: 'dealer',
        profileId: profile.id,
      );
      final balances = await widget.repository.loadBalances(profileId: profile.id);
      final agentRegistration = await widget.repository.loadAgentRegistration(
        profileId: profile.id,
      );
      final allowance = await widget.repository.loadDealBoardAllowance(
        profileId: profile.id,
      );
      final dealerProfile = await widget.repository.loadDealerProfile(
        profileId: profile.id,
      );

      if (!mounted) return;
      setState(() {
        _buyOffers = buyOffers;
        _myDeals = myDeals;
        _outgoingEscrows = outgoing;
        _incomingEscrows = incoming;
        _balances = balances;
        _agentRegistration = agentRegistration;
        _allowance = allowance;
        _dealerProfile = dealerProfile;
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

  Future<void> _registerAgent() async {
    await _runBusy(() async {
      final result = await widget.repository.registerAgent(
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Registered agent #${result.agentId}.');
    });
  }

  Future<void> _approveAllowance() async {
    final controller = TextEditingController(text: '100');
    final amount = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Approve DealBoard USDC'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(
            labelText: 'Allowance amount',
            suffixText: 'USDC',
            border: OutlineInputBorder(),
          ),
          keyboardType: const TextInputType.numberWithOptions(decimal: true),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(controller.text),
            child: const Text('Approve'),
          ),
        ],
      ),
    );
    if (amount == null || amount.trim().isEmpty) return;
    await _runBusy(() async {
      await widget.repository.approveDealBoardUsdc(
        amount: amount.trim(),
        profileId: widget.activeProfile?.id,
      );
      _showMessage('DealBoard allowance updated.');
    });
  }

  Future<void> _editDealerProfile() async {
    final current = _dealerProfile ?? DealerProfile.empty(widget.activeProfile?.address ?? '');
    final displayNameController = TextEditingController(text: current.displayName);
    final contactController = TextEditingController(text: current.contactHandle);
    final bankTransferController = TextEditingController(
      text: current.instructionsByMethod['bank_transfer'] ?? '',
    );
    final sbpController = TextEditingController(
      text: current.instructionsByMethod['sbp'] ?? '',
    );
    final notesController = TextEditingController(text: current.generalNotes);

    final saved = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Dealer Payment Profile'),
        content: SizedBox(
          width: 420,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: displayNameController,
                  decoration: const InputDecoration(
                    labelText: 'Display name',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: contactController,
                  decoration: const InputDecoration(
                    labelText: 'Contact handle or link',
                    hintText: '@hydra_seller or https://t.me/hydra_seller',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: bankTransferController,
                  minLines: 2,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'Bank transfer instructions',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: sbpController,
                  minLines: 2,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'SBP instructions',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: notesController,
                  minLines: 2,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'General notes',
                    border: OutlineInputBorder(),
                  ),
                ),
              ],
            ),
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

    if (saved != true) return;

    await _runBusy(() async {
      await widget.repository.saveDealerProfile(
        current.copyWith(
          displayName: displayNameController.text,
          contactHandle: contactController.text,
          instructionsByMethod: {
            if (bankTransferController.text.trim().isNotEmpty)
              'bank_transfer': bankTransferController.text.trim(),
            if (sbpController.text.trim().isNotEmpty)
              'sbp': sbpController.text.trim(),
          },
          generalNotes: notesController.text,
        ),
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Dealer payment profile saved.');
    });
  }

  Future<void> _createDealOffer() async {
    if (_agentRegistration == null) {
      _showMessage('Register an agent before publishing a deal.');
      return;
    }
    if (_dealerProfile?.isComplete != true) {
      _showMessage(
        'Add a complete dealer payment profile before publishing a deal.',
      );
      return;
    }
    if ((_allowance?.allowanceRaw ?? '0') == '0') {
      _showMessage('Approve DealBoard USDC allowance before publishing.');
      return;
    }

    final currencyController = TextEditingController(text: _currency);
    final rateController = TextEditingController(text: '100');
    final minController = TextEditingController(text: '1');
    final maxController = TextEditingController(text: '100');
    bool bankTransfer = true;
    bool sbp = false;

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setLocalState) => AlertDialog(
            title: const Text('Create Deal Offer'),
            content: SizedBox(
              width: 420,
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    TextField(
                      controller: currencyController,
                      decoration: const InputDecoration(
                        labelText: 'Fiat currency',
                        hintText: 'RUB',
                        border: OutlineInputBorder(),
                      ),
                      textCapitalization: TextCapitalization.characters,
                    ),
                    const SizedBox(height: 12),
                    TextField(
                      controller: rateController,
                      decoration: const InputDecoration(
                        labelText: 'Rate',
                        hintText: '100 = 100 fiat per 1 USDC',
                        border: OutlineInputBorder(),
                      ),
                      keyboardType:
                          const TextInputType.numberWithOptions(decimal: true),
                    ),
                    const SizedBox(height: 12),
                    TextField(
                      controller: minController,
                      decoration: const InputDecoration(
                        labelText: 'Minimum',
                        suffixText: 'USDC',
                        border: OutlineInputBorder(),
                      ),
                      keyboardType:
                          const TextInputType.numberWithOptions(decimal: true),
                    ),
                    const SizedBox(height: 12),
                    TextField(
                      controller: maxController,
                      decoration: const InputDecoration(
                        labelText: 'Maximum',
                        suffixText: 'USDC',
                        border: OutlineInputBorder(),
                      ),
                      keyboardType:
                          const TextInputType.numberWithOptions(decimal: true),
                    ),
                    const SizedBox(height: 12),
                    SwitchListTile(
                      value: bankTransfer,
                      title: const Text('bank_transfer'),
                      onChanged: (value) => setLocalState(() => bankTransfer = value),
                    ),
                    SwitchListTile(
                      value: sbp,
                      title: const Text('sbp'),
                      onChanged: (value) => setLocalState(() => sbp = value),
                    ),
                  ],
                ),
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
      },
    );

    if (confirmed != true) return;

    await _runBusy(() async {
      final methods = <String>[
        if (bankTransfer) 'bank_transfer',
        if (sbp) 'sbp',
      ];
      final result = await widget.repository.createDealOffer(
        agentId: _agentRegistration!.agentId,
        currency: currencyController.text.trim().toUpperCase(),
        rateRaw: _fixed6Raw(rateController.text.trim()),
        minAmountRaw: _fixed6Raw(minController.text.trim()),
        maxAmountRaw: _fixed6Raw(maxController.text.trim()),
        paymentMethods: methods,
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Deal offer #${result.offerId ?? "?"} published.');
    });
  }

  Future<void> _acceptDeal(DealOffer offer) async {
    final amountController = TextEditingController(text: offer.minAmount);
    final accepted = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Accept Deal #${offer.offerId}'),
        content: SizedBox(
          width: 360,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Dealer: ${_shortHash(offer.dealer)}'),
              Text('Rate: ${_formatFixed6(offer.rate)} ${offer.currency}/USDC'),
              Text('Range: ${offer.minAmount} - ${offer.maxAmount} USDC'),
              Text('Payment: ${offer.paymentMethods.join(", ")}'),
              const SizedBox(height: 12),
              TextField(
                controller: amountController,
                decoration: const InputDecoration(
                  labelText: 'Amount',
                  suffixText: 'USDC',
                  border: OutlineInputBorder(),
                ),
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
            onPressed: () => Navigator.of(context).pop(amountController.text.trim()),
            child: const Text('Lock Deal'),
          ),
        ],
      ),
    );
    if (accepted == null || accepted.isEmpty) return;

    await _runBusy(() async {
      final result = await widget.repository.acceptDeal(
        offerId: offer.offerId,
        usdcAmount: accepted,
        profileId: widget.activeProfile?.id,
      );
      _showMessage('Escrow #${result.escrowId ?? "?"} created.');
      if (result.escrowId != null && mounted) {
        final escrow = await widget.repository.checkEscrowStatus(
          escrowId: result.escrowId!,
        );
        if (mounted) {
          await _openEscrowDetail(escrow);
        }
      }
    });
  }

  Future<void> _openEscrowDetail(DealEscrowView escrow) async {
    final isBuyer =
        widget.activeProfile != null &&
        escrow.buyer.toLowerCase() == widget.activeProfile!.address.toLowerCase();
    final counterpartyAddress = isBuyer ? escrow.dealer : escrow.buyer;
    DealerProfile? dealerProfile;
    if (isBuyer) {
      try {
        dealerProfile = await widget.repository.loadDealerProfile(
          address: escrow.dealer,
        );
      } catch (_) {
        dealerProfile = null;
      }
    }

    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (context) {
        return SafeArea(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(20, 8, 20, 24),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Escrow #${escrow.escrowId}',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 12),
                  Text('Status: ${escrow.status}'),
                  Text('USDC: ${escrow.usdcAmount}'),
                  Text('Fiat: ${_formatFixed6(escrow.fiatAmount)}'),
                  Text('Expires: ${_formatEpoch(escrow.expiresAt)}'),
                  const SizedBox(height: 12),
                  AddressTile(
                    label: isBuyer ? 'Dealer' : 'Buyer',
                    address: counterpartyAddress,
                    caption: isBuyer
                        ? dealerProfile?.displayName
                        : 'Counterparty for this escrow',
                  ),
                  if (isBuyer && dealerProfile != null) ...[
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(16),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              'Payment instructions',
                              style: Theme.of(context).textTheme.titleMedium,
                            ),
                            const SizedBox(height: 8),
                            if (dealerProfile.contactHandle.trim().isNotEmpty)
                              ListTile(
                                contentPadding: EdgeInsets.zero,
                                title: const Text('Contact'),
                                subtitle: Text(dealerProfile.contactHandle),
                                trailing: IconButton(
                                  icon: const Icon(Icons.copy),
                                  onPressed: () async {
                                    await Clipboard.setData(
                                      ClipboardData(
                                        text: dealerProfile!.contactHandle,
                                      ),
                                    );
                                    if (context.mounted) {
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        const SnackBar(
                                          content: Text('Contact copied.'),
                                        ),
                                      );
                                    }
                                  },
                                ),
                                onTap: () {
                                  final handle = dealerProfile!.contactHandle.trim();
                                  if (handle.startsWith('http') ||
                                      handle.startsWith('mailto:') ||
                                      handle.startsWith('tg:')) {
                                    launchUrl(Uri.parse(handle));
                                  }
                                },
                              ),
                            for (final entry in dealerProfile.instructionsByMethod.entries)
                              ListTile(
                                contentPadding: EdgeInsets.zero,
                                title: Text(entry.key),
                                subtitle: Text(entry.value),
                                trailing: IconButton(
                                  icon: const Icon(Icons.copy),
                                  onPressed: () async {
                                    await Clipboard.setData(
                                      ClipboardData(text: entry.value),
                                    );
                                    if (context.mounted) {
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        const SnackBar(
                                          content: Text('Instruction copied.'),
                                        ),
                                      );
                                    }
                                  },
                                ),
                              ),
                            if (dealerProfile.generalNotes.trim().isNotEmpty)
                              Text(dealerProfile.generalNotes),
                          ],
                        ),
                      ),
                    ),
                  ],
                  if (escrow.status == 'Funded' && isBuyer)
                    FilledButton.icon(
                      onPressed: _busy
                          ? null
                          : () async {
                              Navigator.of(context).pop();
                              await _runBusy(() async {
                                await widget.repository.markFiatSent(
                                  escrowId: escrow.escrowId,
                                  profileId: widget.activeProfile?.id,
                                );
                                _showMessage('Marked fiat as sent.');
                              });
                            },
                      icon: const Icon(Icons.payments_outlined),
                      label: const Text('Mark fiat sent'),
                    ),
                  if (escrow.status == 'Sent' && !isBuyer)
                    Wrap(
                      spacing: 12,
                      runSpacing: 12,
                      children: [
                        FilledButton.icon(
                          onPressed: _busy
                              ? null
                              : () async {
                                  Navigator.of(context).pop();
                                  await _runBusy(() async {
                                    await widget.repository.confirmDealReceipt(
                                      escrowId: escrow.escrowId,
                                      profileId: widget.activeProfile?.id,
                                    );
                                    _showMessage('Escrow completed.');
                                  });
                                },
                          icon: const Icon(Icons.check_circle),
                          label: const Text('Confirm receipt'),
                        ),
                        OutlinedButton.icon(
                          onPressed: _busy
                              ? null
                              : () async {
                                  Navigator.of(context).pop();
                                  await _runBusy(() async {
                                    await widget.repository.rejectDeal(
                                      escrowId: escrow.escrowId,
                                      profileId: widget.activeProfile?.id,
                                    );
                                    _showMessage('Escrow rejected.');
                                  });
                                },
                          icon: const Icon(Icons.cancel_outlined),
                          label: const Text('Reject'),
                        ),
                      ],
                    ),
                  if ((escrow.status == 'Funded' || escrow.status == 'Sent') &&
                      DateTime.now().millisecondsSinceEpoch >
                          escrow.expiresAt * 1000)
                    OutlinedButton.icon(
                      onPressed: _busy
                          ? null
                          : () async {
                              Navigator.of(context).pop();
                              await _runBusy(() async {
                                await widget.repository.claimExpiredEscrow(
                                  escrowId: escrow.escrowId,
                                  profileId: widget.activeProfile?.id,
                                );
                                _showMessage('Expired escrow claimed.');
                              });
                            },
                      icon: const Icon(Icons.hourglass_bottom),
                      label: const Text('Claim expired'),
                    ),
                ],
              ),
            ),
          ),
        );
      },
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
    if (widget.activeProfile == null) {
      return _InfoCard(
        title: 'Payments need a wallet profile',
        message:
            'Create or import a wallet profile from the Balance header, then come back to buy, sell, or complete escrows.',
      );
    }

    return Column(
      children: [
        TabBar(
          controller: _tabController,
          tabs: const [
            Tab(text: 'Buy'),
            Tab(text: 'Sell'),
            Tab(text: 'Escrows'),
          ],
        ),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : TabBarView(
                  controller: _tabController,
                  children: [
                    _buildBuyTab(),
                    _buildSellTab(),
                    _buildEscrowsTab(),
                  ],
                ),
        ),
      ],
    );
  }

  Widget _buildBuyTab() {
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: TextEditingController(text: _currency),
                      onChanged: (value) => _currency = value,
                      decoration: const InputDecoration(
                        labelText: 'Fiat currency',
                        border: OutlineInputBorder(),
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  FilledButton.icon(
                    onPressed: _busy ? null : _refresh,
                    icon: const Icon(Icons.search),
                    label: const Text('Reload'),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          if (_buyOffers.isEmpty)
            const _InfoCard(
              title: 'No deals yet',
              message:
                  'No matching dealer offers are live for this fiat currency. Change the filter or create a sell offer from the next tab.',
            ),
          for (final offer in _buyOffers) _buildBuyOfferCard(offer),
        ],
      ),
    );
  }

  Widget _buildBuyOfferCard(DealOffer offer) {
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
            Text('Dealer: ${_shortHash(offer.dealer)}'),
            Text('Rate: ${_formatFixed6(offer.rate)} ${offer.currency}/USDC'),
            Text('Range: ${offer.minAmount} - ${offer.maxAmount} USDC'),
            Text('Payment: ${offer.paymentMethods.join(", ")}'),
            if (offer.reputation != null)
              Text(
                'Dealer score: ${offer.reputation!.formattedValue} (${offer.reputation!.feedbackCount} feedback)',
              ),
            const SizedBox(height: 12),
            FilledButton.icon(
              onPressed: _busy ? null : () => _acceptDeal(offer),
              icon: const Icon(Icons.handshake),
              label: const Text('Accept'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildSellTab() {
    final profile = widget.activeProfile!;
    final dealerProfile = _dealerProfile ?? DealerProfile.empty(profile.address);
    final allowanceReady = (_allowance?.allowanceRaw ?? '0') != '0';
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          AddressTile(
            label: 'Active dealer wallet',
            address: profile.address,
            caption: '${profile.name} • Base Sepolia',
          ),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Dealer readiness',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 12),
                  Text('ETH gas: ${_balances?.ethBalance ?? '0'}'),
                  Text('USDC balance: ${_balances?.usdcBalance ?? '0'}'),
                  Text(
                    _agentRegistration == null
                        ? 'Agent: missing'
                        : 'Agent: #${_agentRegistration!.agentId}',
                  ),
                  Text(
                    allowanceReady
                        ? 'DealBoard allowance: ${_allowance?.allowance ?? '0'} USDC'
                        : 'DealBoard allowance: missing',
                  ),
                  Text(
                    dealerProfile.isComplete
                        ? 'Payment profile: ready'
                        : 'Payment profile: incomplete',
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      FilledButton.icon(
                        onPressed: _busy || _agentRegistration != null
                            ? null
                            : _registerAgent,
                        icon: const Icon(Icons.badge_outlined),
                        label: Text(
                          _agentRegistration == null ? 'Register agent' : 'Agent ready',
                        ),
                      ),
                      OutlinedButton.icon(
                        onPressed: _busy ? null : _approveAllowance,
                        icon: const Icon(Icons.verified_user_outlined),
                        label: const Text('Approve'),
                      ),
                      OutlinedButton.icon(
                        onPressed: _busy ? null : _editDealerProfile,
                        icon: const Icon(Icons.account_box_outlined),
                        label: const Text('Payment profile'),
                      ),
                      FilledButton.icon(
                        onPressed: _busy ? null : _createDealOffer,
                        icon: const Icon(Icons.add_circle_outline),
                        label: const Text('Create deal'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
          if (_agentRegistration != null)
            TxHashTile(
              label: 'Agent registration tx',
              txHash: _agentRegistration!.txHash,
            ),
          if (_myDeals.isEmpty)
            const _InfoCard(
              title: 'No sell offers yet',
              message:
                  'Publish a deal offer after your payment profile and allowance are ready.',
            )
          else
            ..._myDeals.map(_buildMyDealCard),
        ],
      ),
    );
  }

  Widget _buildMyDealCard(DealOffer offer) {
    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'My deal #${offer.offerId}',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            Text('Rate: ${_formatFixed6(offer.rate)} ${offer.currency}/USDC'),
            Text('Range: ${offer.minAmount} - ${offer.maxAmount} USDC'),
            Text('Payment: ${offer.paymentMethods.join(", ")}'),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                OutlinedButton.icon(
                  onPressed: _busy
                      ? null
                      : () => _runBusy(() async {
                          await widget.repository.deactivateDealOffer(
                            offer.offerId,
                            profileId: widget.activeProfile?.id,
                          );
                          _showMessage('Deal offer deactivated.');
                        }),
                  icon: const Icon(Icons.pause_circle_outline),
                  label: const Text('Deactivate'),
                ),
                ActionChip(
                  label: Text('Offer #${offer.offerId}'),
                  onPressed: () async {
                    await Clipboard.setData(
                      ClipboardData(text: offer.offerId.toString()),
                    );
                    if (mounted) {
                      _showMessage('Offer ID copied.');
                    }
                  },
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildEscrowsTab() {
    return DefaultTabController(
      length: 2,
      child: Column(
        children: [
          const TabBar(tabs: [Tab(text: 'Outgoing'), Tab(text: 'Incoming')]),
          Expanded(
            child: TabBarView(
              children: [
                _buildEscrowList(_outgoingEscrows, 'No outgoing escrows yet.'),
                _buildEscrowList(_incomingEscrows, 'No incoming escrows yet.'),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildEscrowList(List<DealEscrowView> escrows, String emptyMessage) {
    if (escrows.isEmpty) {
      return _InfoCard(title: 'No escrows', message: emptyMessage);
    }
    return RefreshIndicator(
      onRefresh: _refresh,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          for (final escrow in escrows)
            Card(
              margin: const EdgeInsets.only(bottom: 12),
              child: ListTile(
                title: Text('Escrow #${escrow.escrowId}'),
                subtitle: Text(
                  '${escrow.status} • ${escrow.usdcAmount} USDC • expires ${_formatEpoch(escrow.expiresAt)}',
                ),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => _openEscrowDetail(escrow),
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
}

String _fixed6Raw(String value) {
  final trimmed = value.trim();
  if (trimmed.isEmpty) {
    return '0';
  }
  final parts = trimmed.split('.');
  final whole = parts.first.isEmpty ? '0' : parts.first;
  final fraction = parts.length > 1 ? parts[1] : '';
  final normalizedFraction = fraction.padRight(6, '0').substring(0, 6);
  return '$whole$normalizedFraction';
}

String _formatFixed6(String raw) {
  final digits = raw.replaceAll(RegExp(r'[^0-9-]'), '');
  final negative = digits.startsWith('-');
  final normalized = negative ? digits.substring(1) : digits;
  if (normalized.isEmpty) {
    return '0';
  }
  final padded = normalized.padLeft(7, '0');
  final whole = padded.substring(0, padded.length - 6);
  final fraction = padded.substring(padded.length - 6).replaceFirst(RegExp(r'0+$'), '');
  final value = fraction.isEmpty ? whole : '$whole.$fraction';
  return negative ? '-$value' : value;
}

String _shortHash(String value) {
  if (value.length <= 14) return value;
  return '${value.substring(0, 8)}...${value.substring(value.length - 6)}';
}

String _formatEpoch(int epochSeconds) {
  if (epochSeconds <= 0) return 'unknown';
  final date = DateTime.fromMillisecondsSinceEpoch(epochSeconds * 1000);
  return '${date.year}-${date.month.toString().padLeft(2, '0')}-${date.day.toString().padLeft(2, '0')} ${date.hour.toString().padLeft(2, '0')}:${date.minute.toString().padLeft(2, '0')}';
}
