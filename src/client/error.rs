use cosmrs::{
    tendermint::abci::{Code, Log},
    ErrorReport,
};
use prost::{DecodeError, EncodeError};
use tendermint_rpc::endpoint::{abci_query::AbciQuery, broadcast::tx_commit::TxResult};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("malformed rpc url")]
    InvalidURL { source: tendermint_rpc::Error },

    #[error("invalid account ID: {id:?}")]
    AccountId { id: String },

    #[error("cryptographic error")]
    Crypto { source: ErrorReport },

    #[error("invalid denomination: {name:?}")]
    Denom { name: String },

    #[error("invalid chainId: {chain_id:?}")]
    ChainId { chain_id: String },

    #[error("invalid mnemonic")]
    Mnemonic,

    #[error("invalid derivation path")]
    DerviationPath,

    #[error("proto encoding error")]
    ProtoEncoding { source: ErrorReport },

    #[error("proto decoding error")]
    ProtoDecoding { source: ErrorReport },

    #[error("CosmosSDK error")]
    CosmosSdk { res: TendermintRes },

    #[error(transparent)]
    RPC(#[from] tendermint_rpc::Error),
}

impl ClientError {
    pub fn crypto(e: ErrorReport) -> ClientError {
        ClientError::Crypto { source: e }
    }

    pub fn proto_encoding(e: ErrorReport) -> ClientError {
        ClientError::ProtoEncoding { source: e }
    }

    pub fn prost_proto_en(e: EncodeError) -> ClientError {
        ClientError::ProtoEncoding { source: e.into() }
    }

    pub fn prost_proto_de(e: DecodeError) -> ClientError {
        ClientError::ProtoDecoding { source: e.into() }
    }
}

#[derive(Debug)]
pub struct TendermintRes {
    pub code: Code,
    pub log: Log,
}

impl From<TxResult> for TendermintRes {
    fn from(res: TxResult) -> TendermintRes {
        TendermintRes {
            code: res.code,
            log: res.log,
        }
    }
}

impl From<AbciQuery> for TendermintRes {
    fn from(res: AbciQuery) -> TendermintRes {
        TendermintRes {
            code: res.code,
            log: res.log,
        }
    }
}
