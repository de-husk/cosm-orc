use thiserror::Error;

use crate::client::error::ClientError;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("error reading wasm_dir")]
    WasmDirRead { source: std::io::Error },

    #[error("error reading wasm file")]
    WasmFileRead { source: std::io::Error },

    #[error("wasm contract file name was not valid utf8 or malformed")]
    InvalidWasmFileName,

    #[error("error instrumenting message")]
    Instrument { msg: String }, // TODO: return an actual base error

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

    pub fn instrument(e: Box<dyn std::error::Error>) -> StoreError {
        StoreError::Instrument { msg: e.to_string() }
    }
}

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("serde json serialization error")]
    JsonSerialize { source: serde_json::Error },

    #[error("error instrumenting message")]
    Instrument { msg: String },

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

    pub fn instrument(e: Box<dyn std::error::Error>) -> ProcessError {
        ProcessError::Instrument { msg: e.to_string() }
    }
}

#[derive(Error, Debug)]
pub enum ReportError {
    #[error("error generating report")]
    ReportError { source: Box<dyn std::error::Error> },
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
