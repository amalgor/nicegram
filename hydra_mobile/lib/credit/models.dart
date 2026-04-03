class CreditStatus {
  CreditStatus({
    required this.anchorId,
    required this.anchorSource,
    required this.authorizedTelegram,
    required this.userName,
    required this.usageBytes,
    required this.usageSeconds,
    required this.debtMicroUsdc,
    required this.debtDisplay,
    required this.creditLimitMicroUsdc,
    required this.creditLimitDisplay,
    required this.utilizationPct,
    required this.paymentCount,
    required this.trialAccepted,
    required this.premiumAllowed,
    required this.fallbackToFree,
    required this.throttleFactor,
    required this.advancedUnlocked,
    required this.tier,
    required this.premiumRoutesAvailable,
    required this.premiumTrialAvailable,
    required this.premiumMateriallyBetter,
    required this.routeState,
    required this.routeMessage,
  });

  final String anchorId;
  final String anchorSource;
  final bool authorizedTelegram;
  final String userName;
  final int usageBytes;
  final int usageSeconds;
  final int debtMicroUsdc;
  final String debtDisplay;
  final int creditLimitMicroUsdc;
  final String creditLimitDisplay;
  final double utilizationPct;
  final int paymentCount;
  final bool trialAccepted;
  final bool premiumAllowed;
  final bool fallbackToFree;
  final double throttleFactor;
  final bool advancedUnlocked;
  final String tier;
  final bool premiumRoutesAvailable;
  final bool premiumTrialAvailable;
  final bool premiumMateriallyBetter;
  final String routeState;
  final String routeMessage;

  factory CreditStatus.fromJson(Map<String, dynamic> json) {
    return CreditStatus(
      anchorId: json['anchor_id'] as String? ?? '',
      anchorSource: json['anchor_source'] as String? ?? 'installation',
      authorizedTelegram: json['authorized_telegram'] as bool? ?? false,
      userName: json['user_name'] as String? ?? '',
      usageBytes: (json['usage_bytes'] as num?)?.toInt() ?? 0,
      usageSeconds: (json['usage_seconds'] as num?)?.toInt() ?? 0,
      debtMicroUsdc: (json['debt_micro_usdc'] as num?)?.toInt() ?? 0,
      debtDisplay: json['debt_display'] as String? ?? '0',
      creditLimitMicroUsdc:
          (json['credit_limit_micro_usdc'] as num?)?.toInt() ?? 0,
      creditLimitDisplay: json['credit_limit_display'] as String? ?? '0',
      utilizationPct: (json['utilization_pct'] as num?)?.toDouble() ?? 0,
      paymentCount: (json['payment_count'] as num?)?.toInt() ?? 0,
      trialAccepted: json['trial_accepted'] as bool? ?? false,
      premiumAllowed: json['premium_allowed'] as bool? ?? false,
      fallbackToFree: json['fallback_to_free'] as bool? ?? false,
      throttleFactor: (json['throttle_factor'] as num?)?.toDouble() ?? 1,
      advancedUnlocked: json['advanced_unlocked'] as bool? ?? false,
      tier: json['tier'] as String? ?? 'free',
      premiumRoutesAvailable: json['premium_routes_available'] as bool? ?? false,
      premiumTrialAvailable: json['premium_trial_available'] as bool? ?? false,
      premiumMateriallyBetter:
          json['premium_materially_better'] as bool? ?? false,
      routeState: json['route_state'] as String? ?? 'free',
      routeMessage: json['route_message'] as String? ?? '',
    );
  }

  double get usageFraction => utilizationPct.clamp(0.0, 1.0);

  bool get showAdvancedTools =>
      advancedUnlocked || tier == 'provider' || paymentCount >= 3;
}

class AssistantNudge {
  AssistantNudge({
    required this.id,
    required this.kind,
    required this.title,
    required this.message,
  });

  final String id;
  final String kind;
  final String title;
  final String message;

  factory AssistantNudge.fromJson(Map<String, dynamic> json) {
    return AssistantNudge(
      id: json['id'] as String? ?? '',
      kind: json['kind'] as String? ?? '',
      title: json['title'] as String? ?? '',
      message: json['message'] as String? ?? '',
    );
  }
}

class TelegramAnchorInfo {
  TelegramAnchorInfo({
    required this.authorized,
    required this.userName,
    required this.anchorId,
  });

  final bool authorized;
  final String userName;
  final String anchorId;

  factory TelegramAnchorInfo.fromJson(Map<String, dynamic> json) {
    return TelegramAnchorInfo(
      authorized: json['authorized'] as bool? ?? false,
      userName: json['user_name'] as String? ?? '',
      anchorId: json['anchor_id'] as String? ?? '',
    );
  }
}
