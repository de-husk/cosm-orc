pub mod cosm_orc;

pub mod deploy;

pub mod error;

pub mod gas_profiler;

/// Batch wasm execute request
pub struct ExecReq {
    /// Deployed smart contract name for the corresponding `msg`
    pub contract_name: String,
    /// ExecuteMsg that `contract_name` supports
    pub msg: Box<dyn erased_serde::Serialize>,
    /// Optional tokens transferred to the contract after execution
    pub funds: Vec<Coin>,
}

pub use cosm_tome::chain::coin::{Coin, Denom};
pub use cosm_tome::chain::fee::{Fee, Gas};
pub use cosm_tome::chain::response::{ChainResponse, ChainTxResponse, Code};
pub use cosm_tome::clients::cosmos_grpc::CosmosgRPC;
pub use cosm_tome::clients::tendermint_rpc::TendermintRPC;
pub use cosm_tome::modules::auth::model::Address;
pub use cosm_tome::modules::cosmwasm::model::{AccessConfig, AccessType};
pub use cosm_tome::modules::cosmwasm::model::{
    ExecResponse, InstantiateResponse, MigrateResponse, QueryResponse, StoreCodeResponse,
};
pub use cosm_tome::signing_key::key::{Key, SigningKey};
