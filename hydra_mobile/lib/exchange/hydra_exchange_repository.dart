import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;
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
    http.Client? httpClient,
  }) : _backend = backend ?? const FrbHydraExchangeBackend(),
       _mnemonicStore = mnemonicStore ?? SecureStorageMnemonicStore(),
       _sharedPreferencesLoader =
           sharedPreferencesLoader ?? SharedPreferences.getInstance,
       _httpClient = httpClient ?? http.Client();

  static HydraExchangeRepository? _instance;

  static HydraExchangeRepository get instance {
    _instance ??= HydraExchangeRepository();
    return _instance!;
  }

  static const _profilesKey = 'wallet_profiles_v1';
  static const _activeProfileIdKey = 'wallet_profiles_active_id';
  static const _legacyAgentIdKey = 'marketplace_agent_id';
  static const _legacyAgentTxHashKey = 'marketplace_agent_tx_hash';
  static const _defaultProfileId = 'default';
  static const _workerBaseUrl = 'https://relay.hydra-net.work';

  final HydraExchangeBackend _backend;
  final MnemonicStore _mnemonicStore;
  final SharedPreferencesLoader _sharedPreferencesLoader;
  final http.Client _httpClient;

  bool _profilesInitialized = false;

  Future<MarketplaceConfigStatus> loadConfigStatus() =>
      _backend.getMarketplaceConfigStatus();

  Future<bool> hasWallet() async => (await loadActiveProfile()) != null;

  Future<WalletDraft> createWallet() => _backend.createWallet();

  Future<List<WalletProfile>> listWalletProfiles() async {
    final prefs = await _prefs();
    return _decodeProfiles(prefs.getString(_profilesKey));
  }

  Future<WalletProfile?> loadActiveProfile() async {
    await _ensureProfileStoreInitialized();
    final prefs = await _prefs();
    final profiles = _decodeProfiles(prefs.getString(_profilesKey));
    if (profiles.isEmpty) {
      return null;
    }
    final activeId = prefs.getString(_activeProfileIdKey) ?? profiles.first.id;
    return profiles.cast<WalletProfile?>().firstWhere(
      (profile) => profile?.id == activeId,
      orElse: () => profiles.first,
    );
  }

  Future<void> setActiveProfile(String profileId) async {
    final profile = await _requireProfile(profileId);
    final prefs = await _prefs();
    await prefs.setString(_activeProfileIdKey, profile.id);
  }

  Future<WalletProfile> importWalletProfile(
    String mnemonic, {
    String? name,
    String roleHint = 'general',
    String source = 'imported',
    bool isBackedUp = true,
  }) async {
    await _ensureProfileStoreInitialized();
    final normalized = normalizeMnemonic(mnemonic);
    final imported = await _backend.importWallet(normalized);
    final prefs = await _prefs();
    final profiles = _decodeProfiles(prefs.getString(_profilesKey));
    final profile = WalletProfile(
      id: _generateProfileId(profiles),
      name: _defaultProfileName(name, profiles.length + 1),
      address: imported.address,
      roleHint: roleHint,
      createdAt: DateTime.now().millisecondsSinceEpoch,
      source: source,
      isBackedUp: isBackedUp,
    );
    await _mnemonicStore.writeMnemonic(normalized, profileId: profile.id);
    profiles.add(profile);
    await _saveProfiles(profiles, activeProfileId: profile.id);
    return profile;
  }

  Future<void> renameProfile(String profileId, String newName) async {
    final prefs = await _prefs();
    final profiles = _decodeProfiles(prefs.getString(_profilesKey));
    final updated = profiles
        .map(
          (profile) => profile.id == profileId
              ? profile.copyWith(name: _defaultProfileName(newName, 0))
              : profile,
        )
        .toList();
    await _saveProfiles(
      updated,
      activeProfileId: prefs.getString(_activeProfileIdKey),
    );
  }

  Future<void> deleteProfile(String profileId) async {
    final shareStatus = await loadShareEarnStatus(profileId: profileId);
    if (shareStatus.enabled || shareStatus.active) {
      throw StateError(
        'Disable Share & Earn for this profile before deleting it.',
      );
    }

    final prefs = await _prefs();
    final profiles = _decodeProfiles(prefs.getString(_profilesKey));
    final target = profiles.cast<WalletProfile?>().firstWhere(
      (profile) => profile?.id == profileId,
      orElse: () => null,
    );
    if (target == null) {
      return;
    }

    final remaining = profiles.where((profile) => profile.id != profileId).toList();
    await _mnemonicStore.deleteMnemonic(profileId: profileId);
    await _clearAgentRegistration(profileId);
    await _saveProfiles(
      remaining,
      activeProfileId: remaining.isEmpty
          ? null
          : (prefs.getString(_activeProfileIdKey) == profileId
                ? remaining.first.id
                : prefs.getString(_activeProfileIdKey)),
    );
  }

  Future<String> revealRecoveryPhrase({String? profileId}) async =>
      _requireMnemonic(profileId: profileId);

  Future<WalletIdentity> importWallet(String mnemonic) async {
    final profile = await importWalletProfile(mnemonic);
    return WalletIdentity(address: profile.address);
  }

  Future<WalletIdentity> loadWalletAddress({String? profileId}) async {
    final profile = await _resolveProfile(profileId);
    if (profile == null) {
      throw StateError('Wallet not configured.');
    }
    return WalletIdentity(address: profile.address);
  }

  Future<WalletBalances> loadBalances({String? profileId}) async {
    final wallet = await loadWalletAddress(profileId: profileId);
    return _backend.getWalletBalances(wallet.address);
  }

  Future<List<RouteOffer>> fetchOffers({
    required String region,
    required String protocol,
  }) => _backend.listRouteOffers(
    region: region.trim().toUpperCase(),
    protocol: protocol.trim().toLowerCase(),
  );

  Future<List<RouteOffer>> loadMyRouteOffers({String? profileId}) async {
    final profile = await _requireProfile(profileId);
    return _backend.listMyRouteOffers(address: profile.address);
  }

  Future<RouteBookLifecycle> loadRouteBookLifecycle() =>
      _backend.getRouteBookLifecycle();

  Future<AgentRegistrationResult> registerAgent({String? profileId}) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    final result = await _backend.registerAgent(mnemonic);
    await _cacheAgentRegistration(
      profileId: (await _requireProfile(profileId)).id,
      result: result,
    );
    return result;
  }

  Future<OfferMutationResult> createOffer({
    required int agentId,
    required String endpointUrl,
    required List<String> protocols,
    required String region,
    required String pricePerGbRaw,
    required String stakeAmountRaw,
    required int bandwidthMbps,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.createOffer(
      mnemonic: mnemonic,
      agentId: agentId,
      endpointUrl: endpointUrl,
      protocols: protocols,
      region: region,
      pricePerGbRaw: pricePerGbRaw,
      stakeAmountRaw: stakeAmountRaw,
      bandwidthMbps: bandwidthMbps,
    );
  }

  Future<OfferMutationResult> deactivateOffer(
    int offerId, {
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.deactivateOffer(mnemonic: mnemonic, offerId: offerId);
  }

  Future<OfferMutationResult> withdrawStake(
    int offerId, {
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.withdrawStake(mnemonic: mnemonic, offerId: offerId);
  }

  Future<AgentRegistrationResult?> loadAgentRegistration({
    String? profileId,
  }) async {
    final profile = await _resolveProfile(profileId);
    if (profile == null) {
      return null;
    }
    final prefs = await _prefs();
    final agentId = prefs.getInt(_agentIdKey(profile.id));
    if (agentId == null) {
      return null;
    }
    return AgentRegistrationResult(
      agentId: agentId,
      txHash: prefs.getString(_agentTxHashKey(profile.id)) ?? '',
    );
  }

  Future<TxHashResult> submitFeedback({
    required int agentId,
    required bool positive,
    required String tag1,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.submitFeedback(
      mnemonic: mnemonic,
      agentId: agentId,
      positive: positive,
      tag1: tag1,
    );
  }

  Future<List<DealOffer>> fetchDealOffers({required String currency}) =>
      _backend.listDealOffers(currency: currency.trim().toUpperCase());

  Future<List<DealOffer>> fetchMyDealOffers({String? profileId}) async {
    final profile = await _requireProfile(profileId);
    return _backend.listMyDealOffers(address: profile.address);
  }

  Future<DealOffer> fetchDealOffer({required int offerId}) =>
      _backend.getDealOffer(offerId: offerId);

  Future<OfferMutationResult> createDealOffer({
    required int agentId,
    required String currency,
    required String rateRaw,
    required String minAmountRaw,
    required String maxAmountRaw,
    required List<String> paymentMethods,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.createDealOffer(
      mnemonic: mnemonic,
      agentId: agentId,
      currency: currency,
      rateRaw: rateRaw,
      minAmountRaw: minAmountRaw,
      maxAmountRaw: maxAmountRaw,
      paymentMethods: paymentMethods,
    );
  }

  Future<OfferMutationResult> deactivateDealOffer(
    int offerId, {
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.deactivateDealOffer(mnemonic: mnemonic, offerId: offerId);
  }

  Future<AcceptDealResult> acceptDeal({
    required int offerId,
    required String usdcAmount,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.acceptDeal(
      mnemonic: mnemonic,
      offerId: offerId,
      usdcAmount: usdcAmount,
    );
  }

  Future<TxHashResult> markFiatSent({
    required int escrowId,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.markFiatSent(mnemonic: mnemonic, escrowId: escrowId);
  }

  Future<TxHashResult> confirmDealReceipt({
    required int escrowId,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.confirmDealReceipt(mnemonic: mnemonic, escrowId: escrowId);
  }

  Future<TxHashResult> rejectDeal({
    required int escrowId,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.rejectDeal(mnemonic: mnemonic, escrowId: escrowId);
  }

  Future<DealEscrowView> checkEscrowStatus({required int escrowId}) =>
      _backend.checkEscrowStatus(escrowId: escrowId);

  Future<List<DealEscrowView>> loadMyEscrows({
    String? role,
    String? profileId,
  }) async {
    final profile = await _requireProfile(profileId);
    return _backend.listMyDealEscrows(address: profile.address, role: role);
  }

  Future<TxHashResult> claimExpiredEscrow({
    required int escrowId,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.claimExpiredEscrow(mnemonic: mnemonic, escrowId: escrowId);
  }

  Future<DealBoardAllowance> loadDealBoardAllowance({String? profileId}) async {
    final profile = await _requireProfile(profileId);
    return _backend.getDealBoardAllowance(address: profile.address);
  }

  Future<TxHashResult> approveDealBoardUsdc({
    required String amount,
    String? profileId,
  }) async {
    final mnemonic = await _requireMnemonic(profileId: profileId);
    return _backend.approveDealBoardUsdc(mnemonic: mnemonic, amount: amount);
  }

  Future<DealerProfile> loadDealerProfile({String? profileId, String? address}) async {
    final resolvedAddress =
        address ?? (await _resolveProfile(profileId))?.address;
    if (resolvedAddress == null || resolvedAddress.isEmpty) {
      throw StateError('Wallet not configured.');
    }
    final uri = Uri.parse(
      '$_workerBaseUrl/api/dealer-profiles/${resolvedAddress.toLowerCase()}',
    );
    final response = await _httpClient.get(uri);
    if (response.statusCode == 404) {
      return DealerProfile.empty(resolvedAddress);
    }
    if (response.statusCode != 200) {
      throw StateError(
        'Failed to load dealer payment profile. Check relay worker availability and try again.',
      );
    }
    return DealerProfile.fromJson(
      jsonDecode(response.body) as Map<String, dynamic>,
    );
  }

  Future<DealerProfile> saveDealerProfile(
    DealerProfile profile, {
    String? profileId,
  }) async {
    final resolvedProfile = await _requireProfile(profileId);
    final mnemonic = await _requireMnemonic(profileId: resolvedProfile.id);
    final payload = {
      'address': resolvedProfile.address.toLowerCase(),
      'display_name': profile.displayName.trim(),
      'contact_handle': profile.contactHandle.trim(),
      'instructions_by_method': profile.instructionsByMethod.map(
        (key, value) => MapEntry(key.trim().toLowerCase(), value.trim()),
      ),
      'general_notes': profile.generalNotes.trim(),
    };
    final bodyJson = jsonEncode(payload);
    final timestampMs = DateTime.now().millisecondsSinceEpoch;
    final auth = await _backend.signDealerProfilePut(
      mnemonic: mnemonic,
      address: resolvedProfile.address,
      timestampMs: timestampMs,
      bodyJson: bodyJson,
    );
    final uri = Uri.parse('$_workerBaseUrl${auth['path']}');
    final response = await _httpClient.put(
      uri,
      headers: {
        'Content-Type': 'application/json',
        'X-Hydra-Address': auth['address'].toString(),
        'X-Hydra-Timestamp': auth['timestamp_ms'].toString(),
        'X-Hydra-Signature': auth['signature'].toString(),
      },
      body: bodyJson,
    );
    if (response.statusCode != 200) {
      throw StateError(
        'Failed to save dealer payment profile. Check signature, worker deploy, or connectivity.',
      );
    }
    return DealerProfile.fromJson(
      jsonDecode(response.body) as Map<String, dynamic>,
    );
  }

  Future<ShareEarnStatus> loadShareEarnStatus({String? profileId}) async {
    final profile = await _resolveProfile(profileId);
    return _backend.getShareEarnStatus(profileId: profile?.id ?? profileId);
  }

  Future<ShareEarnStatus> setShareEarnEnabled(
    bool enabled, {
    String? profileId,
  }) async {
    final profile = await _resolveProfile(profileId);
    final mnemonic = enabled
        ? await _mnemonicStore.readMnemonic(profileId: profile?.id)
        : null;
    return _backend.setShareEarnEnabled(
      enabled: enabled,
      profileId: profile?.id ?? profileId,
      mnemonic: mnemonic,
    );
  }

  Future<ProviderEarnings> loadProviderEarnings({String? profileId}) async {
    final profile = await _resolveProfile(profileId);
    return _backend.getProviderEarnings(profileId: profile?.id ?? profileId);
  }

  Future<ShareEarnStatus> updateShareSettings({
    String? profileId,
    String? priceOverrideRaw,
    int? maxBandwidthMbps,
    required bool wifiOnly,
    int? scheduleStartHour,
    int? scheduleEndHour,
  }) async {
    final profile = await _resolveProfile(profileId);
    return _backend.updateShareSettings(
      profileId: profile?.id ?? profileId,
      priceOverrideRaw: priceOverrideRaw,
      maxBandwidthMbps: maxBandwidthMbps,
      wifiOnly: wifiOnly,
      scheduleStartHour: scheduleStartHour,
      scheduleEndHour: scheduleEndHour,
    );
  }

  Future<ShareEarnStatus> syncProviderReputation({String? profileId}) async {
    final profile = await _requireProfile(profileId);
    final mnemonic = await _requireMnemonic(profileId: profile.id);
    return _backend.syncProviderReputation(
      profileId: profile.id,
      mnemonic: mnemonic,
    );
  }

  Future<void> clearWallet({String? profileId}) async {
    final profile = await _resolveProfile(profileId);
    if (profile == null) {
      return;
    }
    await deleteProfile(profile.id);
  }

  Future<void> saveFilters({
    required String region,
    required String protocol,
    String? profileId,
  }) async {
    final resolvedProfile = await _resolveProfile(profileId);
    final suffix = resolvedProfile?.id ?? _defaultProfileId;
    final prefs = await _prefs();
    await prefs.setString(_regionKey(suffix), region.trim().toUpperCase());
    await prefs.setString(_protocolKey(suffix), protocol.trim().toLowerCase());
  }

  Future<Map<String, String>> loadFilters({
    required String defaultRegion,
    String defaultProtocol = 'vless',
    String? profileId,
  }) async {
    final resolvedProfile = await _resolveProfile(profileId);
    final suffix = resolvedProfile?.id ?? _defaultProfileId;
    final prefs = await _prefs();
    return {
      'region':
          prefs.getString(_regionKey(suffix)) ?? defaultRegion.trim().toUpperCase(),
      'protocol':
          prefs.getString(_protocolKey(suffix)) ??
          defaultProtocol.trim().toLowerCase(),
    };
  }

  static String normalizeMnemonic(String mnemonic) {
    return mnemonic
        .split(RegExp(r'\s+'))
        .where((part) => part.isNotEmpty)
        .map((part) => part.toLowerCase())
        .join(' ');
  }

  Future<SharedPreferences> _prefs() async {
    final prefs = await _sharedPreferencesLoader();
    await _ensureProfileStoreInitialized(prefs);
    return prefs;
  }

  Future<void> _ensureProfileStoreInitialized([SharedPreferences? prefs]) async {
    if (_profilesInitialized) {
      return;
    }
    final resolvedPrefs = prefs ?? await _sharedPreferencesLoader();
    if (resolvedPrefs.containsKey(_profilesKey)) {
      _profilesInitialized = true;
      return;
    }

    final legacyMnemonic = await _mnemonicStore.readMnemonic();
    if (legacyMnemonic != null && legacyMnemonic.isNotEmpty) {
      final identity = await _backend.getWalletPreview(legacyMnemonic);
      final profile = WalletProfile(
        id: _defaultProfileId,
        name: 'Primary Wallet',
        address: identity.address,
        roleHint: 'general',
        createdAt: DateTime.now().millisecondsSinceEpoch,
        source: 'migrated',
        isBackedUp: true,
      );
      await _mnemonicStore.writeMnemonic(legacyMnemonic, profileId: profile.id);
      await _mnemonicStore.deleteMnemonic();
      await resolvedPrefs.setString(
        _profilesKey,
        jsonEncode([profile.toJson()]),
      );
      await resolvedPrefs.setString(_activeProfileIdKey, profile.id);

      final legacyAgentId = resolvedPrefs.getInt(_legacyAgentIdKey);
      final legacyAgentTxHash = resolvedPrefs.getString(_legacyAgentTxHashKey);
      if (legacyAgentId != null) {
        await resolvedPrefs.setInt(_agentIdKey(profile.id), legacyAgentId);
      }
      if (legacyAgentTxHash != null && legacyAgentTxHash.isNotEmpty) {
        await resolvedPrefs.setString(_agentTxHashKey(profile.id), legacyAgentTxHash);
      }
      await resolvedPrefs.remove(_legacyAgentIdKey);
      await resolvedPrefs.remove(_legacyAgentTxHashKey);
    } else {
      await resolvedPrefs.setString(_profilesKey, jsonEncode(const []));
    }

    _profilesInitialized = true;
  }

  Future<WalletProfile?> _resolveProfile(String? profileId) async {
    if (profileId == null || profileId.isEmpty) {
      return loadActiveProfile();
    }
    return _requireProfile(profileId);
  }

  Future<WalletProfile> _requireProfile(String? profileId) async {
    final resolved = await _resolveProfile(profileId);
    if (resolved == null) {
      throw StateError('Wallet not configured.');
    }
    return resolved;
  }

  Future<String> _requireMnemonic({String? profileId}) async {
    final profile = await _requireProfile(profileId);
    final mnemonic = await _mnemonicStore.readMnemonic(profileId: profile.id);
    if (mnemonic == null || mnemonic.isEmpty) {
      throw StateError('Wallet not configured.');
    }
    return mnemonic;
  }

  Future<void> _cacheAgentRegistration({
    required String profileId,
    required AgentRegistrationResult result,
  }) async {
    final prefs = await _prefs();
    await prefs.setInt(_agentIdKey(profileId), result.agentId);
    await prefs.setString(_agentTxHashKey(profileId), result.txHash);
  }

  Future<void> _clearAgentRegistration(String profileId) async {
    final prefs = await _prefs();
    await prefs.remove(_agentIdKey(profileId));
    await prefs.remove(_agentTxHashKey(profileId));
  }

  Future<void> _saveProfiles(
    List<WalletProfile> profiles, {
    String? activeProfileId,
  }) async {
    final prefs = await _sharedPreferencesLoader();
    await prefs.setString(
      _profilesKey,
      jsonEncode(profiles.map((profile) => profile.toJson()).toList()),
    );
    if (activeProfileId == null || profiles.isEmpty) {
      await prefs.remove(_activeProfileIdKey);
    } else {
      await prefs.setString(_activeProfileIdKey, activeProfileId);
    }
    _profilesInitialized = true;
  }

  List<WalletProfile> _decodeProfiles(String? raw) {
    if (raw == null || raw.isEmpty) {
      return <WalletProfile>[];
    }
    final parsed = jsonDecode(raw);
    if (parsed is! List<dynamic>) {
      return <WalletProfile>[];
    }
    return parsed
        .whereType<Map<String, dynamic>>()
        .map(WalletProfile.fromJson)
        .toList();
  }

  static String _generateProfileId(List<WalletProfile> profiles) {
    final random = Random.secure();
    String id;
    do {
      id =
          'p${DateTime.now().microsecondsSinceEpoch.toRadixString(36)}${random.nextInt(1 << 20).toRadixString(36)}';
    } while (profiles.any((profile) => profile.id == id));
    return id;
  }

  static String _defaultProfileName(String? proposed, int fallbackIndex) {
    final normalized = proposed?.trim() ?? '';
    if (normalized.isNotEmpty) {
      return normalized;
    }
    return fallbackIndex <= 1 ? 'Wallet' : 'Wallet $fallbackIndex';
  }

  static String _agentIdKey(String profileId) =>
      'marketplace_agent_id:$profileId';
  static String _agentTxHashKey(String profileId) =>
      'marketplace_agent_tx_hash:$profileId';
  static String _regionKey(String profileId) => 'marketplace_region:$profileId';
  static String _protocolKey(String profileId) =>
      'marketplace_protocol:$profileId';
}
