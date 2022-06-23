use anyhow::{Context, Result};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::config::cfg::Config;
use crate::orchestrator::command::{exec_msg, CommandType};
use crate::profilers::profiler::{Profiler, Report};

/// Stores cosmwasm contracts and executes their messages against the configured chain.
pub struct CosmOrc {
    cfg: Config,
    pub contract_map: HashMap<ContractName, DeployInfo>,
    profilers: Vec<Box<dyn Profiler>>,
}

pub type ContractName = String;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployInfo {
    pub code_id: u64,
    pub address: Option<String>,
}

pub enum WasmMsg<X: Serialize, Y: Serialize, Z: Serialize> {
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

    /// Used to add profiler to be used during message execution.
    /// Call multiple times to add additional Profilers.
    pub fn add_profiler(mut self, p: Box<dyn Profiler>) -> Self {
        self.profilers.push(p);
        self
    }

    // TODO: I probably shouldnt be returning `serde::Value`s from this library.
    // I should make it more general purpose and just return a byte slice of the json
    // for general consumption through any library, so they dont have to use serde.

    /// Uploads the contracts in `wasm_dir` to the configured chain
    /// saving the resulting contract ids in `contract_map` and
    /// returning the raw cosmos json responses.
    ///
    /// You don't need to call this function if all of the smart contract ids
    /// are already configured via `cfg.code_ids`.
    pub fn store_contracts(&mut self, wasm_dir: &str) -> Result<Vec<Value>> {
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

                self.contract_map.insert(
                    wasm_path
                        .file_stem()
                        .context("wasm_path has invalid filename")?
                        .to_str()
                        .context("wasm_path has invalid unicode chars")?
                        .to_string(),
                    DeployInfo {
                        code_id,
                        address: None,
                    },
                );

                for prof in &mut self.profilers {
                    prof.instrument("Store_TODO".to_string(), CommandType::Store, &json)?;
                }

                responses.push(json);
            }
        }
        Ok(responses)
    }

    /// Executes multiple smart contract operations against the configured chain
    /// returning the raw cosmos json responses.
    pub fn process_msgs<X: Serialize, Y: Serialize, Z: Serialize>(
        &mut self,
        contract_name: String,
        msgs: &[WasmMsg<X, Y, Z>],
    ) -> Result<Vec<Value>> {
        let mut responses = vec![];
        for msg in msgs {
            let json = self.process_msg(contract_name.clone(), msg)?;
            responses.push(json);
        }

        Ok(responses)
    }

    /// Executes a single smart contract operation against the configured chain
    /// returning the raw cosmos json response.
    pub fn process_msg<X: Serialize, Y: Serialize, Z: Serialize>(
        &mut self,
        contract_name: String,
        msg: &WasmMsg<X, Y, Z>,
    ) -> Result<Value> {
        let deploy_info = self
            .contract_map
            .get_mut(&contract_name)
            .context("contract not stored")?;

        let json = match msg {
            WasmMsg::InstantiateMsg(m) => {
                let json = serde_json::to_string(&m)?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Instantiate,
                    &[
                        vec![
                            deploy_info.code_id.to_string(),
                            json,
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
                        "Instantiate_TODO".to_string(),
                        CommandType::Instantiate,
                        &json,
                    )?;
                }

                let addr = json["logs"][0]["events"][0]["attributes"][0]["value"]
                    .as_str()
                    .context("not string")?
                    .to_string();

                (*deploy_info).address = Some(addr);
                json
            }
            WasmMsg::ExecuteMsg(m) => {
                let json = serde_json::to_value(&m)?;
                let addr = deploy_info
                    .address
                    .clone()
                    .context("contract not instantiated")?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Execute,
                    &[vec![addr, json.to_string()], self.cfg.tx_flags.clone()].concat(),
                )?;

                for prof in &mut self.profilers {
                    prof.instrument("Execute_TODO".to_string(), CommandType::Execute, &json)?;
                }

                json
            }
            WasmMsg::QueryMsg(m) => {
                let json = serde_json::to_string(&m)?;
                let addr = deploy_info
                    .address
                    .clone()
                    .context("contract not instantiated")?;

                let json = exec_msg(
                    &self.cfg.chain_cfg.binary,
                    CommandType::Query,
                    &[addr, json, "--output".to_string(), "json".to_string()],
                )?;

                for prof in &mut self.profilers {
                    prof.instrument("Query_TODO".to_string(), CommandType::Query, &json)?;
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
