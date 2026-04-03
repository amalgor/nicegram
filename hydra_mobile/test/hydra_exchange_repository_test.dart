import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_backend.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/mnemonic_store.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _FakeMnemonicStore implements MnemonicStore {
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
  int registerCalls = 0;
  String lastImportedMnemonic = '';
  MarketplaceConfigStatus configStatus = MarketplaceConfigStatus(
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
  );

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
  Future<WalletIdentity> getWalletPreview(String mnemonic) async => WalletIdentity(
    address: mnemonic.contains('legal') ? '0xabc' : '0xdef',
  );

  @override
  Future<WalletIdentity> importWallet(String mnemonic) async {
    lastImportedMnemonic = mnemonic;
    return getWalletPreview(mnemonic);
  }

  @override
  Future<DealOffer> getDealOffer({required int offerId}) async => DealOffer(
    offerId: offerId,
    dealer: '0xdealer',
    agentId: 7,
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
      agentId: 7,
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
  }) async {
    return [
      RouteOffer(
        offerId: 1,
        provider: '0xprovider',
        agentId: 7,
        endpointCiphertext: 'ciphertext',
        protocols: [protocol],
        region: region,
        pricePerGbRaw: '1000000',
        pricePerGb: '1',
        stakeAmountRaw: '5000000',
        stakeAmount: '5',
        bandwidthMbps: 150,
        createdAt: 1,
        deactivatedAt: 0,
        active: true,
        reputation: ReputationSummary(
          feedbackCount: 2,
          summaryValue: '1',
          valueDecimals: 0,
          formattedValue: '1',
        ),
      ),
    ];
  }

  @override
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async {
    registerCalls += 1;
    return AgentRegistrationResult(agentId: 11, txHash: '0xtx');
  }

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
    agentId: 11,
    endpointUrl: 'wss://relay.hydra-net.work?agent=11',
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
    agentId: 11,
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
  test('create/import flow stores normalized mnemonic and persists registration', () async {
    SharedPreferences.setMockInitialValues({});
    final store = _FakeMnemonicStore();
    final backend = _FakeBackend();
    final repository = HydraExchangeRepository(
      backend: backend,
      mnemonicStore: store,
    );

    final status = await repository.loadConfigStatus();
    expect(status.ready, isTrue);

    expect(await repository.hasWallet(), isFalse);

    final draft = await repository.createWallet();
    expect(draft.address, '0xabc');

    final imported = await repository.importWallet(
      'LEGAL   winner THANK year wave sausage worth useful legal winner thank yellow',
    );

    expect(imported.address, '0xabc');
    expect(
      store.value,
      'legal winner thank year wave sausage worth useful legal winner thank yellow',
    );
    expect(backend.lastImportedMnemonic, store.value);
    expect(await repository.hasWallet(), isTrue);

    final balances = await repository.loadBalances();
    expect(balances.ethBalance, '1');

    final registration = await repository.registerAgent();
    expect(registration.agentId, 11);
    expect(backend.registerCalls, 1);

    final restored = await repository.loadAgentRegistration();
    expect(restored?.agentId, 11);
    expect(restored?.txHash, '0xtx');
  });
}
