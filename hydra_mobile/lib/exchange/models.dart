class WalletIdentity {
  WalletIdentity({required this.address});

  final String address;

  factory WalletIdentity.fromJson(Map<String, dynamic> json) {
    return WalletIdentity(address: json['address'] as String? ?? '');
  }
}

class MarketplaceConfigStatus {
  MarketplaceConfigStatus({
    required this.state,
    required this.ready,
    required this.enabled,
    required this.reputationEnabled,
    required this.chain,
    required this.rpcUrl,
    required this.routeBookAddress,
    required this.identityRegistryAddress,
    required this.reputationRegistryAddress,
    required this.usdcAddress,
    required this.message,
  });

  final String state;
  final bool ready;
  final bool enabled;
  final bool reputationEnabled;
  final String chain;
  final String rpcUrl;
  final String routeBookAddress;
  final String identityRegistryAddress;
  final String reputationRegistryAddress;
  final String usdcAddress;
  final String message;

  bool get isDisabled => state == 'disabled';

  bool get isIncomplete => state == 'incomplete';

  factory MarketplaceConfigStatus.fromJson(Map<String, dynamic> json) {
    return MarketplaceConfigStatus(
      state: json['state'] as String? ?? 'incomplete',
      ready: json['ready'] as bool? ?? false,
      enabled: json['enabled'] as bool? ?? false,
      reputationEnabled: json['reputation_enabled'] as bool? ?? false,
      chain: json['chain'] as String? ?? '',
      rpcUrl: json['rpc_url'] as String? ?? '',
      routeBookAddress: json['route_book_address'] as String? ?? '',
      identityRegistryAddress:
          json['identity_registry_address'] as String? ?? '',
      reputationRegistryAddress:
          json['reputation_registry_address'] as String? ?? '',
      usdcAddress: json['usdc_address'] as String? ?? '',
      message: json['message'] as String? ?? '',
    );
  }
}

class WalletDraft extends WalletIdentity {
  WalletDraft({required super.address, required this.mnemonic});

  final String mnemonic;

  factory WalletDraft.fromJson(Map<String, dynamic> json) {
    return WalletDraft(
      mnemonic: json['mnemonic'] as String? ?? '',
      address: json['address'] as String? ?? '',
    );
  }
}

class WalletBalances {
  WalletBalances({
    required this.address,
    required this.chain,
    required this.ethBalanceWei,
    required this.ethBalance,
    required this.usdcAddress,
    required this.usdcBalanceRaw,
    required this.usdcBalance,
  });

  final String address;
  final String chain;
  final String ethBalanceWei;
  final String ethBalance;
  final String usdcAddress;
  final String usdcBalanceRaw;
  final String usdcBalance;

  factory WalletBalances.fromJson(Map<String, dynamic> json) {
    return WalletBalances(
      address: json['address'] as String? ?? '',
      chain: json['chain'] as String? ?? '',
      ethBalanceWei: json['eth_balance_wei'] as String? ?? '0',
      ethBalance: json['eth_balance'] as String? ?? '0',
      usdcAddress: json['usdc_address'] as String? ?? '',
      usdcBalanceRaw: json['usdc_balance_raw'] as String? ?? '0',
      usdcBalance: json['usdc_balance'] as String? ?? '0',
    );
  }
}

class ReputationSummary {
  ReputationSummary({
    required this.feedbackCount,
    required this.summaryValue,
    required this.valueDecimals,
    required this.formattedValue,
  });

  final int feedbackCount;
  final String summaryValue;
  final int valueDecimals;
  final String formattedValue;

  factory ReputationSummary.fromJson(Map<String, dynamic> json) {
    return ReputationSummary(
      feedbackCount: (json['feedback_count'] as num?)?.toInt() ?? 0,
      summaryValue: json['summary_value'] as String? ?? '0',
      valueDecimals: (json['value_decimals'] as num?)?.toInt() ?? 0,
      formattedValue: json['formatted_value'] as String? ?? '0',
    );
  }
}

class RouteOffer {
  RouteOffer({
    required this.offerId,
    required this.provider,
    required this.agentId,
    required this.endpointCiphertext,
    required this.protocols,
    required this.region,
    required this.pricePerGbRaw,
    required this.pricePerGb,
    required this.stakeAmountRaw,
    required this.stakeAmount,
    required this.bandwidthMbps,
    required this.createdAt,
    required this.deactivatedAt,
    required this.active,
    required this.reputation,
  });

  final int offerId;
  final String provider;
  final int agentId;
  final String endpointCiphertext;
  final List<String> protocols;
  final String region;
  final String pricePerGbRaw;
  final String pricePerGb;
  final String stakeAmountRaw;
  final String stakeAmount;
  final int bandwidthMbps;
  final int createdAt;
  final int deactivatedAt;
  final bool active;
  final ReputationSummary? reputation;

  factory RouteOffer.fromJson(Map<String, dynamic> json) {
    final reputationJson = json['reputation'];
    return RouteOffer(
      offerId: (json['offer_id'] as num?)?.toInt() ?? 0,
      provider: json['provider'] as String? ?? '',
      agentId: (json['agent_id'] as num?)?.toInt() ?? 0,
      endpointCiphertext: json['endpoint_ciphertext'] as String? ?? '',
      protocols:
          (json['protocols'] as List<dynamic>? ?? const [])
              .map((item) => item.toString())
              .toList(),
      region: json['region'] as String? ?? '',
      pricePerGbRaw: json['price_per_gb_raw'] as String? ?? '0',
      pricePerGb: json['price_per_gb'] as String? ?? '0',
      stakeAmountRaw: json['stake_amount_raw'] as String? ?? '0',
      stakeAmount: json['stake_amount'] as String? ?? '0',
      bandwidthMbps: (json['bandwidth_mbps'] as num?)?.toInt() ?? 0,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      deactivatedAt: (json['deactivated_at'] as num?)?.toInt() ?? 0,
      active: json['active'] as bool? ?? false,
      reputation: reputationJson is Map<String, dynamic>
          ? ReputationSummary.fromJson(reputationJson)
          : null,
    );
  }
}

class AgentRegistrationResult {
  AgentRegistrationResult({required this.agentId, required this.txHash});

  final int agentId;
  final String txHash;

  factory AgentRegistrationResult.fromJson(Map<String, dynamic> json) {
    return AgentRegistrationResult(
      agentId: (json['agent_id'] as num?)?.toInt() ?? 0,
      txHash: json['tx_hash'] as String? ?? '',
    );
  }
}

class TxHashResult {
  TxHashResult({required this.txHash});

  final String txHash;

  factory TxHashResult.fromJson(Map<String, dynamic> json) {
    return TxHashResult(txHash: json['tx_hash'] as String? ?? '');
  }
}

class OfferMutationResult {
  OfferMutationResult({required this.offerId, required this.txHash});

  final int? offerId;
  final String txHash;

  factory OfferMutationResult.fromJson(Map<String, dynamic> json) {
    return OfferMutationResult(
      offerId: (json['offer_id'] as num?)?.toInt(),
      txHash: json['tx_hash'] as String? ?? '',
    );
  }
}

// ── P2P Deal Board models ──────────────────────────────────────────────

class DealOffer {
  DealOffer({
    required this.offerId,
    required this.dealer,
    required this.agentId,
    required this.currency,
    required this.rate,
    required this.minAmount,
    required this.maxAmount,
    required this.paymentMethods,
    required this.active,
    required this.reputation,
  });

  final int offerId;
  final String dealer;
  final int agentId;
  final String currency;
  final String rate;
  final String minAmount;
  final String maxAmount;
  final List<String> paymentMethods;
  final bool active;
  final ReputationSummary? reputation;

  factory DealOffer.fromJson(Map<String, dynamic> json) {
    final reputationJson = json['reputation'];
    return DealOffer(
      offerId: (json['offer_id'] as num?)?.toInt() ?? 0,
      dealer: json['dealer'] as String? ?? '',
      agentId: (json['agent_id'] as num?)?.toInt() ?? 0,
      currency: json['currency'] as String? ?? '',
      rate: json['rate'] as String? ?? '0',
      minAmount: json['min_amount'] as String? ?? '0',
      maxAmount: json['max_amount'] as String? ?? '0',
      paymentMethods: (json['payment_methods'] as List<dynamic>? ?? const [])
          .map((item) => item.toString())
          .toList(),
      active: json['active'] as bool? ?? false,
      reputation: reputationJson is Map<String, dynamic>
          ? ReputationSummary.fromJson(reputationJson)
          : null,
    );
  }
}

class DealEscrowView {
  DealEscrowView({
    required this.escrowId,
    required this.offerId,
    required this.buyer,
    required this.dealer,
    required this.usdcAmount,
    required this.fiatAmount,
    required this.status,
    required this.createdAt,
    required this.expiresAt,
  });

  final int escrowId;
  final int offerId;
  final String buyer;
  final String dealer;
  final String usdcAmount;
  final String fiatAmount;
  final String status;
  final int createdAt;
  final int expiresAt;

  factory DealEscrowView.fromJson(Map<String, dynamic> json) {
    return DealEscrowView(
      escrowId: (json['escrow_id'] as num?)?.toInt() ?? 0,
      offerId: (json['offer_id'] as num?)?.toInt() ?? 0,
      buyer: json['buyer'] as String? ?? '',
      dealer: json['dealer'] as String? ?? '',
      usdcAmount: json['usdc_amount'] as String? ?? '0',
      fiatAmount: json['fiat_amount'] as String? ?? '0',
      status: json['status'] as String? ?? 'Funded',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      expiresAt: (json['expires_at'] as num?)?.toInt() ?? 0,
    );
  }
}

class AcceptDealResult {
  AcceptDealResult({required this.escrowId, required this.txHash});

  final int? escrowId;
  final String txHash;

  factory AcceptDealResult.fromJson(Map<String, dynamic> json) {
    return AcceptDealResult(
      escrowId: (json['escrow_id'] as num?)?.toInt(),
      txHash: json['tx_hash'] as String? ?? '',
    );
  }
}

class ShareSettings {
  ShareSettings({
    required this.priceOverrideRaw,
    required this.maxBandwidthMbps,
    required this.wifiOnly,
    required this.scheduleStartHour,
    required this.scheduleEndHour,
  });

  final String? priceOverrideRaw;
  final int? maxBandwidthMbps;
  final bool wifiOnly;
  final int? scheduleStartHour;
  final int? scheduleEndHour;

  factory ShareSettings.fromJson(Map<String, dynamic> json) {
    return ShareSettings(
      priceOverrideRaw: json['price_override_raw'] as String?,
      maxBandwidthMbps: (json['max_bandwidth_mbps'] as num?)?.toInt(),
      wifiOnly: json['wifi_only'] as bool? ?? false,
      scheduleStartHour: (json['schedule_start_hour'] as num?)?.toInt(),
      scheduleEndHour: (json['schedule_end_hour'] as num?)?.toInt(),
    );
  }
}

class ShareEarnStatus {
  ShareEarnStatus({
    required this.enabled,
    required this.active,
    required this.unlocked,
    required this.agentId,
    required this.endpointUrl,
    required this.region,
    required this.protocol,
    required this.pricePerGbRaw,
    required this.pricePerGbDisplay,
    required this.bandwidthMbps,
    required this.routeBookOfferId,
    required this.onchainActive,
    required this.lastError,
    required this.lastAnnouncedAt,
    required this.estimatedEarningsDisplay,
    required this.settledEarningsDisplay,
    required this.localRoutingScore,
    required this.toggleMessage,
    required this.settings,
  });

  final bool enabled;
  final bool active;
  final bool unlocked;
  final int? agentId;
  final String endpointUrl;
  final String region;
  final String protocol;
  final String pricePerGbRaw;
  final String pricePerGbDisplay;
  final int bandwidthMbps;
  final int? routeBookOfferId;
  final bool onchainActive;
  final String lastError;
  final int lastAnnouncedAt;
  final String estimatedEarningsDisplay;
  final String settledEarningsDisplay;
  final double localRoutingScore;
  final String toggleMessage;
  final ShareSettings settings;

  factory ShareEarnStatus.fromJson(Map<String, dynamic> json) {
    return ShareEarnStatus(
      enabled: json['enabled'] as bool? ?? false,
      active: json['active'] as bool? ?? false,
      unlocked: json['unlocked'] as bool? ?? false,
      agentId: (json['agent_id'] as num?)?.toInt(),
      endpointUrl: json['endpoint_url'] as String? ?? '',
      region: json['region'] as String? ?? 'US',
      protocol: json['protocol'] as String? ?? 'vless',
      pricePerGbRaw: json['price_per_gb_raw'] as String? ?? '0',
      pricePerGbDisplay: json['price_per_gb_display'] as String? ?? '0',
      bandwidthMbps: (json['bandwidth_mbps'] as num?)?.toInt() ?? 0,
      routeBookOfferId: (json['route_book_offer_id'] as num?)?.toInt(),
      onchainActive: json['onchain_active'] as bool? ?? false,
      lastError: json['last_error'] as String? ?? '',
      lastAnnouncedAt: (json['last_announced_at'] as num?)?.toInt() ?? 0,
      estimatedEarningsDisplay:
          json['estimated_earnings_display'] as String? ?? '0',
      settledEarningsDisplay:
          json['settled_earnings_display'] as String? ?? '0',
      localRoutingScore:
          (json['local_routing_score'] as num?)?.toDouble() ?? 0,
      toggleMessage: json['toggle_message'] as String? ?? '',
      settings: json['settings'] is Map<String, dynamic>
          ? ShareSettings.fromJson(json['settings'] as Map<String, dynamic>)
          : ShareSettings(
              priceOverrideRaw: null,
              maxBandwidthMbps: null,
              wifiOnly: false,
              scheduleStartHour: null,
              scheduleEndHour: null,
            ),
    );
  }
}

class ProviderEarnings {
  ProviderEarnings({
    required this.agentId,
    required this.sessionCount,
    required this.bytesRelayed,
    required this.estimatedEarningsMicroUsdc,
    required this.estimatedEarningsDisplay,
    required this.settledEarningsMicroUsdc,
    required this.settledEarningsDisplay,
    required this.localRoutingScore,
    required this.pendingReputationSyncs,
  });

  final int? agentId;
  final int sessionCount;
  final int bytesRelayed;
  final int estimatedEarningsMicroUsdc;
  final String estimatedEarningsDisplay;
  final int settledEarningsMicroUsdc;
  final String settledEarningsDisplay;
  final double localRoutingScore;
  final int pendingReputationSyncs;

  factory ProviderEarnings.fromJson(Map<String, dynamic> json) {
    return ProviderEarnings(
      agentId: (json['agent_id'] as num?)?.toInt(),
      sessionCount: (json['session_count'] as num?)?.toInt() ?? 0,
      bytesRelayed: (json['bytes_relayed'] as num?)?.toInt() ?? 0,
      estimatedEarningsMicroUsdc:
          (json['estimated_earnings_micro_usdc'] as num?)?.toInt() ?? 0,
      estimatedEarningsDisplay:
          json['estimated_earnings_display'] as String? ?? '0',
      settledEarningsMicroUsdc:
          (json['settled_earnings_micro_usdc'] as num?)?.toInt() ?? 0,
      settledEarningsDisplay:
          json['settled_earnings_display'] as String? ?? '0',
      localRoutingScore:
          (json['local_routing_score'] as num?)?.toDouble() ?? 0,
      pendingReputationSyncs:
          (json['pending_reputation_syncs'] as num?)?.toInt() ?? 0,
    );
  }
}
