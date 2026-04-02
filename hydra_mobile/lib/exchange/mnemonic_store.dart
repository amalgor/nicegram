import 'package:flutter_secure_storage/flutter_secure_storage.dart';

abstract class MnemonicStore {
  Future<String?> readMnemonic();
  Future<void> writeMnemonic(String mnemonic);
  Future<void> deleteMnemonic();
}

class SecureStorageMnemonicStore implements MnemonicStore {
  SecureStorageMnemonicStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  static const _key = 'hydra_marketplace_mnemonic';

  final FlutterSecureStorage _storage;

  @override
  Future<String?> readMnemonic() => _storage.read(key: _key);

  @override
  Future<void> writeMnemonic(String mnemonic) =>
      _storage.write(key: _key, value: mnemonic);

  @override
  Future<void> deleteMnemonic() => _storage.delete(key: _key);
}

