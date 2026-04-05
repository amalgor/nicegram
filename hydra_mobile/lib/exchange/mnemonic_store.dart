import 'package:flutter_secure_storage/flutter_secure_storage.dart';

abstract class MnemonicStore {
  Future<String?> readMnemonic({String? profileId});
  Future<void> writeMnemonic(String mnemonic, {String? profileId});
  Future<void> deleteMnemonic({String? profileId});
}

class SecureStorageMnemonicStore implements MnemonicStore {
  SecureStorageMnemonicStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  static const legacyKey = 'hydra_marketplace_mnemonic';

  final FlutterSecureStorage _storage;

  static String keyForProfile(String profileId) =>
      'hydra_wallet_profile_${profileId}_mnemonic';

  @override
  Future<String?> readMnemonic({String? profileId}) =>
      _storage.read(key: profileId == null ? legacyKey : keyForProfile(profileId));

  @override
  Future<void> writeMnemonic(String mnemonic, {String? profileId}) => _storage.write(
    key: profileId == null ? legacyKey : keyForProfile(profileId),
    value: mnemonic,
  );

  @override
  Future<void> deleteMnemonic({String? profileId}) => _storage.delete(
    key: profileId == null ? legacyKey : keyForProfile(profileId),
  );
}
