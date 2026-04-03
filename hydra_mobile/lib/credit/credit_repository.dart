import 'package:hydra_mobile/credit/credit_backend.dart';
import 'package:hydra_mobile/credit/models.dart';
import 'package:shared_preferences/shared_preferences.dart';

typedef CreditPrefsLoader = Future<SharedPreferences> Function();

class CreditRepository {
  CreditRepository({
    CreditBackend? backend,
    CreditPrefsLoader? sharedPreferencesLoader,
  }) : _backend = backend ?? const FrbCreditBackend(),
       _sharedPreferencesLoader =
           sharedPreferencesLoader ?? SharedPreferences.getInstance;

  static CreditRepository? _instance;

  static CreditRepository get instance {
    _instance ??= CreditRepository();
    return _instance!;
  }

  static const advancedModeKey = 'advanced_mode';

  final CreditBackend _backend;
  final CreditPrefsLoader _sharedPreferencesLoader;

  Future<CreditStatus> loadStatus() => _backend.getCreditStatus();

  Future<AssistantNudge?> loadNudge() => _backend.getNudge();

  Future<void> dismissNudge(String nudgeId) => _backend.dismissNudge(nudgeId);

  Future<void> acceptTrialRoute() => _backend.acceptTrialRoute();

  Future<TelegramAnchorInfo> loadTelegramAnchorInfo() =>
      _backend.getTelegramAnchorInfo();

  Future<bool> isAdvancedModeEnabled() async {
    final prefs = await _sharedPreferencesLoader();
    return prefs.getBool(advancedModeKey) ?? false;
  }

  Future<void> setAdvancedModeEnabled(bool enabled) async {
    final prefs = await _sharedPreferencesLoader();
    await prefs.setBool(advancedModeKey, enabled);
  }
}
