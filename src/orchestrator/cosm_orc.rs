use cosmrs::rpc::endpoint::broadcast::tx_commit::TxResult;
use log::{debug, info};
use serde::Serialize;
use std::ffi::OsStr;
use std::fmt::{self, Debug};
use std::fs;
use std::panic::Location;
use std::path::Path;

use super::error::{ProcessError, ReportError, StoreError};
use crate::client::cosm_client::{tokio_block, CosmClient};
use crate::client::error::ClientError;
use crate::config::cfg::Config;
use crate::config::key::SigningKey;
use crate::orchestrator::deploy::ContractMap;
use crate::profilers::profiler::{CommandType, Profiler, Report};

/// Stores cosmwasm contracts and executes their messages against the configured chain.
pub struct CosmOrc {
    pub contract_map: ContractMap,
    client: CosmClient,
    profilers: Vec<Box<dyn Profiler + Send>>,
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

impl Debug for CosmOrc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.contract_map)
    }
}

impl CosmOrc {
    /// Creates a CosmOrc object from the supplied Config
    pub fn new(cfg: Config) -> Result<Self, ClientError> {
        Ok(Self {
            contract_map: ContractMap::new(&cfg.code_ids),
            client: CosmClient::new(cfg.chain_cfg)?,
            profilers: vec![],
        })
    }

    /// Used to add a profiler to be used during message execution.
    /// Call multiple times to add additional Profilers.
    pub fn add_profiler(mut self, p: Box<dyn Profiler + Send>) -> Self {
        self.profilers.push(p);
        self
    }

    // TODO: allow for the ability to optimize the wasm here too

    /// Uploads the contracts in `wasm_dir` to the configured chain
    /// saving the resulting contract ids in `contract_map` and
    /// returning the raw cosmos json responses.
    ///
    /// You don't need to call this function if all of the smart contract ids
    /// are already configured via `cfg.code_ids`.
    #[track_caller]
    pub fn store_contracts(
        &mut self,
        wasm_dir: &str,
        key: &SigningKey,
    ) -> Result<Vec<TxResult>, StoreError> {
        let caller_loc = Location::caller();
        let mut responses = vec![];
        let wasm_path = Path::new(wasm_dir);

        for wasm in fs::read_dir(wasm_path).map_err(StoreError::wasmdir)? {
            let wasm_path = wasm?.path();
            if wasm_path.extension() == Some(OsStr::new("wasm")) {
                info!("Storing {:?}", wasm_path);

                let wasm = fs::read(&wasm_path).map_err(StoreError::wasmfile)?;

                let res =
                    tokio_block(async { self.client.store(wasm, &key.clone().try_into()?).await })?;

                let contract = wasm_path
                    .file_stem()
                    .ok_or(StoreError::InvalidWasmFileName)?
                    .to_str()
                    .ok_or(StoreError::InvalidWasmFileName)?;

                self.contract_map
                    .register_contract(contract.to_string(), res.code_id);

                for prof in &mut self.profilers {
                    prof.instrument(
                        contract.to_string(),
                        "Store".to_string(),
                        CommandType::Store,
                        &res.data,
                        caller_loc,
                        0,
                    )
                    .map_err(StoreError::instrument)?;
                }

                responses.push(res.data);
            }
        }
        Ok(responses)
    }

    /// Executes multiple smart contract operations against the configured chain
    /// returning the raw cosmos json responses.
    #[track_caller]
    pub fn process_msgs<X, Y, Z, S>(
        &mut self,
        contract_name: S,
        op_name: S,
        msgs: &[WasmMsg<X, Y, Z>],
        key: &SigningKey,
    ) -> Result<Vec<TxResult>, ProcessError>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
        S: Into<String>,
    {
        let caller_loc = Location::caller();
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let mut responses = vec![];
        for (idx, msg) in msgs.iter().enumerate() {
            let res = self.process_msg_internal(
                contract_name.clone(),
                op_name.clone(),
                msg,
                key,
                idx,
                caller_loc,
            )?;
            responses.push(res);
        }

        Ok(responses)
    }

    /// Executes a single smart contract operation against the configured chain
    /// returning the raw cosmos json response.
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name for the corresponding `msg`.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    ///
    /// # Errors (TODO: Add all docs for all errors)
    #[track_caller]
    pub fn process_msg<X, Y, Z, S>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &WasmMsg<X, Y, Z>,
        key: &SigningKey,
    ) -> Result<TxResult, ProcessError>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
        S: Into<String>,
    {
        let caller_loc = Location::caller();
        self.process_msg_internal(
            contract_name.into(),
            op_name.into(),
            msg,
            key,
            0,
            caller_loc,
        )
    }

    // process_msg_internal is a private method with an index
    // of the passed in message for profiler bookkeeping
    fn process_msg_internal<X, Y, Z>(
        &mut self,
        contract_name: String,
        op_name: String,
        msg: &WasmMsg<X, Y, Z>,
        key: &SigningKey,
        idx: usize,
        caller_loc: &Location,
    ) -> Result<TxResult, ProcessError>
    where
        X: Serialize,
        Y: Serialize,
        Z: Serialize,
    {
        let code_id = self.contract_map.code_id(&contract_name)?;

        let res = match msg {
            WasmMsg::InstantiateMsg(m) => {
                let payload = serde_json::to_vec(&m).map_err(ProcessError::json)?;

                let res = tokio_block(async {
                    self.client
                        .instantiate(code_id, payload, &key.clone().try_into()?)
                        .await
                })?;

                self.contract_map.add_address(&contract_name, res.address)?;

                res.data
            }
            WasmMsg::ExecuteMsg(m) => {
                let payload = serde_json::to_vec(&m).map_err(ProcessError::json)?;
                let addr = self.contract_map.address(&contract_name)?;

                let res = tokio_block(async {
                    self.client
                        .execute(addr, payload, &key.clone().try_into()?)
                        .await
                })?;

                res.data
            }
            WasmMsg::QueryMsg(m) => {
                let payload = serde_json::to_vec(&m).map_err(ProcessError::json)?;
                let addr = self.contract_map.address(&contract_name)?;

                let res = tokio_block(async { self.client.query(addr, payload).await })?;

                res.data
            }
        };

        for prof in &mut self.profilers {
            prof.instrument(
                contract_name.clone(),
                op_name.clone(),
                msg.into(),
                &res,
                caller_loc,
                idx,
            )
            .map_err(ProcessError::instrument)?;
        }

        debug!("{:?}", res);
        Ok(res)
    }

    /// Get instrumentation reports for each configured profiler.
    pub fn profiler_reports(&self) -> Result<Vec<Report>, ReportError> {
        let mut reports = vec![];
        for prof in &self.profilers {
            reports.push(
                prof.report()
                    .map_err(|e| ReportError::ReportError { source: e })?,
            );
        }

        Ok(reports)
    }
}
