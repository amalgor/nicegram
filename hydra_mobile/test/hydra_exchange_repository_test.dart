import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/exchange/hydra_exchange_repository.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'test_support/exchange_fakes.dart';

void main() {
  test('create/import flow stores normalized mnemonic and persists registration', () async {
    SharedPreferences.setMockInitialValues({});
    final store = InMemoryMnemonicStore();
    final backend = TestExchangeBackend(
      routeOffers: [
        RouteOffer(
          offerId: 1,
          provider: '0xabc',
          agentId: 11,
          endpointCiphertext: 'wss://relay.hydra-net.work?agent=11',
          protocols: const ['wss'],
          region: 'US',
          pricePerGbRaw: '1000000',
          pricePerGb: '1',
          stakeAmountRaw: '5000000',
          stakeAmount: '5',
          bandwidthMbps: 150,
          createdAt: 1,
          deactivatedAt: 0,
          active: true,
          reputation: ReputationSummary(
            feedbackCount: 2,
            summaryValue: '1',
            valueDecimals: 0,
            formattedValue: '1',
          ),
        ),
      ],
    );
    final repository = HydraExchangeRepository(
      backend: backend,
      mnemonicStore: store,
    );

    final status = await repository.loadConfigStatus();
    expect(status.ready, isTrue);

    expect(await repository.hasWallet(), isFalse);

    final draft = await repository.createWallet();
    expect(draft.address, '0xabc');

    final imported = await repository.importWallet(
      'LEGAL   winner THANK year wave sausage worth useful legal winner thank yellow',
    );

    expect(imported.address, '0xabc');
    final activeProfile = await repository.loadActiveProfile();
    expect(activeProfile, isNotNull);
    expect(
      store.mnemonicForProfile(activeProfile!.id),
      TestExchangeBackend.defaultMnemonic,
    );
    expect(backend.lastImportedMnemonic, TestExchangeBackend.defaultMnemonic);
    expect(await repository.hasWallet(), isTrue);

    final balances = await repository.loadBalances();
    expect(balances.ethBalance, '1');

    final registration = await repository.registerAgent();
    expect(registration.agentId, 11);
    expect(backend.registerCalls, 1);

    final restored = await repository.loadAgentRegistration();
    expect(restored?.agentId, 11);
    expect(restored?.txHash, '0xtx');

    final routeOffers = await repository.loadMyRouteOffers();
    expect(routeOffers, hasLength(1));
    expect(routeOffers.first.offerId, 1);
  });
}
