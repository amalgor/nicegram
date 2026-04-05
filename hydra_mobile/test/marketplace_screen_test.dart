import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:hydra_mobile/screens/marketplace_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'test_support/exchange_fakes.dart';

void main() {
  HydraExchangeRepository buildRepository({
    TestExchangeBackend? backend,
    String? mnemonic,
  }) {
    return HydraExchangeRepository(
      backend: backend ?? TestExchangeBackend(),
      mnemonicStore: InMemoryMnemonicStore(legacyMnemonic: mnemonic),
    );
  }

  TestExchangeBackend buildBackend({
    MarketplaceConfigStatus? configStatus,
    List<RouteOffer>? offers,
  }) {
    return TestExchangeBackend(
      configStatus: configStatus,
      routeOffers: offers ??
          [
            RouteOffer(
              offerId: 1,
              provider: '0xprovider',
              agentId: 99,
              endpointCiphertext: 'wss://relay.hydra-net.work?agent=99',
              protocols: const ['wss'],
              region: 'US',
              pricePerGbRaw: '1000000',
              pricePerGb: '1',
              stakeAmountRaw: '5000000',
              stakeAmount: '5',
              bandwidthMbps: 200,
              createdAt: 1,
              deactivatedAt: 0,
              active: true,
              reputation: ReputationSummary(
                feedbackCount: 4,
                summaryValue: '1',
                valueDecimals: 0,
                formattedValue: '1',
              ),
            ),
          ],
    );
  }

  testWidgets('marketplace shows wallet, registration, and offers', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final repository = buildRepository(
      mnemonic: TestExchangeBackend.defaultMnemonic,
      backend: buildBackend(),
    );
    await repository.importWallet(TestExchangeBackend.defaultMnemonic);
    await repository.registerAgent();

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    expect(find.text('Wallet'), findsOneWidget);
    expect(find.textContaining('Gas balance: 1 ETH'), findsOneWidget);
    await tester.scrollUntilVisible(
      find.textContaining('Agent ID: 11'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.textContaining('Agent ID: 11'), findsOneWidget);

    await tester.scrollUntilVisible(
      find.text('Offer #1'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    expect(find.text('Offer #1'), findsOneWidget);
    expect(find.textContaining('Price / GB: 1 USDC'), findsOneWidget);
  });

  testWidgets('marketplace shows disabled config state without raw backend errors', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final repository = buildRepository(
      backend: buildBackend(
        configStatus: MarketplaceConfigStatus(
          state: 'disabled',
          ready: false,
          enabled: false,
          reputationEnabled: false,
          chain: 'BASE-SEPOLIA',
          rpcUrl: 'https://sepolia.base.org',
          routeBookAddress: '',
          identityRegistryAddress: '0xidentity',
          reputationRegistryAddress: '0xreputation',
          usdcAddress: '0xusdc',
          message: 'Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.',
        ),
      ),
      mnemonic: TestExchangeBackend.defaultMnemonic,
    );

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    await tester.scrollUntilVisible(
      find.text('Marketplace Disabled'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Marketplace Disabled'), findsOneWidget);
    expect(
      find.text('Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.'),
      findsWidgets,
    );
    expect(find.textContaining('Gas balance:'), findsNothing);
  });

  testWidgets('marketplace shows no offers state for empty query result', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final repository = buildRepository(
      backend: buildBackend(offers: const []),
      mnemonic: TestExchangeBackend.defaultMnemonic,
    );

    await tester.pumpWidget(
      MaterialApp(home: MarketplaceScreen(repository: repository)),
    );
    await tester.pumpAndSettle();

    await tester.scrollUntilVisible(
      find.text('No Offers'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('No Offers'), findsOneWidget);
    expect(
      find.text('No active offers matched the selected region and protocol.'),
      findsOneWidget,
    );
  });
}
