use thiserror::Error;
use tokio::time::error::Elapsed;

use crate::client::error::ClientError;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("error reading wasm_dir")]
    WasmDirRead { source: std::io::Error },

    #[error("error reading wasm file")]
    WasmFileRead { source: std::io::Error },

    #[error("wasm contract file name was not valid utf8 or malformed")]
    InvalidWasmFileName,

    #[error(transparent)]
    ClientError(#[from] ClientError),

    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

impl StoreError {
    pub fn wasmdir(e: std::io::Error) -> StoreError {
        StoreError::WasmDirRead { source: e }
    }

    pub fn wasmfile(e: std::io::Error) -> StoreError {
        StoreError::WasmFileRead { source: e }
    }
}

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("serde json serialization error")]
    JsonSerialize { source: serde_json::Error },

    #[error(transparent)]
    ContractMapError(#[from] ContractMapError),

    #[error(transparent)]
    ClientError(#[from] ClientError),

    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

impl ProcessError {
    pub fn json(e: serde_json::Error) -> ProcessError {
        ProcessError::JsonSerialize { source: e }
    }
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ContractMapError {
    #[error("smart contract not stored on chain: {name:?}")]
    NotStored { name: String },

    #[error("smart contract with addr not initialized on chain: {name:?}")]
    NotDeployed { name: String },
}

#[derive(Error, Debug)]
pub enum OptimizeError {
    #[error("error running optimizoor")]
    Optimize { source: Box<dyn std::error::Error> },
}

#[derive(Error, Debug)]
pub enum PollBlockError {
    #[error(transparent)]
    Timeout(#[from] Elapsed),

    #[error(transparent)]
    ClientError(#[from] ClientError),
}
