use serde::Serialize;
use std::fmt::{self, Debug};
use std::panic::Location;
use std::time::Duration;

use crate::client::chain_res::{
    ExecResponse, InstantiateResponse, MigrateResponse, QueryResponse, StoreCodeResponse,
};
use crate::client::cosmwasm::CosmWasmClient;
use crate::config::cfg::Coin;
use crate::config::key::SigningKey;
use crate::orchestrator::deploy::ContractMap;
use crate::orchestrator::error::{PollBlockError, ProcessError, StoreError};
use crate::orchestrator::gas_profiler::{GasProfiler, Report};
use crate::orchestrator::{internal_api, AccessConfig};

#[cfg(feature = "optimize")]
use crate::orchestrator::error::OptimizeError;

#[cfg(not(test))]
use crate::client::error::ClientError;
#[cfg(not(test))]
use crate::config::cfg::Config;

/// Async version of [crate::orchestrator::cosm_orc]
///
/// Stores cosmwasm contracts and executes their messages against the configured chain.
#[derive(Clone)]
pub struct CosmOrc {
    pub contract_map: ContractMap,
    client: CosmWasmClient,
    gas_profiler: Option<GasProfiler>,
}

impl Debug for CosmOrc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.contract_map)
    }
}

impl CosmOrc {
    /// Creates a CosmOrc object from the supplied Config,
    /// optionally using a gas profiler
    #[cfg(not(test))]
    pub fn new(cfg: Config, use_gas_profiler: bool) -> Result<Self, ClientError> {
        let gas_profiler = if use_gas_profiler {
            Some(GasProfiler::new())
        } else {
            None
        };

        Ok(Self {
            contract_map: ContractMap::new(cfg.contract_deploy_info),
            client: CosmWasmClient::new(cfg.chain_cfg)?,
            gas_profiler,
        })
    }

    /// Build and optimize all smart contracts in a given workspace.
    /// `workspace_path` is the path to the Cargo.toml or directory containing the Cargo.toml.
    #[cfg(feature = "optimize")]
    pub async fn optimize_contracts(&self, workspace_path: &str) -> Result<(), OptimizeError> {
        internal_api::optimize_contracts(workspace_path).await
    }

    /// Uploads the optimized contracts in `wasm_dir` to the configured chain
    /// saving the resulting contract ids in `contract_map`.
    ///
    /// You don't need to call this function if all of the smart contract ids
    /// are already configured via `config::cfg::Config::code_ids`.
    ///
    /// If you have not built and optimized the wasm files, use [Self::optimize_contracts()]
    ///
    /// NOTE: Currently, the name of the wasm files in `wasm_dir` will be
    /// used as the `contract_name` parameter to `instantiate()`, `query()` and `execute()`.
    #[track_caller]
    pub async fn store_contracts(
        &mut self,
        wasm_dir: &str,
        key: &SigningKey,
        instantiate_perms: Option<AccessConfig>,
    ) -> Result<Vec<StoreCodeResponse>, StoreError> {
        internal_api::store_contracts(
            &mut self.contract_map,
            &self.client,
            &mut self.gas_profiler,
            wasm_dir,
            key,
            instantiate_perms,
            &Location::caller().into(),
        )
        .await
    }

    /// Initializes a smart contract against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Stored smart contract name for the corresponding `msg`.
    /// * `msg` - InstantiateMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    /// * `key` - SigningKey used to sign the tx.
    /// * `admin` - Optional admin address for contract migration.
    /// * `funds` - Optional tokens transferred to the contract after instantiation.
    ///
    /// # Errors
    /// * If `contract_name` has not been configured in `Config::code_ids` or stored through
    ///   [Self::store_contracts()] `cosm_orc::orchestrator::error::ContractMapError::NotStored` is thrown.
    #[track_caller]
    pub async fn instantiate<S, T>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &T,
        key: &SigningKey,
        admin: Option<String>,
        funds: Vec<Coin>,
    ) -> Result<InstantiateResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        internal_api::instantiate(
            &mut self.contract_map,
            &self.client,
            &mut self.gas_profiler,
            contract_name,
            op_name,
            msg,
            key,
            admin,
            funds,
            &Location::caller().into(),
        )
        .await
    }

    /// Executes a smart contract operation against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name for the corresponding `msg`.
    /// * `msg` - ExecuteMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    /// * `key` - SigningKey used to sign the tx.
    /// * `funds` - Optional tokens transferred to the contract after execution.
    ///
    /// # Errors
    /// * If `contract_name` has not been instantiated via [Self::instantiate()]
    ///   `cosm_orc::orchestrator::error::ContractMapError::NotDeployed` is thrown.
    #[track_caller]
    pub async fn execute<S, T>(
        &mut self,
        contract_name: S,
        op_name: S,
        msg: &T,
        key: &SigningKey,
        funds: Vec<Coin>,
    ) -> Result<ExecResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        internal_api::execute(
            &mut self.contract_map,
            &self.client,
            &mut self.gas_profiler,
            contract_name,
            op_name,
            msg,
            key,
            funds,
            &Location::caller().into(),
        )
        .await
    }

    /// Queries a smart contract operation against the configured chain.
    ///
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name for the corresponding `msg`.
    /// * `msg` - QueryMsg that `contract_name` supports.
    ///
    /// # Errors
    /// * If `contract_name` has not been instantiated via [Self::instantiate()]
    ///   `cosm_orc::orchestrator::error::ContractMapError::NotDeployed` is thrown.
    #[track_caller]
    pub async fn query<S, T>(
        &self,
        contract_name: S,
        msg: &T,
    ) -> Result<QueryResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        internal_api::query(&self.contract_map, &self.client, contract_name, msg).await
    }

    /// Migrates a smart contract deployed at `contract_name` to `new_code_id`
    ///
    /// # Arguments
    /// * `contract_name` - Deployed smart contract name that we will migrate.
    /// * `new_code_id` - New code id that we will migrate `contract_name` to.
    /// * `msg` - MigrateMsg that `contract_name` supports.
    /// * `op_name` - Human readable operation name for profiling bookkeeping usage.
    /// * `key` - SigningKey used to sign the tx.
    #[track_caller]
    pub async fn migrate<S, T>(
        &mut self,
        contract_name: S,
        new_code_id: u64,
        op_name: S,
        msg: &T,
        key: &SigningKey,
    ) -> Result<MigrateResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        internal_api::migrate(
            &mut self.contract_map,
            &self.client,
            &mut self.gas_profiler,
            contract_name,
            new_code_id,
            op_name,
            msg,
            key,
            &Location::caller().into(),
        )
        .await
    }

    /// Blocks the current thread until `n` blocks have been processed.
    /// # Arguments
    /// * `n` - Wait for this number of blocks to process
    /// * `timeout` - Throws `PollBlockError` once `timeout` has elapsed.
    /// * `is_first_block` - Set to true if waiting for the first block to process for new test nodes.
    pub async fn poll_for_n_blocks<T: Into<Duration> + Send>(
        &self,
        n: u64,
        timeout: T,
        is_first_block: bool,
    ) -> Result<(), PollBlockError> {
        internal_api::poll_for_n_blocks(&self.client, n, timeout, is_first_block).await
    }

    /// Get gas usage report
    pub fn gas_profiler_report(&self) -> Option<&Report> {
        self.gas_profiler.as_ref().map(|p| p.report())
    }
}
