import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/credit/credit_backend.dart';
import 'package:hydra_mobile/credit/credit_repository.dart';
import 'package:hydra_mobile/credit/models.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/screens/balance_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'test_support/exchange_fakes.dart';

class _FakeCreditBackend implements CreditBackend {
  _FakeCreditBackend({
    required this.creditStatus,
    required this.anchorInfo,
    this.nudge,
  });

  final Map<String, dynamic> creditStatus;
  final Map<String, dynamic> anchorInfo;
  final Map<String, dynamic>? nudge;

  @override
  Future<void> acceptTrialRoute() async {}

  @override
  Future<void> dismissNudge(String nudgeId) async {}

  @override
  Future<CreditStatus> getCreditStatus() async =>
      CreditStatus.fromJson(creditStatus);

  @override
  Future<TelegramAnchorInfo> getTelegramAnchorInfo() async =>
      TelegramAnchorInfo.fromJson(anchorInfo);

  @override
  Future<AssistantNudge?> getNudge() async =>
      nudge == null ? null : AssistantNudge.fromJson(nudge!);
}

void main() {
  Future<(CreditRepository, HydraExchangeRepository)> makeRepository({
    required Map<String, dynamic> creditStatus,
    required Map<String, dynamic> anchorInfo,
    Map<String, dynamic>? nudge,
    Map<String, Object> prefs = const {},
  }) async {
    SharedPreferences.setMockInitialValues(prefs);
    return (
      CreditRepository(
        backend: _FakeCreditBackend(
          creditStatus: creditStatus,
          anchorInfo: anchorInfo,
          nudge: nudge,
        ),
        sharedPreferencesLoader: SharedPreferences.getInstance,
      ),
      HydraExchangeRepository(
        backend: TestExchangeBackend(),
        mnemonicStore: InMemoryMnemonicStore(),
      ),
    );
  }

  Map<String, dynamic> baseStatus({bool advancedUnlocked = false}) => {
    'anchor_id': 'abc',
    'anchor_source': 'installation',
    'authorized_telegram': false,
    'user_name': '',
    'usage_bytes': 10,
    'usage_seconds': 20,
    'debt_micro_usdc': 100000,
    'debt_display': '0.1',
    'credit_limit_micro_usdc': 200000,
    'credit_limit_display': '0.2',
    'utilization_pct': 0.5,
    'payment_count': advancedUnlocked ? 3 : 0,
    'trial_accepted': false,
    'premium_allowed': false,
    'fallback_to_free': false,
    'throttle_factor': 1.0,
    'advanced_unlocked': advancedUnlocked,
    'tier': advancedUnlocked ? 'paid' : 'free',
    'premium_routes_available': true,
    'premium_trial_available': true,
    'premium_materially_better': true,
    'route_state': 'trial_available',
    'route_message': 'Hydra found faster routes that can be tried with one tap.',
  };

  testWidgets('balance screen keeps default wording non-crypto', (tester) async {
    final (repository, exchangeRepository) = await makeRepository(
      creditStatus: baseStatus(),
      anchorInfo: {'authorized': false, 'user_name': '', 'anchor_id': ''},
      nudge: {
        'id': 'trial:better-route',
        'kind': 'trial_offer',
        'title': 'Found a faster route',
        'message': 'Hydra found a better route for Telegram.',
      },
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: BalanceScreen(
            repository: repository,
            exchangeRepository: exchangeRepository,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Balance'), findsOneWidget);
    expect(find.textContaining('mnemonic'), findsNothing);
    expect(find.textContaining('blockchain'), findsNothing);
    expect(find.textContaining('ERC'), findsNothing);
    expect(find.text('Route status'), findsOneWidget);
    expect(find.textContaining('Create or import a wallet profile'), findsOneWidget);
  });

  testWidgets('advanced tab stays gated until the user enables advanced tools', (
    tester,
  ) async {
    final (unlockedRepository, unlockedExchangeRepository) = await makeRepository(
      creditStatus: baseStatus(advancedUnlocked: true),
      anchorInfo: {'authorized': true, 'user_name': 'Hydra User', 'anchor_id': 'tg'},
    );
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: BalanceScreen(
            key: const ValueKey('unlocked-balance'),
            repository: unlockedRepository,
            exchangeRepository: unlockedExchangeRepository,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Advanced'));
    await tester.pumpAndSettle();

    expect(
      find.text(
        'Enable advanced tools from Overview to open the raw marketplace surface.',
      ),
      findsOneWidget,
    );
  });
}
