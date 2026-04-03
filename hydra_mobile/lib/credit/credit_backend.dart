import 'dart:convert';

import 'package:hydra_mobile/credit/models.dart';
import 'package:hydra_mobile/src/rust/api/credit.dart' as credit_api;

abstract class CreditBackend {
  Future<CreditStatus> getCreditStatus();
  Future<AssistantNudge?> getNudge();
  Future<void> dismissNudge(String nudgeId);
  Future<void> acceptTrialRoute();
  Future<TelegramAnchorInfo> getTelegramAnchorInfo();
}

class FrbCreditBackend implements CreditBackend {
  const FrbCreditBackend();

  @override
  Future<CreditStatus> getCreditStatus() async {
    final json = jsonDecode(await credit_api.getCreditStatus()) as Map<String, dynamic>;
    return CreditStatus.fromJson(json);
  }

  @override
  Future<AssistantNudge?> getNudge() async {
    final raw = await credit_api.getNudge();
    if (raw.trim().isEmpty || raw.trim() == 'null') {
      return null;
    }
    final json = jsonDecode(raw);
    if (json is! Map<String, dynamic>) {
      return null;
    }
    return AssistantNudge.fromJson(json);
  }

  @override
  Future<void> dismissNudge(String nudgeId) async {
    credit_api.dismissNudge(nudgeId: nudgeId);
  }

  @override
  Future<void> acceptTrialRoute() => credit_api.acceptTrialRoute();

  @override
  Future<TelegramAnchorInfo> getTelegramAnchorInfo() async {
    final json =
        jsonDecode(await credit_api.getTelegramAnchorInfo()) as Map<String, dynamic>;
    return TelegramAnchorInfo.fromJson(json);
  }
}
