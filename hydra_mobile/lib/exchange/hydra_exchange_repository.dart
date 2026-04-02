import 'package:hydra_mobile/exchange/hydra_exchange_backend.dart';
import 'package:hydra_mobile/exchange/mnemonic_store.dart';
import 'package:hydra_mobile/exchange/models.dart';
import 'package:shared_preferences/shared_preferences.dart';

typedef SharedPreferencesLoader = Future<SharedPreferences> Function();

class HydraExchangeRepository {
  HydraExchangeRepository({
    HydraExchangeBackend? backend,
    MnemonicStore? mnemonicStore,
    SharedPreferencesLoader? sharedPreferencesLoader,
  }) : _backend = backend ?? const FrbHydraExchangeBackend(),
       _mnemonicStore = mnemonicStore ?? SecureStorageMnemonicStore(),
       _sharedPreferencesLoader =
           sharedPreferencesLoader ?? SharedPreferences.getInstance;

  static HydraExchangeRepository? _instance;

  static HydraExchangeRepository get instance {
    _instance ??= HydraExchangeRepository();
    return _instance!;
  }

  static const agentIdKey = 'marketplace_agent_id';
  static const agentTxHashKey = 'marketplace_agent_tx_hash';
  static const regionKey = 'marketplace_region';
  static const protocolKey = 'marketplace_protocol';

  final HydraExchangeBackend _backend;
  final MnemonicStore _mnemonicStore;
  final SharedPreferencesLoader _sharedPreferencesLoader;

  Future<MarketplaceConfigStatus> loadConfigStatus() =>
      _backend.getMarketplaceConfigStatus();

  Future<bool> hasWallet() async => (await _mnemonicStore.readMnemonic()) != null;

  Future<WalletDraft> createWallet() => _backend.createWallet();

  Future<WalletIdentity> importWallet(String mnemonic) async {
    final normalized = normalizeMnemonic(mnemonic);
    final imported = await _backend.importWallet(normalized);
    await _mnemonicStore.writeMnemonic(normalized);
    await _clearAgentRegistration();
    return imported;
  }

  Future<WalletIdentity> loadWalletAddress() async {
    final mnemonic = await _requireMnemonic();
    return _backend.getWalletPreview(mnemonic);
  }

  Future<WalletBalances> loadBalances() async {
    final wallet = await loadWalletAddress();
    return _backend.getWalletBalances(wallet.address);
  }

  Future<List<RouteOffer>> fetchOffers({
    required String region,
    required String protocol,
  }) => _backend.listRouteOffers(
    region: region.trim().toUpperCase(),
    protocol: protocol.trim().toLowerCase(),
  );

  Future<AgentRegistrationResult> registerAgent() async {
    final mnemonic = await _requireMnemonic();
    final result = await _backend.registerAgent(mnemonic);
    final prefs = await _sharedPreferencesLoader();
    await prefs.setInt(agentIdKey, result.agentId);
    await prefs.setString(agentTxHashKey, result.txHash);
    return result;
  }

  Future<AgentRegistrationResult?> loadAgentRegistration() async {
    final prefs = await _sharedPreferencesLoader();
    final agentId = prefs.getInt(agentIdKey);
    if (agentId == null) {
      return null;
    }
    return AgentRegistrationResult(
      agentId: agentId,
      txHash: prefs.getString(agentTxHashKey) ?? '',
    );
  }

  Future<TxHashResult> submitFeedback({
    required int agentId,
    required bool positive,
    required String tag1,
  }) async {
    final mnemonic = await _requireMnemonic();
    return _backend.submitFeedback(
      mnemonic: mnemonic,
      agentId: agentId,
      positive: positive,
      tag1: tag1,
    );
  }

  Future<void> clearWallet() async {
    await _mnemonicStore.deleteMnemonic();
    await _clearAgentRegistration();
  }

  Future<void> saveFilters({
    required String region,
    required String protocol,
  }) async {
    final prefs = await _sharedPreferencesLoader();
    await prefs.setString(regionKey, region.trim().toUpperCase());
    await prefs.setString(protocolKey, protocol.trim().toLowerCase());
  }

  Future<Map<String, String>> loadFilters({
    required String defaultRegion,
    String defaultProtocol = 'vless',
  }) async {
    final prefs = await _sharedPreferencesLoader();
    return {
      'region': prefs.getString(regionKey) ?? defaultRegion.trim().toUpperCase(),
      'protocol':
          prefs.getString(protocolKey) ?? defaultProtocol.trim().toLowerCase(),
    };
  }

  static String normalizeMnemonic(String mnemonic) {
    return mnemonic
        .split(RegExp(r'\s+'))
        .where((part) => part.isNotEmpty)
        .map((part) => part.toLowerCase())
        .join(' ');
  }

  Future<String> _requireMnemonic() async {
    final mnemonic = await _mnemonicStore.readMnemonic();
    if (mnemonic == null || mnemonic.isEmpty) {
      throw StateError('Wallet not configured.');
    }
    return mnemonic;
  }

  Future<void> _clearAgentRegistration() async {
    final prefs = await _sharedPreferencesLoader();
    await prefs.remove(agentIdKey);
    await prefs.remove(agentTxHashKey);
  }
}
