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
//! This project is not yet intended to be used for mainnet.
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
//! #    orchestrator::cosm_orc::CosmOrc,
//! # };
//! # use cosm_orc::orchestrator::{SigningKey, Key};
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # use cw20::TokenInfoResponse;
//! # fn main() -> Result<(), Box<dyn Error>> {
//!  // juno_local.yaml has the `cw20_base` code_id already stored
//!  // If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
//!  let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?, false)?;
//!  let key = SigningKey {
//!      name: "validator".to_string(),
//!      key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      derivation_path: "m/44'/118'/0'/0/0".to_string(),
//!  };
//!    
//!  cosm_orc.instantiate(
//!      "cw20_base",
//!      "meme_token_test",
//!      &InstantiateMsg {
//!          name: "Meme Token".to_string(),
//!          symbol: "MEME".to_string(),
//!          decimals: 6,
//!          initial_balances: vec![],
//!          mint: None,
//!          marketing: None,
//!      },
//!      &key,
//!      None,
//!      vec![]
//!  )?;
//!      
//!  let res = cosm_orc.query(
//!      "cw20_base",
//!      &QueryMsg::TokenInfo {},
//!  )?;
//!      
//!  let res: TokenInfoResponse = res.data()?;
//! # Ok(())
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
//! #    orchestrator::cosm_orc::CosmOrc,
//! # };
//! # use cosm_orc::orchestrator::{SigningKey, Key};
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # fn main() -> Result<(), Box<dyn Error>> {
//!  let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?, false)?;
//!
//!  let key = SigningKey {
//!      name: "validator".to_string(),
//!      key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      derivation_path: "m/44'/118'/0'/0/0".to_string(),
//!   };
//!
//!  // `./artifacts` is a directory that contains the rust optimized wasm files.
//!  //
//!  // NOTE: currently cosm-orc is expecting a wasm filed called: `cw20_base.wasm`
//!  // to be in `/artifacts`, since `cw20_base` is used as the contract name in process_msgs() call below
//!  cosm_orc.store_contracts("./artifacts", &key, None)?;
//!
//!  cosm_orc.instantiate(
//!      "cw20_base",
//!      "meme_token_test",
//!      &InstantiateMsg {
//!          name: "Meme Token".to_string(),
//!          symbol: "MEME".to_string(),
//!          decimals: 6,
//!          initial_balances: vec![],
//!          mint: None,
//!          marketing: None,
//!      },
//!      &key,
//!      None,
//!      vec![]
//!  )?;
//!      
//!  let res = cosm_orc.query(
//!      "cw20_base",
//!      &QueryMsg::TokenInfo {},
//!  )?;
//!
//! #  Ok(())
//! # }
//! ```
//!
//! # Gas Profiling
//!
//! ```no_run
//! # use std::error::Error;
//! # use cosm_orc::{
//! #    config::cfg::Config,
//! #    orchestrator::cosm_orc::CosmOrc,
//! # };
//! # use cosm_orc::orchestrator::{SigningKey, Key};
//! # use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
//! # fn main() -> Result<(), Box<dyn Error>> {
//!  let mut cosm_orc = CosmOrc::new(Config::from_yaml("config.yaml")?, true)?;
//!
//!  let key = SigningKey {
//!      name: "validator".to_string(),
//!      key: Key::Mnemonic("word1 word2 ...".to_string()),
//!      derivation_path: "m/44'/118'/0'/0/0".to_string(),
//!  };
//!
//!  cosm_orc.instantiate(
//!      "cw20_base",
//!      "meme_token_test",
//!      &InstantiateMsg {
//!          name: "Meme Token".to_string(),
//!          symbol: "MEME".to_string(),
//!          decimals: 6,
//!          initial_balances: vec![],
//!          mint: None,
//!          marketing: None,
//!      },
//!      &key,
//!      None,
//!      vec![]
//!  )?;
//!
//!  let reports = cosm_orc.gas_profiler_report();
//!
//! #  Ok(())
//! # }
//! ```
//!

pub mod orchestrator;

pub mod config;
