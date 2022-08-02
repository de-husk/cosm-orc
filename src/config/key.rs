use cosmrs::bip32;

use crate::client::error::ClientError;

// TODO: It would be cool if cosm-orc could create test accounts for you

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

impl TryFrom<SigningKey> for cosmrs::crypto::secp256k1::SigningKey {
    type Error = ClientError;
    fn try_from(signer: SigningKey) -> Result<cosmrs::crypto::secp256k1::SigningKey, ClientError> {
        match signer.key {
            Key::Mnemonic(key) => {
                let seed = bip32::Mnemonic::new(key, bip32::Language::English)
                    .map_err(|_| ClientError::Mnemonic)?
                    .to_seed("");
                Ok(bip32::XPrv::derive_from_path(
                    seed,
                    &signer
                        .derivation_path
                        .parse()
                        .map_err(|_| ClientError::DerviationPath)?,
                )
                .map_err(|_| ClientError::DerviationPath)?
                .into())
            }
        }
    }
}
