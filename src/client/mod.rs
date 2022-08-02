pub(crate) mod cosm_client;

pub mod error;

// reexport some tendermint types
pub use cosmrs::rpc::endpoint::broadcast::tx_commit::TxResult;
pub use cosmrs::tendermint::abci::Code;
pub use cosmrs::tendermint::abci::Event;
