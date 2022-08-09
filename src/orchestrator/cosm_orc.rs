use log::{debug, info};
use serde::Serialize;
use std::ffi::OsStr;
use std::fmt::{self, Debug};
use std::fs;
use std::panic::Location;
use std::path::Path;

use super::error::{ProcessError, ReportError, StoreError};
use crate::client::cosm_client::{tokio_block, CosmClient, TendermintRes};
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
    /// saving the resulting contract ids in `contract_map`.
    ///
    /// You don't need to call this function if all of the smart contract ids
    /// are already configured via `config::cfg::Config::code_ids`.
    ///
    /// NOTE: Currently, the name of the wasm files in `wasm_dir` will be
    /// used as the `contract_name` parameter to `instantiate()`, `query()` and `execute()`.
    #[track_caller]
    pub fn store_contracts(
        &mut self,
        wasm_dir: &str,
        key: &SigningKey,
    ) -> Result<Vec<TendermintRes>, StoreError> {
        let mut responses = vec![];
        let wasm_path = Path::new(wasm_dir);

        for wasm in fs::read_dir(wasm_path).map_err(StoreError::wasmdir)? {
            let wasm_path = wasm?.path();
            if wasm_path.extension() == Some(OsStr::new("wasm")) {
                info!("Storing {:?}", wasm_path);

                let wasm = fs::read(&wasm_path).map_err(StoreError::wasmfile)?;

                let res = tokio_block(async { self.client.store(wasm, key).await })?;

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
                        &res.res,
                        Location::caller(),
                    )
                    .map_err(StoreError::instrument)?;
                }

                responses.push(res.res);
            }
        }
        Ok(responses)
    }

    /// Initializes a smart contract against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Stored smart contract name for the corresponding `msg`.
    /// * `msg` - InstantiateMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    /// * `key` - SigningKey used to sign the tx
    ///
    /// # Errors
    /// * If `contract_name` has not been configured in `Config::code_ids` or stored through
    ///   [Self::store_contracts()] `cosm_orc::orchestrator::error::ContractMapError::NotStored` is thrown.
    #[track_caller]
    pub fn instantiate<S, T>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &T,
        key: &SigningKey,
    ) -> Result<TendermintRes, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let code_id = self.contract_map.code_id(&contract_name)?;

        let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

        let res = tokio_block(async { self.client.instantiate(code_id, payload, key).await })?;

        self.contract_map.add_address(&contract_name, res.address)?;

        for prof in &mut self.profilers {
            prof.instrument(
                contract_name.clone(),
                op_name.clone(),
                CommandType::Instantiate,
                &res.res,
                Location::caller(),
            )
            .map_err(ProcessError::instrument)?;
        }

        debug!("{:?}", res.res);

        Ok(res.res)
    }

    /// Executes a smart contract operation against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name for the corresponding `msg`.
    /// * `msg` - ExecuteMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    /// * `key` - SigningKey used to sign the tx
    ///
    /// # Errors
    /// * If `contract_name` has not been instantiated via [Self::instantiate()]
    ///   `cosm_orc::orchestrator::error::ContractMapError::NotDeployed` is thrown.
    #[track_caller]
    pub fn execute<S, T>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &T,
        key: &SigningKey,
    ) -> Result<TendermintRes, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let addr = self.contract_map.address(&contract_name)?;

        let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

        let res = tokio_block(async { self.client.execute(addr, payload, key).await })?;

        for prof in &mut self.profilers {
            prof.instrument(
                contract_name.clone(),
                op_name.clone(),
                CommandType::Execute,
                &res.res,
                Location::caller(),
            )
            .map_err(ProcessError::instrument)?;
        }

        debug!("{:?}", res.res);

        Ok(res.res)
    }

    /// Queries a smart contract operation against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name for the corresponding `msg`.
    /// * `msg` - QueryMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    ///
    /// # Errors
    /// * If `contract_name` has not been instantiated via [Self::instantiate()]
    ///   `cosm_orc::orchestrator::error::ContractMapError::NotDeployed` is thrown.
    #[track_caller]
    pub fn query<S, T>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &T,
    ) -> Result<TendermintRes, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let addr = self.contract_map.address(&contract_name)?;

        let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

        let res = tokio_block(async { self.client.query(addr, payload).await })?;

        for prof in &mut self.profilers {
            prof.instrument(
                contract_name.clone(),
                op_name.clone(),
                CommandType::Query,
                &res.res,
                Location::caller(),
            )
            .map_err(ProcessError::instrument)?;
        }

        debug!("{:?}", res.res);

        Ok(res.res)
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
