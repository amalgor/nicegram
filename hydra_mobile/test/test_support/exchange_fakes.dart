import 'package:hydra_mobile/exchange/hydra_exchange_backend.dart';
import 'package:hydra_mobile/exchange/mnemonic_store.dart';
import 'package:hydra_mobile/exchange/models.dart';

class InMemoryMnemonicStore implements MnemonicStore {
  InMemoryMnemonicStore({String? legacyMnemonic}) {
    if (legacyMnemonic != null) {
      _mnemonics[null] = legacyMnemonic;
    }
  }

  final Map<String?, String> _mnemonics = {};

  String? get legacyMnemonic => _mnemonics[null];

  String? mnemonicForProfile(String? profileId) => _mnemonics[profileId];

  @override
  Future<void> deleteMnemonic({String? profileId}) async {
    _mnemonics.remove(profileId);
  }

  @override
  Future<String?> readMnemonic({String? profileId}) async => _mnemonics[profileId];

  @override
  Future<void> writeMnemonic(String mnemonic, {String? profileId}) async {
    _mnemonics[profileId] = mnemonic;
  }
}

WalletProfile buildWalletProfile({
  String id = 'default',
  String name = 'Primary Wallet',
  String address = '0xabc',
  String roleHint = 'general',
  String source = 'migrated',
  bool isBackedUp = true,
  int createdAt = 1,
}) {
  return WalletProfile(
    id: id,
    name: name,
    address: address,
    roleHint: roleHint,
    createdAt: createdAt,
    source: source,
    isBackedUp: isBackedUp,
  );
}

ShareEarnStatus buildShareEarnStatus({
  String profileId = 'default',
  String? runtimeProfileId,
  bool sharingActiveUnderOtherProfile = false,
  bool enabled = false,
  bool active = false,
  bool unlocked = true,
  int? agentId = 1,
  String agentTxHash = '0xagent',
  String endpointUrl = 'wss://relay.hydra-net.work?agent=1',
  String region = 'US',
  String protocol = 'wss',
  String pricePerGbRaw = '1000000',
  String pricePerGbDisplay = '1.000000',
  int bandwidthMbps = 20,
  int? routeBookOfferId,
  bool onchainActive = false,
  String lastError = '',
  int lastAnnouncedAt = 0,
  String estimatedEarningsDisplay = '0.000000',
  String settledEarningsDisplay = '0.000000',
  double localRoutingScore = 50,
  double pendingReputationDelta = 0,
  int pendingReputationSyncs = 0,
  double averageLatencyMs = 0,
  double averageThroughputMbps = 0,
  double uptimeRatio = 1,
  int recentFailures = 0,
  int lastOnchainSyncTime = 0,
  String toggleMessage = 'ready',
  ShareSettings? settings,
}) {
  return ShareEarnStatus(
    profileId: profileId,
    runtimeProfileId: runtimeProfileId,
    sharingActiveUnderOtherProfile: sharingActiveUnderOtherProfile,
    enabled: enabled,
    active: active,
    unlocked: unlocked,
    agentId: agentId,
    agentTxHash: agentTxHash,
    endpointUrl: endpointUrl,
    region: region,
    protocol: protocol,
    pricePerGbRaw: pricePerGbRaw,
    pricePerGbDisplay: pricePerGbDisplay,
    bandwidthMbps: bandwidthMbps,
    routeBookOfferId: routeBookOfferId,
    onchainActive: onchainActive,
    lastError: lastError,
    lastAnnouncedAt: lastAnnouncedAt,
    estimatedEarningsDisplay: estimatedEarningsDisplay,
    settledEarningsDisplay: settledEarningsDisplay,
    localRoutingScore: localRoutingScore,
    pendingReputationDelta: pendingReputationDelta,
    pendingReputationSyncs: pendingReputationSyncs,
    averageLatencyMs: averageLatencyMs,
    averageThroughputMbps: averageThroughputMbps,
    uptimeRatio: uptimeRatio,
    recentFailures: recentFailures,
    lastOnchainSyncTime: lastOnchainSyncTime,
    toggleMessage: toggleMessage,
    settings: settings ??
        ShareSettings(
          priceOverrideRaw: null,
          maxBandwidthMbps: null,
          wifiOnly: false,
          scheduleStartHour: null,
          scheduleEndHour: null,
        ),
  );
}

ProviderEarnings buildProviderEarnings({
  String profileId = 'default',
  int? agentId = 1,
  String agentTxHash = '0xagent',
  int sessionCount = 0,
  int successfulSessions = 0,
  int bytesRelayed = 0,
  int estimatedEarningsMicroUsdc = 0,
  String estimatedEarningsDisplay = '0.000000',
  int settledEarningsMicroUsdc = 0,
  String settledEarningsDisplay = '0.000000',
  double localRoutingScore = 50,
  double pendingReputationDelta = 0,
  int pendingReputationSyncs = 0,
  double averageLatencyMs = 0,
  double averageThroughputMbps = 0,
  double uptimeRatio = 1,
  int recentFailures = 0,
  int lastOnchainSyncTime = 0,
}) {
  return ProviderEarnings(
    profileId: profileId,
    agentId: agentId,
    agentTxHash: agentTxHash,
    sessionCount: sessionCount,
    successfulSessions: successfulSessions,
    bytesRelayed: bytesRelayed,
    estimatedEarningsMicroUsdc: estimatedEarningsMicroUsdc,
    estimatedEarningsDisplay: estimatedEarningsDisplay,
    settledEarningsMicroUsdc: settledEarningsMicroUsdc,
    settledEarningsDisplay: settledEarningsDisplay,
    localRoutingScore: localRoutingScore,
    pendingReputationDelta: pendingReputationDelta,
    pendingReputationSyncs: pendingReputationSyncs,
    averageLatencyMs: averageLatencyMs,
    averageThroughputMbps: averageThroughputMbps,
    uptimeRatio: uptimeRatio,
    recentFailures: recentFailures,
    lastOnchainSyncTime: lastOnchainSyncTime,
  );
}

class TestExchangeBackend implements HydraExchangeBackend {
  TestExchangeBackend({
    MarketplaceConfigStatus? configStatus,
    WalletDraft? walletDraft,
    WalletBalances? walletBalances,
    this.routeOffers = const [],
    this.dealOffers = const [],
    this.buyerEscrows = const [],
    this.dealerEscrows = const [],
    ShareEarnStatus? shareStatus,
    ProviderEarnings? providerEarnings,
    AgentRegistrationResult? registrationResult,
    DealBoardAllowance? allowance,
    RouteBookLifecycle? routeBookLifecycle,
  }) : configStatus = configStatus ??
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
       walletDraft = walletDraft ?? WalletDraft(address: '0xabc', mnemonic: defaultMnemonic),
       walletBalances = walletBalances ??
            WalletBalances(
              address: '0xabc',
              chain: 'BASE-SEPOLIA',
              ethBalanceWei: '1000000000000000000',
              ethBalance: '1',
              usdcAddress: '0xusdc',
              usdcBalanceRaw: '2500000',
              usdcBalance: '2.5',
            ),
       shareStatus = shareStatus ?? buildShareEarnStatus(),
       providerEarnings = providerEarnings ?? buildProviderEarnings(),
       registrationResult = registrationResult ??
            AgentRegistrationResult(agentId: 11, txHash: '0xtx'),
       allowance = allowance ??
            DealBoardAllowance(
              owner: '0xabc',
              spender: '0xdealboard',
              allowanceRaw: '0',
              allowance: '0',
            ),
       routeBookLifecycle = routeBookLifecycle ??
            RouteBookLifecycle(withdrawalDelaySecs: 3600);

  static const defaultMnemonic =
      'legal winner thank year wave sausage worth useful legal winner thank yellow';

  final MarketplaceConfigStatus configStatus;
  final WalletDraft walletDraft;
  final WalletBalances walletBalances;
  final List<RouteOffer> routeOffers;
  final List<DealOffer> dealOffers;
  final List<DealEscrowView> buyerEscrows;
  final List<DealEscrowView> dealerEscrows;
  final ShareEarnStatus shareStatus;
  final ProviderEarnings providerEarnings;
  final AgentRegistrationResult registrationResult;
  final DealBoardAllowance allowance;
  final RouteBookLifecycle routeBookLifecycle;

  int registerCalls = 0;
  String lastImportedMnemonic = '';

  @override
  Future<MarketplaceConfigStatus> getMarketplaceConfigStatus() async => configStatus;

  @override
  Future<WalletDraft> createWallet() async => walletDraft;

  @override
  Future<WalletIdentity> getWalletPreview(String mnemonic) async =>
      WalletIdentity(address: mnemonic.contains('legal') ? walletDraft.address : '0xdef');

  @override
  Future<WalletIdentity> importWallet(String mnemonic) async {
    lastImportedMnemonic = mnemonic;
    return getWalletPreview(mnemonic);
  }

  @override
  Future<WalletBalances> getWalletBalances(String address) async => walletBalances;

  @override
  Future<List<RouteOffer>> listRouteOffers({
    required String region,
    required String protocol,
  }) async => routeOffers
      .map(
        (offer) => RouteOffer(
          offerId: offer.offerId,
          provider: offer.provider,
          agentId: offer.agentId,
          endpointCiphertext: offer.endpointCiphertext,
          protocols: protocol.isEmpty ? offer.protocols : [protocol],
          region: region.isEmpty ? offer.region : region,
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
  Future<List<RouteOffer>> listMyRouteOffers({required String address}) async =>
      routeOffers.where((offer) => offer.provider.toLowerCase() == address.toLowerCase()).toList();

  @override
  Future<RouteBookLifecycle> getRouteBookLifecycle() async => routeBookLifecycle;

  @override
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async {
    registerCalls += 1;
    return registrationResult;
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
  Future<OfferMutationResult> withdrawStake({
    required String mnemonic,
    required int offerId,
  }) async => OfferMutationResult(offerId: offerId, txHash: '0xwithdraw');

  @override
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
  }) async => TxHashResult(txHash: '0xfeedback');

  @override
  Future<List<DealOffer>> listDealOffers({required String currency}) async =>
      dealOffers
          .map(
            (offer) => DealOffer(
              offerId: offer.offerId,
              dealer: offer.dealer,
              agentId: offer.agentId,
              currency: currency,
              rate: offer.rate,
              minAmount: offer.minAmount,
              maxAmount: offer.maxAmount,
              paymentMethods: offer.paymentMethods,
              active: offer.active,
              reputation: offer.reputation,
            ),
          )
          .toList();

  @override
  Future<List<DealOffer>> listMyDealOffers({required String address}) async =>
      dealOffers.where((offer) => offer.dealer.toLowerCase() == address.toLowerCase()).toList();

  @override
  Future<DealOffer> getDealOffer({required int offerId}) async =>
      dealOffers.firstWhere((offer) => offer.offerId == offerId);

  @override
  Future<OfferMutationResult> createDealOffer({
    required String mnemonic,
    required int agentId,
    required String currency,
    required String rateRaw,
    required String minAmountRaw,
    required String maxAmountRaw,
    required List<String> paymentMethods,
  }) async => OfferMutationResult(offerId: 1, txHash: '0xdealcreate');

  @override
  Future<OfferMutationResult> deactivateDealOffer({
    required String mnemonic,
    required int offerId,
  }) async => OfferMutationResult(offerId: offerId, txHash: '0xdealdeactivate');

  @override
  Future<AcceptDealResult> acceptDeal({
    required String mnemonic,
    required int offerId,
    required String usdcAmount,
  }) async => AcceptDealResult(escrowId: 1, txHash: '0xaccept');

  @override
  Future<TxHashResult> markFiatSent({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xmark');

  @override
  Future<DealEscrowView> checkEscrowStatus({required int escrowId}) async {
    final allEscrows = [...buyerEscrows, ...dealerEscrows];
    return allEscrows.firstWhere(
      (escrow) => escrow.escrowId == escrowId,
      orElse: () => DealEscrowView(
        escrowId: escrowId,
        offerId: 1,
        buyer: '0xbuyer',
        dealer: '0xdealer',
        usdcAmount: '1000000',
        fiatAmount: '100000000',
        status: 'Funded',
        createdAt: 1,
        expiresAt: 2,
      ),
    );
  }

  @override
  Future<List<DealEscrowView>> listMyDealEscrows({
    required String address,
    String? role,
  }) async {
    if (role == 'buyer') {
      return buyerEscrows;
    }
    if (role == 'dealer') {
      return dealerEscrows;
    }
    return [...buyerEscrows, ...dealerEscrows];
  }

  @override
  Future<TxHashResult> confirmDealReceipt({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xconfirm');

  @override
  Future<TxHashResult> rejectDeal({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xreject');

  @override
  Future<TxHashResult> claimExpiredEscrow({
    required String mnemonic,
    required int escrowId,
  }) async => TxHashResult(txHash: '0xclaim');

  @override
  Future<DealBoardAllowance> getDealBoardAllowance({required String address}) async =>
      allowance;

  @override
  Future<TxHashResult> approveDealBoardUsdc({
    required String mnemonic,
    required String amount,
  }) async => TxHashResult(txHash: '0xapprove');

  @override
  Future<Map<String, dynamic>> signDealerProfilePut({
    required String mnemonic,
    required String address,
    required int timestampMs,
    required String bodyJson,
  }) async => {
    'address': address.toLowerCase(),
    'timestamp_ms': timestampMs,
    'signature': '0xsigned',
    'path': '/api/dealer-profiles/${address.toLowerCase()}',
  };

  @override
  Future<ShareEarnStatus> getShareEarnStatus({String? profileId}) async =>
      shareStatus;

  @override
  Future<ShareEarnStatus> setShareEarnEnabled({
    required bool enabled,
    String? profileId,
    String? mnemonic,
  }) async => buildShareEarnStatus(
    profileId: profileId ?? shareStatus.profileId,
    enabled: enabled,
    active: enabled,
    agentId: shareStatus.agentId,
    agentTxHash: shareStatus.agentTxHash,
    endpointUrl: shareStatus.endpointUrl,
    region: shareStatus.region,
    protocol: shareStatus.protocol,
    routeBookOfferId: shareStatus.routeBookOfferId,
    onchainActive: shareStatus.onchainActive,
  );

  @override
  Future<ProviderEarnings> getProviderEarnings({String? profileId}) async =>
      providerEarnings;

  @override
  Future<ShareEarnStatus> updateShareSettings({
    String? profileId,
    String? priceOverrideRaw,
    int? maxBandwidthMbps,
    required bool wifiOnly,
    int? scheduleStartHour,
    int? scheduleEndHour,
  }) async => buildShareEarnStatus(
    profileId: profileId ?? shareStatus.profileId,
    enabled: shareStatus.enabled,
    active: shareStatus.active,
    agentId: shareStatus.agentId,
    agentTxHash: shareStatus.agentTxHash,
    endpointUrl: shareStatus.endpointUrl,
    region: shareStatus.region,
    protocol: shareStatus.protocol,
    settings: ShareSettings(
      priceOverrideRaw: priceOverrideRaw,
      maxBandwidthMbps: maxBandwidthMbps,
      wifiOnly: wifiOnly,
      scheduleStartHour: scheduleStartHour,
      scheduleEndHour: scheduleEndHour,
    ),
  );

  @override
  Future<ShareEarnStatus> syncProviderReputation({
    String? profileId,
    required String mnemonic,
  }) async => shareStatus;
}
