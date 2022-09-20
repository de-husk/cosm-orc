use config::ConfigError as _ConfigError;
use cosmrs::ErrorReport;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Chain ID not found: {chain_id:?}")]
    ChainID { chain_id: String },

    #[error("Chain Registry API Error")]
    ChainRegistryAPI { source: ErrorReport },

    #[error("Chain Registry config is missing Fee for: {chain_id:?}")]
    MissingFee { chain_id: String },

    #[error("Chain Registry config is missing RPC endpoint for: {chain_id:?}")]
    MissingRPC { chain_id: String },

    #[error("Chain Registry config is missing gRPC endpoint for: {chain_id:?}")]
    MissingGRPC { chain_id: String },

    #[error("Error parsing url")]
    UrlParse(#[from] tendermint_rpc::Error),

    #[error(transparent)]
    Config(#[from] _ConfigError),
}
