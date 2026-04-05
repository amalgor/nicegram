Uri baseSepoliaAddressUri(String address) =>
    Uri.parse('https://sepolia.basescan.org/address/$address');

Uri baseSepoliaTxUri(String txHash) =>
    Uri.parse('https://sepolia.basescan.org/tx/$txHash');
