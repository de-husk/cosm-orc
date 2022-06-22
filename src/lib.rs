//! Cosmwasm smart contract orchestration and gas profiling tool
//!
//! Store, instantiate, execute, and query [Cosmwasm] smart contracts against a configured [Cosmos] based chain.
//! Optionally, profile gas usage of the smart contract operations.
//!
//! Potential uses:
//! * Integration tests
//! * Deployments / Bootstrapping environments
//! * Gas profiling
//!
//! [cosmwasm]: https://github.com/CosmWasm/cosmwasm
//! [Cosmos]: https://github.com/cosmos/cosmos-sdk
//!
//!
//! # Quick Start
//!
//! ```
//! use cosm_orc::{
//!    config::config::Config,
//!    orchestrator::cosm_orc::{CosmOrc, WasmMsg},
//! };
//! use cw20::Cw20Coin;
//! use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//!
//! // config.yaml has the cw20_base code_id already stored
//! // If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
//! let mut cosm_orc = CosmOrc::new(Config::from_yaml("config.yaml")?);
//!
//! let msgs: Vec<WasmMsg<InstantiateMsg, ExecuteMsg, QueryMsg>> = vec![
//!     WasmMsg::InstantiateMsg(InstantiateMsg {
//!         name: "Meme Token".to_string(),
//!         symbol: "MEME".to_string(),
//!         decimals: 6,
//!         initial_balances: vec![],
//!         mint: None,
//!         marketing: None,
//!     }),
//!     WasmMsg::QueryMsg(QueryMsg::TokenInfo {}),
//! ];
//!
//! cosm_orc.process_msgs("cw20_base".to_string(), &msgs)?;
//!
//! ```

pub mod orchestrator;

pub mod profilers;

pub mod config;
