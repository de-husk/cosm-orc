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
//! ```no_run
//! # use std::error::Error;
//! # use cosm_orc::{
//! #    config::cfg::Config,
//! #    orchestrator::cosm_orc::{CosmOrc, WasmMsg},
//! # };
//! # use cosm_orc::config::key::SigningKey;
//! # use cosm_orc::config::key::Key;
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # fn main() -> Result<(), Box<dyn Error>> {
//!     // juno_local.yaml has the cw20_base code_id already stored
//!     // If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
//!     let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?)?;
//!
//!     let key = SigningKey {
//!          name: "validator".to_string(),
//!          key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      };
//!
//!     let msgs: Vec<WasmMsg<InstantiateMsg, ExecuteMsg, QueryMsg>> = vec![
//!         WasmMsg::InstantiateMsg(InstantiateMsg {
//!             name: "Meme Token".to_string(),
//!             symbol: "MEME".to_string(),
//!             decimals: 6,
//!             initial_balances: vec![],
//!             mint: None,
//!             marketing: None,
//!         }),
//!         WasmMsg::QueryMsg(QueryMsg::TokenInfo {}),
//!     ];
//!
//!      cosm_orc.process_msgs("cw20_base", "meme_token_test", &msgs, &key)?;
//! #    Ok(())
//! # }
//! ```
//!
//! # Store Contracts
//!
//! If `config.yaml` doesn't have the pre-stored contract code ids, you can call `store_contracts()`:
//!
//! ```no_run
//! # use std::error::Error;
//! # use cosm_orc::{
//! #    config::cfg::Config,
//! #    orchestrator::cosm_orc::{CosmOrc, WasmMsg},
//! # };
//! # use cosm_orc::config::key::SigningKey;
//! # use cosm_orc::config::key::Key;
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # fn main() -> Result<(), Box<dyn Error>> {
//!     let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?)?;
//!
//!     let key = SigningKey {
//!          name: "validator".to_string(),
//!          key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      };
//!
//!     // `./artifacts` is a directory that contains the rust optimized wasm files.
//!     //
//!     // NOTE: currently cosm-orc is expecting a wasm filed called: `cw20_base.wasm`
//!     // to be in `/artifacts`, since `cw20_base` is used as the contract name in process_msgs() call below
//!     cosm_orc.store_contracts("./artifacts", &key)?;
//!
//!     let msgs: Vec<WasmMsg<InstantiateMsg, ExecuteMsg, QueryMsg>> = vec![
//!         WasmMsg::InstantiateMsg(InstantiateMsg {
//!             name: "Meme Token".to_string(),
//!             symbol: "MEME".to_string(),
//!             decimals: 6,
//!             initial_balances: vec![],
//!             mint: None,
//!             marketing: None,
//!         }),
//!         WasmMsg::QueryMsg(QueryMsg::TokenInfo {}),
//!     ];
//!
//!      cosm_orc.process_msgs("cw20_base", "meme_token_test", &msgs, &key)?;
//! #    Ok(())
//! # }
//! ```
//!
//! # Gas Profiling
//!
//! ```no_run
//! # use std::error::Error;
//! # use cosm_orc::{
//! #    config::cfg::Config,
//! #    orchestrator::cosm_orc::{CosmOrc, WasmMsg},
//! #    profilers::gas_profiler::GasProfiler,
//! # };
//! # use cosm_orc::config::key::SigningKey;
//! # use cosm_orc::config::key::Key;
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # fn main() -> Result<(), Box<dyn Error>> {
//!    let mut cosm_orc =
//!        CosmOrc::new(Config::from_yaml("config.yaml")?)?.add_profiler(Box::new(GasProfiler::new()));
//!
//!     let key = SigningKey {
//!          name: "validator".to_string(),
//!          key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      };
//!
//!     // `./artifacts` is a directory that contains the rust optimized wasm files.
//!     //
//!     // NOTE: currently cosm-orc is expecting a wasm filed called: `cw20_base.wasm`
//!     // to be in `/artifacts`, since `cw20_base` is used as the contract name in process_msgs() call below
//!     cosm_orc.store_contracts("./artifacts", &key)?;
//!
//!     let msgs: Vec<WasmMsg<InstantiateMsg, ExecuteMsg, QueryMsg>> = vec![
//!         WasmMsg::InstantiateMsg(InstantiateMsg {
//!             name: "Meme Token".to_string(),
//!             symbol: "MEME".to_string(),
//!             decimals: 6,
//!             initial_balances: vec![],
//!             mint: None,
//!             marketing: None,
//!         }),
//!         WasmMsg::QueryMsg(QueryMsg::TokenInfo {}),
//!     ];
//!
//!      cosm_orc.process_msgs("cw20_base", "meme_token_test", &msgs, &key)?;
//!      let reports = cosm_orc.profiler_reports()?;
//! #    Ok(())
//! # }
//! ```
//!

pub mod orchestrator;

pub mod profilers;

pub mod config;

pub mod client;
