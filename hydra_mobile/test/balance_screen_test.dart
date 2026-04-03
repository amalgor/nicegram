import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/credit/credit_backend.dart';
import 'package:hydra_mobile/credit/models.dart';
import 'package:hydra_mobile/credit/credit_repository.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_backend.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/mnemonic_store.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/screens/balance_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _FakeCreditBackend implements CreditBackend {
  _FakeCreditBackend({
    required this.creditStatus,
    required this.anchorInfo,
    this.nudge,
  });

  final Map<String, dynamic> creditStatus;
  final Map<String, dynamic> anchorInfo;
  final Map<String, dynamic>? nudge;

  @override
  Future<void> acceptTrialRoute() async {}

  @override
  Future<void> dismissNudge(String nudgeId) async {}

  @override
  Future<CreditStatus> getCreditStatus() async =>
      CreditStatus.fromJson(creditStatus);

  @override
  Future<TelegramAnchorInfo> getTelegramAnchorInfo() async =>
      TelegramAnchorInfo.fromJson(anchorInfo);

  @override
  Future<AssistantNudge?> getNudge() async =>
      nudge == null ? null : AssistantNudge.fromJson(nudge!);
}

class _FakeMnemonicStore implements MnemonicStore {
  @override
  Future<void> deleteMnemonic() async {}

  @override
  Future<String?> readMnemonic() async => 'legal winner thank year wave sausage worth useful legal winner thank yellow';

  @override
  Future<void> writeMnemonic(String mnemonic) async {}
}

class _FakeExchangeBackend implements HydraExchangeBackend {
  @override
  Future<MarketplaceConfigStatus> getMarketplaceConfigStatus() async =>
      MarketplaceConfigStatus(
        state: 'ready',
        ready: true,
        enabled: true,
        reputationEnabled: true,
        chain: 'BASE-SEPOLIA',
        rpcUrl: 'https://sepolia.base.org',
        routeBookAddress: '0xroute',
        identityRegistryAddress: '0xidentity',
        reputationRegistryAddress: '0xreputation',
        usdcAddress: '0xusdc',
        message: 'ready',
      );

  @override
  Future<WalletDraft> createWallet() async =>
      WalletDraft(address: '0xabc', mnemonic: 'mnemonic');

  @override
  Future<AcceptDealResult> acceptDeal({
    required String mnemonic,
    required int offerId,
    required String usdcAmount,
  }) async => AcceptDealResult(escrowId: 1, txHash: '0xaccept');

  @override
  Future<TxHashResult> approveDealBoardUsdc({
    required String mnemonic,
    required String amount,
  }) async => TxHashResult(txHash: '0xapprove');

  @override
  Future<DealEscrowView> checkEscrowStatus({required int escrowId}) async =>
      DealEscrowView(
        escrowId: escrowId,
        offerId: 1,
        buyer: '0xbuyer',
        dealer: '0xdealer',
        usdcAmount: '1000000',
        fiatAmount: '100000000',
        status: 'Funded',
        createdAt: 1,
        expiresAt: 2,
      );

  @override
  Future<TxHashResult> claimExpiredEscrow({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xclaim');

  @override
  Future<OfferMutationResult> createOffer({
    required String mnemonic,
    required int agentId,
    required String endpointUrl,
    required List<String> protocols,
    required String region,
    required String pricePerGbRaw,
    required String stakeAmountRaw,
    required int bandwidthMbps,
  }) async => OfferMutationResult(offerId: 1, txHash: '0xcreate');

  @override
  Future<OfferMutationResult> deactivateOffer({
    required String mnemonic,
    required int offerId,
  }) async => OfferMutationResult(offerId: offerId, txHash: '0xdeactivate');

  @override
  Future<ProviderEarnings> getProviderEarnings() async => ProviderEarnings(
    agentId: 1,
    sessionCount: 0,
    bytesRelayed: 0,
    estimatedEarningsMicroUsdc: 0,
    estimatedEarningsDisplay: '0.000000',
    settledEarningsMicroUsdc: 0,
    settledEarningsDisplay: '0.000000',
    localRoutingScore: 50,
    pendingReputationSyncs: 0,
  );

  @override
  Future<ShareEarnStatus> getShareEarnStatus() async => ShareEarnStatus(
    enabled: false,
    active: false,
    unlocked: true,
    agentId: 1,
    endpointUrl: 'wss://relay.hydra-net.work?agent=1',
    region: 'US',
    protocol: 'vless',
    pricePerGbRaw: '1000000',
    pricePerGbDisplay: '1.000000',
    bandwidthMbps: 20,
    routeBookOfferId: null,
    onchainActive: false,
    lastError: '',
    lastAnnouncedAt: 0,
    estimatedEarningsDisplay: '0.000000',
    settledEarningsDisplay: '0.000000',
    localRoutingScore: 50,
    toggleMessage: 'ready',
    settings: ShareSettings(
      priceOverrideRaw: null,
      maxBandwidthMbps: null,
      wifiOnly: false,
      scheduleStartHour: null,
      scheduleEndHour: null,
    ),
  );

  @override
  Future<WalletBalances> getWalletBalances(String address) async => WalletBalances(
    address: address,
    chain: 'BASE-SEPOLIA',
    ethBalanceWei: '0',
    ethBalance: '0',
    usdcAddress: '0xusdc',
    usdcBalanceRaw: '0',
    usdcBalance: '0',
  );

  @override
  Future<WalletIdentity> getWalletPreview(String mnemonic) async =>
      WalletIdentity(address: '0xabc');

  @override
  Future<WalletIdentity> importWallet(String mnemonic) async =>
      WalletIdentity(address: '0xabc');

  @override
  Future<DealOffer> getDealOffer({required int offerId}) async => DealOffer(
    offerId: offerId,
    dealer: '0xdealer',
    agentId: 1,
    currency: 'RUB',
    rate: '1000000',
    minAmount: '1000000',
    maxAmount: '100000000',
    paymentMethods: const ['bank_transfer'],
    active: true,
    reputation: null,
  );

  @override
  Future<List<DealOffer>> listDealOffers({required String currency}) async => [
    DealOffer(
      offerId: 1,
      dealer: '0xdealer',
      agentId: 1,
      currency: currency,
      rate: '1000000',
      minAmount: '1000000',
      maxAmount: '100000000',
      paymentMethods: const ['bank_transfer'],
      active: true,
      reputation: null,
    ),
  ];

  @override
  Future<List<RouteOffer>> listRouteOffers({
    required String region,
    required String protocol,
  }) async => const [];

  @override
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async =>
      AgentRegistrationResult(agentId: 1, txHash: '0xagent');

  @override
  Future<ShareEarnStatus> setShareEarnEnabled({
    required bool enabled,
    String? mnemonic,
  }) async {
    return getShareEarnStatus();
  }

  @override
  Future<TxHashResult> markFiatSent({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xmark');

  @override
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
  }) async => TxHashResult(txHash: '0xfeedback');

  @override
  Future<ShareEarnStatus> updateShareSettings({
    String? priceOverrideRaw,
    int? maxBandwidthMbps,
    required bool wifiOnly,
    int? scheduleStartHour,
    int? scheduleEndHour,
  }) async {
    return getShareEarnStatus();
  }

  @override
  Future<OfferMutationResult> withdrawStake({
    required String mnemonic,
    required int offerId,
  }) async => OfferMutationResult(offerId: offerId, txHash: '0xwithdraw');
}

void main() {
  Future<(CreditRepository, HydraExchangeRepository)> makeRepository({
    required Map<String, dynamic> creditStatus,
    required Map<String, dynamic> anchorInfo,
    Map<String, dynamic>? nudge,
    Map<String, Object> prefs = const {},
  }) async {
    SharedPreferences.setMockInitialValues(prefs);
    return (
      CreditRepository(
        backend: _FakeCreditBackend(
          creditStatus: creditStatus,
          anchorInfo: anchorInfo,
          nudge: nudge,
        ),
        sharedPreferencesLoader: SharedPreferences.getInstance,
      ),
      HydraExchangeRepository(
        backend: _FakeExchangeBackend(),
        mnemonicStore: _FakeMnemonicStore(),
      ),
    );
  }

  Map<String, dynamic> baseStatus({bool advancedUnlocked = false}) => {
    'anchor_id': 'abc',
    'anchor_source': 'installation',
    'authorized_telegram': false,
    'user_name': '',
    'usage_bytes': 10,
    'usage_seconds': 20,
    'debt_micro_usdc': 100000,
    'debt_display': '0.1',
    'credit_limit_micro_usdc': 200000,
    'credit_limit_display': '0.2',
    'utilization_pct': 0.5,
    'payment_count': advancedUnlocked ? 3 : 0,
    'trial_accepted': false,
    'premium_allowed': false,
    'fallback_to_free': false,
    'throttle_factor': 1.0,
    'advanced_unlocked': advancedUnlocked,
    'tier': advancedUnlocked ? 'paid' : 'free',
    'premium_routes_available': true,
    'premium_trial_available': true,
    'premium_materially_better': true,
    'route_state': 'trial_available',
    'route_message': 'Hydra found faster routes that can be tried with one tap.',
  };

  testWidgets('balance screen keeps default wording non-crypto', (tester) async {
    final (repository, exchangeRepository) = await makeRepository(
      creditStatus: baseStatus(),
      anchorInfo: {'authorized': false, 'user_name': '', 'anchor_id': ''},
      nudge: {
        'id': 'trial:better-route',
        'kind': 'trial_offer',
        'title': 'Found a faster route',
        'message': 'Hydra found a better route for Telegram.',
      },
    );

    await tester.pumpWidget(
      MaterialApp(
        home: BalanceScreen(
          repository: repository,
          exchangeRepository: exchangeRepository,
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Balance'), findsOneWidget);
    expect(find.textContaining('wallet'), findsNothing);
    expect(find.textContaining('mnemonic'), findsNothing);
    expect(find.textContaining('blockchain'), findsNothing);
    expect(find.textContaining('ERC'), findsNothing);
    await tester.scrollUntilVisible(
      find.text('Try faster route'),
      200,
      scrollable: find.byType(Scrollable),
    );
    expect(find.text('Try faster route'), findsOneWidget);
  });

  testWidgets('advanced tools stay hidden until unlocked or enabled', (tester) async {
    final (lockedRepository, lockedExchangeRepository) = await makeRepository(
      creditStatus: baseStatus(),
      anchorInfo: {'authorized': false, 'user_name': '', 'anchor_id': ''},
    );

    await tester.pumpWidget(
      MaterialApp(
        home: BalanceScreen(
          key: const ValueKey('locked-balance'),
          repository: lockedRepository,
          exchangeRepository: lockedExchangeRepository,
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Open advanced tools'), findsNothing);

    final (unlockedRepository, unlockedExchangeRepository) = await makeRepository(
      creditStatus: baseStatus(advancedUnlocked: true),
      anchorInfo: {'authorized': true, 'user_name': 'Hydra User', 'anchor_id': 'tg'},
    );
    await tester.pumpWidget(
      MaterialApp(
        home: BalanceScreen(
          key: const ValueKey('unlocked-balance'),
          repository: unlockedRepository,
          exchangeRepository: unlockedExchangeRepository,
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(
      find.text('Open advanced tools'),
      200,
      scrollable: find.byType(Scrollable),
    );
    expect(find.text('Open advanced tools'), findsOneWidget);
  });
}
