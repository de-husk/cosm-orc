use cosmrs::bip32;

#[derive(Debug, Clone)]
pub struct SigningKey {
    /// human readable key name
    pub name: String,
    /// private key associated with `name`
    pub key: Key,
    pub derivation_path: String,
}

#[derive(Debug, Clone)]
pub enum Key {
    /// Mnemonic allows you to pass the private key mnemonic words
    /// to Cosm-orc for configuring a transaction signing key.
    /// DO NOT USE FOR MAINNET
    Mnemonic(String),
    // TODO: Support other types of credentials
}

impl From<SigningKey> for cosmrs::crypto::secp256k1::SigningKey {
    // TODO: Stop unwrapping here and make this a TryFrom instead of a From
    fn from(signer: SigningKey) -> cosmrs::crypto::secp256k1::SigningKey {
        match signer.key {
            Key::Mnemonic(key) => {
                let seed = bip32::Mnemonic::new(key, bip32::Language::English)
                    .unwrap()
                    .to_seed("");
                bip32::XPrv::derive_from_path(seed, &signer.derivation_path.parse().unwrap())
                    .unwrap()
                    .into()
            }
        }
    }
}
