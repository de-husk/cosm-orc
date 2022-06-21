use anyhow::{Context, Result};
use log::{debug, info};
use serde::Serialize;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::profiler::command::{exec_msg, CommandType};
use crate::profiler::config::Config;

// TODO:
// * Add CI steps to build, test, and push to crates.io

pub struct GasProfiler {
  cfg: Config,
  pub contract_map: HashMap<ContractName, DeployInfo>,
  pub report: HashMap<ContractName, GasReport>,
}

pub type ContractName = String;

#[derive(Debug)]
pub struct DeployInfo {
  pub code_id: u64,
  pub address: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GasReport {
  pub gas_wanted: u64,
  pub gas_used: u64,
}

pub enum WasmMsg<X: Serialize, Y: Serialize, Z: Serialize> {
  InstantiateMsg(X),
  ExecuteMsg(Y),
  QueryMsg(Z),
}

impl GasProfiler {
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
      report: HashMap::new(),
    }
  }

  // uploads the contracts in `cfg.wasm_dir` to the configured chain
  // saving the resulting contract ids in `contract_map`
  //
  // you don't need to call this function if all of the smart contract ids
  // are already passed in via `cfg.code_ids`
  pub fn store_contracts(&mut self) -> Result<()> {
    let wasm_path = Path::new(&self.cfg.wasm_dir);

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
      }
    }

    Ok(())
  }

  // executes each msg against the configured chain
  // storing the gas usage in `report`
  pub fn run_benchmark<X: Serialize, Y: Serialize, Z: Serialize>(
    &mut self,
    contract_name: String,
    msgs: &[WasmMsg<X, Y, Z>],
  ) -> Result<()> {
    let deploy_info = self
      .contract_map
      .get_mut(&contract_name)
      .context("contract not stored")?;

    for msg in msgs {
      match msg {
        WasmMsg::InstantiateMsg(m) => {
          let json = serde_json::to_string(&m)?;

          let json = exec_msg(
            &self.cfg.chain_cfg.binary,
            CommandType::Instantiate,
            &[
              vec![
                deploy_info.code_id.to_string(),
                json.to_string(),
                "--label".to_string(),
                "gas profiler".to_string(),
                "--no-admin".to_string(), // TODO: Allow for configurable admin addr to be passed
              ],
              self.cfg.tx_flags.clone(),
            ]
            .concat(),
          )?;

          self.report.insert(
            "Instantiate_TODO".to_string(), // TODO
            GasReport {
              gas_used: json["gas_used"].as_str().context("not string")?.parse()?,
              gas_wanted: json["gas_wanted"].as_str().context("not string")?.parse()?,
            },
          );

          let addr = json["logs"][0]["events"][0]["attributes"][0]["value"]
            .as_str()
            .context("not string")?
            .to_string();

          (*deploy_info).address = Some(addr);
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

          self.report.insert(
            "Execute_TODO".to_string(), // TODO
            GasReport {
              gas_used: json["gas_used"].as_str().context("not string")?.parse()?,
              gas_wanted: json["gas_wanted"].as_str().context("not string")?.parse()?,
            },
          );
        }
        WasmMsg::QueryMsg(m) => {
          // NOTE: QueryMsg's don't cost gas, but we support it for debugging purposes
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

          debug!("{}", json);
        }
      }
    }

    debug!("{:?}", self.contract_map);
    debug!("{:?}", self.report);

    Ok(())
  }

  pub fn write_report(&self, file_path: &str) -> Result<()> {
    let json = serde_json::to_string(&self.report)?;
    fs::write(file_path, json).context("Unable to write file")?;
    Ok(())
  }
}
