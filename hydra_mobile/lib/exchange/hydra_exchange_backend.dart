import 'dart:convert';

import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/src/rust/api/exchange.dart' as exchange_api;

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
  Future<AgentRegistrationResult> registerAgent(String mnemonic);
  Future<TxHashResult> submitFeedback({
    required String mnemonic,
    required int agentId,
    required bool positive,
    required String tag1,
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
  Future<AgentRegistrationResult> registerAgent(String mnemonic) async {
    final json = jsonDecode(
      await exchange_api.registerAgent(mnemonic: mnemonic),
    ) as Map<String, dynamic>;
    return AgentRegistrationResult.fromJson(json);
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
}
