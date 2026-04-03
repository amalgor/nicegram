import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_backend.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/mnemonic_store.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/screens/marketplace_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _FakeMnemonicStore implements MnemonicStore {
  _FakeMnemonicStore(this.value);

  String? value;

  @override
  Future<void> deleteMnemonic() async {
    value = null;
  }

  @override
  Future<String?> readMnemonic() async => value;

  @override
  Future<void> writeMnemonic(String mnemonic) async {
    value = mnemonic;
  }
}

class _FakeBackend implements HydraExchangeBackend {
  _FakeBackend({
    MarketplaceConfigStatus? configStatus,
    List<RouteOffer>? offers,
  }) : configStatus =
           configStatus ??
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
             message: 'Marketplace is configured for Base Sepolia.',
           ),
       offers =
           offers ??
           [
             RouteOffer(
               offerId: 1,
               provider: '0xprovider',
               agentId: 99,
               endpointCiphertext: 'ciphertext',
               protocols: ['vless'],
               region: 'US',
               pricePerGbRaw: '1000000',
               pricePerGb: '1',
               stakeAmountRaw: '5000000',
               stakeAmount: '5',
               bandwidthMbps: 200,
               createdAt: 1,
               deactivatedAt: 0,
               active: true,
               reputation: ReputationSummary(
                 feedbackCount: 4,
                 summaryValue: '1',
                 valueDecimals: 0,
                 formattedValue: '1',
               ),
             ),
           ];

  final MarketplaceConfigStatus configStatus;
  final List<RouteOffer> offers;

  @override
  Future<MarketplaceConfigStatus> getMarketplaceConfigStatus() async =>
      configStatus;

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
  Future<WalletDraft> createWallet() async => WalletDraft(
    mnemonic: 'legal winner thank year wave sausage worth useful legal winner thank yellow',
    address: '0xabc',
  );

  @override
  Future<WalletBalances> getWalletBalances(String address) async => WalletBalances(
    address: address,
    chain: 'BASE-SEPOLIA',
    ethBalanceWei: '1000000000000000000',
    ethBalance: '1',
    usdcAddress: '0xusdc',
    usdcBalanceRaw: '2500000',
    usdcBalance: '2.5',
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
    agentId: 15,
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
      agentId: 15,
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
  }) async => offers
      .map(
        (offer) => RouteOffer(
          offerId: offer.offerId,
          provider: offer.provider,
          agentId: offer.agentId,
          endpointCiphertext: offer.endpointCiphertext,
          protocols: [protocol],
          region: region,
          pricePerGbRaw: offer.pricePerGbRaw,
          pricePerGb: offer.pricePerGb,
          stakeAmountRaw: offer.stakeAmountRaw,
          stakeAmount: offer.stakeAmount,
          bandwidthMbps: offer.bandwidthMbps,
          createdAt: offer.createdAt,
          deactivatedAt: offer.deactivatedAt,
          active: offer.active,
          reputation: offer.reputation,
        ),
      )
      .toList();

  @override
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async =>
      AgentRegistrationResult(agentId: 15, txHash: '0xtx');

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
  Future<ShareEarnStatus> getShareEarnStatus() async => ShareEarnStatus(
    enabled: false,
    active: false,
    unlocked: true,
    agentId: 15,
    endpointUrl: 'wss://relay.hydra-net.work?agent=15',
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
  Future<ProviderEarnings> getProviderEarnings() async => ProviderEarnings(
    agentId: 15,
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
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
  }) async => TxHashResult(txHash: '0xfeedback');

  @override
  Future<OfferMutationResult> withdrawStake({
    required String mnemonic,
    required int offerId,
  }) async => OfferMutationResult(offerId: offerId, txHash: '0xwithdraw');
}

void main() {
  HydraExchangeRepository buildRepository({
    _FakeBackend? backend,
    String? mnemonic,
  }) {
    return HydraExchangeRepository(
      backend: backend ?? _FakeBackend(),
      mnemonicStore: _FakeMnemonicStore(mnemonic),
    );
  }

  testWidgets('marketplace shows wallet, registration, and offers', (tester) async {
    SharedPreferences.setMockInitialValues({
      HydraExchangeRepository.agentIdKey: 15,
      HydraExchangeRepository.agentTxHashKey: '0xtx',
      HydraExchangeRepository.regionKey: 'US',
      HydraExchangeRepository.protocolKey: 'vless',
    });

    final repository = buildRepository(
      mnemonic:
          'legal winner thank year wave sausage worth useful legal winner thank yellow',
      backend: _FakeBackend(),
    );

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    expect(find.text('Wallet'), findsOneWidget);
    expect(find.textContaining('Gas balance: 1 ETH'), findsOneWidget);
    expect(find.textContaining('Agent ID: 15'), findsOneWidget);

    await tester.scrollUntilVisible(
      find.text('Offer #1'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    expect(find.text('Offer #1'), findsOneWidget);
    expect(find.textContaining('Price / GB: 1 USDC'), findsOneWidget);
  });

  testWidgets('marketplace shows disabled config state without raw backend errors', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final repository = buildRepository(
      backend: _FakeBackend(
        configStatus: MarketplaceConfigStatus(
          state: 'disabled',
          ready: false,
          enabled: false,
          reputationEnabled: false,
          chain: 'BASE-SEPOLIA',
          rpcUrl: 'https://sepolia.base.org',
          routeBookAddress: '',
          identityRegistryAddress: '0xidentity',
          reputationRegistryAddress: '0xreputation',
          usdcAddress: '0xusdc',
          message: 'Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.',
        ),
      ),
      mnemonic:
          'legal winner thank year wave sausage worth useful legal winner thank yellow',
    );

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    await tester.scrollUntilVisible(
      find.text('Marketplace Disabled'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    expect(find.text('Marketplace Disabled'), findsOneWidget);
    expect(
      find.text('Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.'),
      findsWidgets,
    );
    expect(find.textContaining('Gas balance:'), findsNothing);
  });

  testWidgets('marketplace shows no offers state for empty query result', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final repository = buildRepository(
      backend: _FakeBackend(offers: const []),
      mnemonic:
          'legal winner thank year wave sausage worth useful legal winner thank yellow',
    );

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    await tester.scrollUntilVisible(
      find.text('No Offers'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    expect(find.text('No Offers'), findsOneWidget);
    expect(
      find.text('No active offers matched the selected region and protocol.'),
      findsOneWidget,
    );
  });
}
