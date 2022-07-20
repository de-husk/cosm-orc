use anyhow::{Context, Result};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::panic::Location;
use std::path::Path;

use crate::config::cfg::Config;
use crate::orchestrator::command::{exec_msg, CommandType};
use crate::profilers::profiler::{Profiler, Report};
use crate::util::key_str::type_name;

/// Stores cosmwasm contracts and executes their messages against the configured chain.
pub struct CosmOrc {
    cfg: Config,
    pub contract_map: HashMap<ContractName, DeployInfo>,
    profilers: Vec<Box<dyn Profiler + Send>>,
}

pub type ContractName = String;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployInfo {
    pub code_id: u64,
    pub address: Option<String>,
}

pub enum WasmMsg<X, Y, Z>
where
    X: Serialize,
    Y: Serialize,
    Z: Serialize,
{
    InstantiateMsg(X),
    ExecuteMsg(Y),
    QueryMsg(Z),
}

impl CosmOrc {
    /// Creates a CosmOrc object from the supplied Config
    pub fn new(cfg: Config) -> Self {
        let mut contract_map = HashMap::new();

        for (name, code_id) in &cfg.code_ids {
            contract_map.insert(
                name.clone(),
                DeployInfo {
                    code_id: *code_id,
                    address: None,
                },
            );
        }

        Self {
            cfg,
            contract_map,
            profilers: vec![],
        }
    }

    /// Used to add a profiler to be used during message execution.
    /// Call multiple times to add additional Profilers.
    pub fn add_profiler(mut self, p: Box<dyn Profiler + Send>) -> Self {
        self.profilers.push(p);
        self
    }

    // TODO: I probably shouldnt be returning `serde::Value`s from this library.
    // I should make it more general purpose and just return a byte slice of the json
    // for general consumption through any library, so they dont have to use serde.

    // TODO: Implement a `store_contract()` that takes in a single wasm file as well

    /// Uploads the contracts in `wasm_dir` to the configured chain
    /// saving the resulting contract ids in `contract_map` and
    /// returning the raw cosmos json responses.
    ///
    /// You don't need to call this function if all of the smart contract ids
    /// are already configured via `cfg.code_ids`.
    #[track_caller]
    pub fn store_contracts(&mut self, wasm_dir: &str) -> Result<Vec<Value>> {
        let caller_loc = Location::caller();
        let mut responses = vec![];
        let wasm_path = Path::new(wasm_dir);

        for wasm in fs::read_dir(wasm_path)? {
            let wasm_path = wasm?.path();
            if wasm_path.extension() == Some(OsStr::new("wasm")) {
                info!("Storing {:?}", wasm_path);

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Store,
                    &[
                        vec![wasm_path
                            .to_str()
                            .context("invalid unicode chars")?
                            .to_string()],
                        self.cfg.tx_flags.clone(),
                    ]
                    .concat(),
                )?;

                let code_id: u64 = json["logs"][0]["events"][1]["attributes"][0]["value"]
                    .as_str()
                    .context("value is not a string")?
                    .parse()?;

                let contract = wasm_path
                    .file_stem()
                    .context("wasm_path has invalid filename")?
                    .to_str()
                    .context("wasm_path has invalid unicode chars")?
                    .to_string();

                self.contract_map.insert(
                    contract.clone(),
                    DeployInfo {
                        code_id,
                        address: None,
                    },
                );

                for prof in &mut self.profilers {
                    prof.instrument(
                        contract.clone(),
                        "Store".to_string(),
                        CommandType::Store,
                        &json,
                        caller_loc,
                        0,
                    )?;
                }

                responses.push(json);
            }
        }
        Ok(responses)
    }

    /// Executes multiple smart contract operations against the configured chain
    /// returning the raw cosmos json responses.
    #[track_caller]
    pub fn process_msgs<X, Y, Z>(
        &mut self,
        contract_name: String,
        msgs: &[WasmMsg<X, Y, Z>],
    ) -> Result<Vec<Value>>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
    {
        let caller_loc = Location::caller();
        let mut responses = vec![];
        for (idx, msg) in msgs.iter().enumerate() {
            let json = self.process_msg_internal(contract_name.clone(), msg, idx, caller_loc)?;
            responses.push(json);
        }

        Ok(responses)
    }

    /// Executes a single smart contract operation against the configured chain
    /// returning the raw cosmos json response.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] if `contract_name` does not have a `DeployInfo` entry in `self.contract_map`.
    /// `contract_name` needs to be configured in `Config.code_ids`
    /// or `CosmOrc::store_contracts()` needs to be called with the `contract_name.wasm` in the passed directory.
    #[track_caller]
    pub fn process_msg<X, Y, Z>(
        &mut self,
        contract_name: String,
        msg: &WasmMsg<X, Y, Z>,
    ) -> Result<Value>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
    {
        let caller_loc = Location::caller();
        self.process_msg_internal(contract_name, msg, 0, caller_loc)
    }

    // process_msg_internal is a private method with an index
    // of the passed in message for profiler bookeeping
    fn process_msg_internal<X, Y, Z>(
        &mut self,
        contract_name: String,
        msg: &WasmMsg<X, Y, Z>,
        idx: usize,
        caller_loc: &Location,
    ) -> Result<Value>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
    {
        let deploy_info = self
            .contract_map
            .get_mut(&contract_name)
            .context("contract not stored")?;

        let json = match msg {
            WasmMsg::InstantiateMsg(m) => {
                let input_json = serde_json::to_value(&m)?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Instantiate,
                    &[
                        vec![
                            deploy_info.code_id.to_string(),
                            input_json.to_string(),
                            "--label".to_string(),
                            "gas profiler".to_string(),
                            "--no-admin".to_string(), // TODO: Allow for configurable admin addr to be passed
                        ],
                        self.cfg.tx_flags.clone(),
                    ]
                    .concat(),
                )?;

                for prof in &mut self.profilers {
                    prof.instrument(
                        contract_name.clone(),
                        type_name(m),
                        CommandType::Instantiate,
                        &json,
                        caller_loc,
                        idx,
                    )?;
                }

                let addr = json["logs"][0]["events"][0]["attributes"][0]["value"]
                    .as_str()
                    .context("not string")?
                    .to_string();

                deploy_info.address = Some(addr);
                json
            }
            WasmMsg::ExecuteMsg(m) => {
                let input_json = serde_json::to_value(&m)?;
                let addr = deploy_info
                    .address
                    .clone()
                    .context("contract not instantiated")?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Execute,
                    &[
                        vec![addr, input_json.to_string()],
                        self.cfg.tx_flags.clone(),
                    ]
                    .concat(),
                )?;

                for prof in &mut self.profilers {
                    prof.instrument(
                        contract_name.clone(),
                        type_name(m),
                        CommandType::Execute,
                        &json,
                        caller_loc,
                        idx,
                    )?;
                }

                json
            }
            WasmMsg::QueryMsg(m) => {
                let input_json = serde_json::to_value(&m)?;
                let addr = deploy_info
                    .address
                    .clone()
                    .context("contract not instantiated")?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Query,
                    &[
                        addr,
                        input_json.to_string(),
                        "--node".to_string(),
                        self.cfg.chain_cfg.rpc_endpoint.clone(),
                        "--output".to_string(),
                        "json".to_string(),
                    ],
                )?;

                for prof in &mut self.profilers {
                    prof.instrument(
                        contract_name.clone(),
                        type_name(m),
                        CommandType::Query,
                        &json,
                        caller_loc,
                        idx,
                    )?;
                }

                json
            }
        };

        debug!("{}", json);
        Ok(json)
    }

    /// Get instrumentation reports for each configured profiler.
    pub fn profiler_reports(&self) -> Result<Vec<Report>> {
        let mut reports = vec![];
        for prof in &self.profilers {
            reports.push(prof.report()?);
        }

        Ok(reports)
    }
}
