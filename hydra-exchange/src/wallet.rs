use alloy::signers::local::{
    MnemonicBuilder, PrivateKeySigner,
    coins_bip39::{English, Mnemonic},
};
use anyhow::{Context, Result};
use rand::thread_rng;
use zeroize::Zeroizing;

use crate::models::{GeneratedWallet, WalletIdentity};

const DERIVATION_INDEX: u32 = 0;

pub struct LocalWallet;

impl LocalWallet {
    pub fn generate() -> Result<GeneratedWallet> {
        let mut rng = thread_rng();
        let mnemonic = Mnemonic::<English>::new_with_count(&mut rng, 12)
            .context("Failed to generate BIP-39 mnemonic")?;
        let phrase = mnemonic.to_phrase();
        let preview = Self::import(&phrase)?;

        Ok(GeneratedWallet {
            mnemonic: phrase,
            address: preview.address,
        })
    }

    pub fn import(mnemonic: &str) -> Result<WalletIdentity> {
        let normalized = normalize_mnemonic(mnemonic);
        let signer = Self::signer_from_phrase(normalized.as_str())?;

        Ok(WalletIdentity {
            address: format!("{:#x}", signer.address()),
        })
    }

    pub fn signer_from_phrase(mnemonic: &str) -> Result<PrivateKeySigner> {
        let phrase = Zeroizing::new(normalize_mnemonic(mnemonic));
        Mnemonic::<English>::new_from_phrase(phrase.as_str())
            .context("Invalid BIP-39 mnemonic")?;

        MnemonicBuilder::<English>::default()
            .phrase(phrase.as_str())
            .index(DERIVATION_INDEX)
            .context("Failed to derive Ethereum wallet path")?
            .build()
            .context("Failed to build local wallet signer from mnemonic")
    }
}

pub fn normalize_mnemonic(mnemonic: &str) -> String {
    mnemonic
        .split_whitespace()
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_wallet_uses_12_word_phrase() {
        let wallet = LocalWallet::generate().unwrap();
        assert_eq!(wallet.mnemonic.split_whitespace().count(), 12);
        assert!(wallet.address.starts_with("0x"));
    }

    #[test]
    fn import_normalizes_whitespace_and_case() {
        let phrase = " legal winner thank year wave sausage worth useful legal winner thank yellow ";
        let imported = LocalWallet::import(phrase).unwrap();
        assert_eq!(
            imported.address,
            "0x58a57ed9d8d624cbd12e2c467d34787555bb1b25"
        );
    }
}
