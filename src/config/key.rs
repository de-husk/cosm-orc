use crate::client::error::ClientError;
use cosmrs::crypto::secp256k1;
use cosmrs::{bip32, AccountId};

// TODO: It would be cool if cosm-orc could create test accounts for you

// https://github.com/confio/cosmos-hd-key-derivation-spec#the-cosmos-hub-path
const DERVIATION_PATH: &str = "m/44'/118'/0'/0/0";

#[derive(Debug, Clone)]
pub struct SigningKey {
    /// human readable key name
    pub name: String,
    /// private key associated with `name`
    pub key: Key,
}

impl SigningKey {
    pub fn to_account(&self, prefix: &str) -> Result<AccountId, ClientError> {
        let key: secp256k1::SigningKey = self.try_into()?;
        let account = key
            .public_key()
            .account_id(prefix)
            .map_err(ClientError::crypto)?;
        Ok(account)
    }
}

#[derive(Debug, Clone)]
pub enum Key {
    /// Mnemonic allows you to pass the private key mnemonic words
    /// to Cosm-orc for configuring a transaction signing key.
    /// DO NOT USE FOR MAINNET
    Mnemonic(String),
    // TODO: Support other types of credentials
}

impl TryFrom<&SigningKey> for secp256k1::SigningKey {
    type Error = ClientError;
    fn try_from(signer: &SigningKey) -> Result<secp256k1::SigningKey, ClientError> {
        match &signer.key {
            Key::Mnemonic(phrase) => {
                let seed = bip32::Mnemonic::new(phrase, bip32::Language::English)
                    .map_err(|_| ClientError::Mnemonic)?
                    .to_seed("");
                Ok(
                    bip32::XPrv::derive_from_path(seed, &DERVIATION_PATH.parse().unwrap())
                        .map_err(|_| ClientError::DerviationPath)?
                        .into(),
                )
            }
        }
    }
}
