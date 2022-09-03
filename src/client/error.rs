use cosmrs::ErrorReport;
use prost::{DecodeError, EncodeError};
use thiserror::Error;

use super::chain_res::ChainResponse;

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

    #[error("invalid admin address")]
    AdminAddress,

    #[error("invalid instantiate permissions")]
    InstantiatePerms { source: ErrorReport },

    #[error("proto encoding error")]
    ProtoEncoding { source: ErrorReport },

    #[error("proto decoding error")]
    ProtoDecoding { source: ErrorReport },

    #[error("CosmosSDK error: {res:?}")]
    CosmosSdk { res: ChainResponse },

    #[error(transparent)]
    GRPC(#[from] tonic::transport::Error),

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

#[derive(Error, Debug)]
pub enum DeserializeError {
    #[error("Raw tendermint response is empty")]
    EmptyResponse,

    #[error(transparent)]
    Serde(#[from] serde_json::error::Error),
}
