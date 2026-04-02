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
