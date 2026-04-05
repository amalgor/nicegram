import 'dart:convert';

import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/src/rust/api/exchange.dart' as exchange_api;
import 'package:hydra_mobile/src/rust/api/provider.dart' as provider_api;

abstract class HydraExchangeBackend {
  Future<MarketplaceConfigStatus> getMarketplaceConfigStatus();
  Future<WalletDraft> createWallet();
  Future<WalletIdentity> getWalletPreview(String mnemonic);
  Future<WalletIdentity> importWallet(String mnemonic);
  Future<WalletBalances> getWalletBalances(String address);
  Future<List<RouteOffer>> listRouteOffers({
    required String region,
    required String protocol,
  });
  Future<List<RouteOffer>> listMyRouteOffers({required String address});
  Future<RouteBookLifecycle> getRouteBookLifecycle();
  Future<AgentRegistrationResult> registerAgent(String mnemonic);
  Future<OfferMutationResult> createOffer({
    required String mnemonic,
    required int agentId,
    required String endpointUrl,
    required List<String> protocols,
    required String region,
    required String pricePerGbRaw,
    required String stakeAmountRaw,
    required int bandwidthMbps,
  });
  Future<OfferMutationResult> deactivateOffer({
    required String mnemonic,
    required int offerId,
  });
  Future<OfferMutationResult> withdrawStake({
    required String mnemonic,
    required int offerId,
  });
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
  });
  // P2P Deal Board
  Future<List<DealOffer>> listDealOffers({required String currency});
  Future<List<DealOffer>> listMyDealOffers({required String address});
  Future<DealOffer> getDealOffer({required int offerId});
  Future<OfferMutationResult> createDealOffer({
    required String mnemonic,
    required int agentId,
    required String currency,
    required String rateRaw,
    required String minAmountRaw,
    required String maxAmountRaw,
    required List<String> paymentMethods,
  });
  Future<OfferMutationResult> deactivateDealOffer({
    required String mnemonic,
    required int offerId,
  });
  Future<AcceptDealResult> acceptDeal({
    required String mnemonic,
    required int offerId,
    required String usdcAmount,
  });
  Future<TxHashResult> markFiatSent({
    required String mnemonic,
    required int escrowId,
  });
  Future<DealEscrowView> checkEscrowStatus({required int escrowId});
  Future<List<DealEscrowView>> listMyDealEscrows({
    required String address,
    String? role,
  });
  Future<TxHashResult> confirmDealReceipt({
    required String mnemonic,
    required int escrowId,
  });
  Future<TxHashResult> rejectDeal({
    required String mnemonic,
    required int escrowId,
  });
  Future<TxHashResult> claimExpiredEscrow({
    required String mnemonic,
    required int escrowId,
  });
  Future<DealBoardAllowance> getDealBoardAllowance({required String address});
  Future<TxHashResult> approveDealBoardUsdc({
    required String mnemonic,
    required String amount,
  });
  Future<Map<String, dynamic>> signDealerProfilePut({
    required String mnemonic,
    required String address,
    required int timestampMs,
    required String bodyJson,
  });

  Future<ShareEarnStatus> getShareEarnStatus({String? profileId});
  Future<ShareEarnStatus> setShareEarnEnabled({
    required bool enabled,
    String? profileId,
    String? mnemonic,
  });
  Future<ProviderEarnings> getProviderEarnings({String? profileId});
  Future<ShareEarnStatus> updateShareSettings({
    String? profileId,
    String? priceOverrideRaw,
    int? maxBandwidthMbps,
    required bool wifiOnly,
    int? scheduleStartHour,
    int? scheduleEndHour,
  });
  Future<ShareEarnStatus> syncProviderReputation({
    String? profileId,
    required String mnemonic,
  });
}

class FrbHydraExchangeBackend implements HydraExchangeBackend {
  const FrbHydraExchangeBackend();

  @override
  Future<MarketplaceConfigStatus> getMarketplaceConfigStatus() async {
    final json = jsonDecode(
      exchange_api.getMarketplaceConfigStatus(),
    ) as Map<String, dynamic>;
    return MarketplaceConfigStatus.fromJson(json);
  }

  @override
  Future<WalletDraft> createWallet() async {
    final json =
        jsonDecode(exchange_api.createWallet()) as Map<String, dynamic>;
    return WalletDraft.fromJson(json);
  }

  @override
  Future<WalletIdentity> getWalletPreview(String mnemonic) async {
    final json = jsonDecode(
      exchange_api.getWalletPreview(mnemonic: mnemonic),
    ) as Map<String, dynamic>;
    return WalletIdentity.fromJson(json);
  }

  @override
  Future<WalletIdentity> importWallet(String mnemonic) async {
    final json = jsonDecode(
      exchange_api.importWallet(mnemonic: mnemonic),
    ) as Map<String, dynamic>;
    return WalletIdentity.fromJson(json);
  }

  @override
  Future<WalletBalances> getWalletBalances(String address) async {
    final json = jsonDecode(
      await exchange_api.getWalletBalances(address: address),
    ) as Map<String, dynamic>;
    return WalletBalances.fromJson(json);
  }

  @override
  Future<List<RouteOffer>> listRouteOffers({
    required String region,
    required String protocol,
  }) async {
    final json = jsonDecode(
      await exchange_api.listRouteOffers(region: region, protocol: protocol),
    ) as List<dynamic>;
    return json
        .whereType<Map<String, dynamic>>()
        .map(RouteOffer.fromJson)
        .toList();
  }

  @override
  Future<List<RouteOffer>> listMyRouteOffers({required String address}) async {
    final json = jsonDecode(
      await exchange_api.listMyRouteOffers(address: address),
    ) as List<dynamic>;
    return json
        .whereType<Map<String, dynamic>>()
        .map(RouteOffer.fromJson)
        .toList();
  }

  @override
  Future<RouteBookLifecycle> getRouteBookLifecycle() async {
    final json = jsonDecode(
      await exchange_api.getRouteBookLifecycle(),
    ) as Map<String, dynamic>;
    return RouteBookLifecycle.fromJson(json);
  }

  @override
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async {
    final json = jsonDecode(
      await exchange_api.registerAgent(mnemonic: mnemonic),
    ) as Map<String, dynamic>;
    return AgentRegistrationResult.fromJson(json);
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
  }) async {
    final json = jsonDecode(
      await exchange_api.createOffer(
        mnemonic: mnemonic,
        agentId: BigInt.from(agentId),
        endpointUrl: endpointUrl,
        protocols: protocols,
        region: region,
        pricePerGbRaw: pricePerGbRaw,
        stakeAmountRaw: stakeAmountRaw,
        bandwidthMbps: BigInt.from(bandwidthMbps),
      ),
    ) as Map<String, dynamic>;
    return OfferMutationResult.fromJson(json);
  }

  @override
  Future<OfferMutationResult> deactivateOffer({
    required String mnemonic,
    required int offerId,
  }) async {
    final json = jsonDecode(
      await exchange_api.deactivateOffer(
        mnemonic: mnemonic,
        offerId: BigInt.from(offerId),
      ),
    ) as Map<String, dynamic>;
    return OfferMutationResult.fromJson(json);
  }

  @override
  Future<OfferMutationResult> withdrawStake({
    required String mnemonic,
    required int offerId,
  }) async {
    final json = jsonDecode(
      await exchange_api.withdrawStake(
        mnemonic: mnemonic,
        offerId: BigInt.from(offerId),
      ),
    ) as Map<String, dynamic>;
    return OfferMutationResult.fromJson(json);
  }

  @override
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
  }) async {
    final json = jsonDecode(
      await exchange_api.submitFeedback(
        mnemonic: mnemonic,
        agentId: BigInt.from(agentId),
        positive: positive,
        tag1: tag1,
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  // ── P2P Deal Board ────────────────────────────────────────────────────

  @override
  Future<List<DealOffer>> listDealOffers({required String currency}) async {
    final json = jsonDecode(
      await exchange_api.listDealOffers(currency: currency),
    ) as List<dynamic>;
    return json
        .whereType<Map<String, dynamic>>()
        .map(DealOffer.fromJson)
        .toList();
  }

  @override
  Future<List<DealOffer>> listMyDealOffers({required String address}) async {
    final json = jsonDecode(
      await exchange_api.listMyDealOffers(address: address),
    ) as List<dynamic>;
    return json
        .whereType<Map<String, dynamic>>()
        .map(DealOffer.fromJson)
        .toList();
  }

  @override
  Future<DealOffer> getDealOffer({required int offerId}) async {
    final json = jsonDecode(
      await exchange_api.getDealOffer(offerId: BigInt.from(offerId)),
    ) as Map<String, dynamic>;
    return DealOffer.fromJson(json);
  }

  @override
  Future<OfferMutationResult> createDealOffer({
    required String mnemonic,
    required int agentId,
    required String currency,
    required String rateRaw,
    required String minAmountRaw,
    required String maxAmountRaw,
    required List<String> paymentMethods,
  }) async {
    final json = jsonDecode(
      await exchange_api.createDealOffer(
        mnemonic: mnemonic,
        agentId: BigInt.from(agentId),
        currency: currency,
        rateRaw: rateRaw,
        minAmountRaw: minAmountRaw,
        maxAmountRaw: maxAmountRaw,
        paymentMethods: paymentMethods,
      ),
    ) as Map<String, dynamic>;
    return OfferMutationResult.fromJson(json);
  }

  @override
  Future<OfferMutationResult> deactivateDealOffer({
    required String mnemonic,
    required int offerId,
  }) async {
    final json = jsonDecode(
      await exchange_api.deactivateDealOffer(
        mnemonic: mnemonic,
        offerId: BigInt.from(offerId),
      ),
    ) as Map<String, dynamic>;
    return OfferMutationResult.fromJson(json);
  }

  @override
  Future<AcceptDealResult> acceptDeal({
    required String mnemonic,
    required int offerId,
    required String usdcAmount,
  }) async {
    final json = jsonDecode(
      await exchange_api.acceptDeal(
        mnemonic: mnemonic,
        offerId: BigInt.from(offerId),
        usdcAmount: usdcAmount,
      ),
    ) as Map<String, dynamic>;
    return AcceptDealResult.fromJson(json);
  }

  @override
  Future<TxHashResult> markFiatSent({
    required String mnemonic,
    required int escrowId,
  }) async {
    final json = jsonDecode(
      await exchange_api.markFiatSent(
        mnemonic: mnemonic,
        escrowId: BigInt.from(escrowId),
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  @override
  Future<DealEscrowView> checkEscrowStatus({required int escrowId}) async {
    final json = jsonDecode(
      await exchange_api.checkEscrowStatus(escrowId: BigInt.from(escrowId)),
    ) as Map<String, dynamic>;
    return DealEscrowView.fromJson(json);
  }

  @override
  Future<List<DealEscrowView>> listMyDealEscrows({
    required String address,
    String? role,
  }) async {
    final json = jsonDecode(
      await exchange_api.listMyDealEscrows(address: address, role: role),
    ) as List<dynamic>;
    return json
        .whereType<Map<String, dynamic>>()
        .map(DealEscrowView.fromJson)
        .toList();
  }

  @override
  Future<TxHashResult> confirmDealReceipt({
    required String mnemonic,
    required int escrowId,
  }) async {
    final json = jsonDecode(
      await exchange_api.confirmDealReceipt(
        mnemonic: mnemonic,
        escrowId: BigInt.from(escrowId),
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  @override
  Future<TxHashResult> rejectDeal({
    required String mnemonic,
    required int escrowId,
  }) async {
    final json = jsonDecode(
      await exchange_api.rejectDeal(
        mnemonic: mnemonic,
        escrowId: BigInt.from(escrowId),
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  @override
  Future<TxHashResult> claimExpiredEscrow({
    required String mnemonic,
    required int escrowId,
  }) async {
    final json = jsonDecode(
      await exchange_api.claimExpiredEscrow(
        mnemonic: mnemonic,
        escrowId: BigInt.from(escrowId),
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  @override
  Future<DealBoardAllowance> getDealBoardAllowance({
    required String address,
  }) async {
    final json = jsonDecode(
      await exchange_api.getDealBoardAllowance(address: address),
    ) as Map<String, dynamic>;
    return DealBoardAllowance.fromJson(json);
  }

  @override
  Future<TxHashResult> approveDealBoardUsdc({
    required String mnemonic,
    required String amount,
  }) async {
    final json = jsonDecode(
      await exchange_api.approveDealBoardUsdc(
        mnemonic: mnemonic,
        amount: amount,
      ),
    ) as Map<String, dynamic>;
    return TxHashResult.fromJson(json);
  }

  @override
  Future<Map<String, dynamic>> signDealerProfilePut({
    required String mnemonic,
    required String address,
    required int timestampMs,
    required String bodyJson,
  }) async {
    return jsonDecode(
          exchange_api.signDealerProfilePut(
            mnemonic: mnemonic,
            address: address,
            timestampMs: BigInt.from(timestampMs),
            bodyJson: bodyJson,
          ),
        )
        as Map<String, dynamic>;
  }

  @override
  Future<ShareEarnStatus> getShareEarnStatus({String? profileId}) async {
    final json = jsonDecode(
      await provider_api.getShareEarnStatus(profileId: profileId),
    ) as Map<String, dynamic>;
    return ShareEarnStatus.fromJson(json);
  }

  @override
  Future<ShareEarnStatus> setShareEarnEnabled({
    required bool enabled,
    String? profileId,
    String? mnemonic,
  }) async {
    final json = jsonDecode(
      await provider_api.setShareEarnEnabled(
        enabled: enabled,
        profileId: profileId,
        mnemonic: mnemonic,
      ),
    ) as Map<String, dynamic>;
    return ShareEarnStatus.fromJson(json);
  }

  @override
  Future<ProviderEarnings> getProviderEarnings({String? profileId}) async {
    final json = jsonDecode(
      await provider_api.getProviderEarnings(profileId: profileId),
    ) as Map<String, dynamic>;
    return ProviderEarnings.fromJson(json);
  }

  @override
  Future<ShareEarnStatus> updateShareSettings({
    String? profileId,
    String? priceOverrideRaw,
    int? maxBandwidthMbps,
    required bool wifiOnly,
    int? scheduleStartHour,
    int? scheduleEndHour,
  }) async {
    final json = jsonDecode(
      await provider_api.updateShareSettings(
        profileId: profileId,
        priceOverrideRaw: priceOverrideRaw,
        maxBandwidthMbps: maxBandwidthMbps == null
            ? null
            : BigInt.from(maxBandwidthMbps),
        wifiOnly: wifiOnly,
        scheduleStartHour: scheduleStartHour,
        scheduleEndHour: scheduleEndHour,
      ),
    ) as Map<String, dynamic>;
    return ShareEarnStatus.fromJson(json);
  }

  @override
  Future<ShareEarnStatus> syncProviderReputation({
    String? profileId,
    required String mnemonic,
  }) async {
    final json = jsonDecode(
      await provider_api.syncProviderReputation(
        profileId: profileId,
        mnemonic: mnemonic,
      ),
    ) as Map<String, dynamic>;
    return ShareEarnStatus.fromJson(json);
  }
}
