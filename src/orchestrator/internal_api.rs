use log::{debug, info};
use serde::Serialize;
use std::env::consts::ARCH;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout as _timeout;

use super::error::{PollBlockError, ProcessError, StoreError};
use super::gas_profiler::CallLoc;
use crate::client::chain_res::{
    ExecResponse, InstantiateResponse, MigrateResponse, QueryResponse, StoreCodeResponse,
};
use crate::client::cosmwasm::CosmWasmClient;
use crate::config::cfg::Coin;
use crate::config::key::SigningKey;
use crate::orchestrator::deploy::ContractMap;
use crate::orchestrator::gas_profiler::{CommandType, GasProfiler};
use crate::orchestrator::AccessConfig;

#[cfg(feature = "optimize")]
use super::error::OptimizeError;

// Internal implementation details used by both the async and blocking public APIs

#[cfg(feature = "optimize")]
pub(crate) async fn optimize_contracts(workspace_path: &str) -> Result<(), OptimizeError> {
    let workspace_path = Path::new(workspace_path);
    cw_optimizoor::run(workspace_path)
        .await
        .map_err(|e| OptimizeError::Optimize { source: e.into() })?;
    Ok(())
}

pub(crate) async fn store_contracts(
    contract_map: &mut ContractMap,
    client: &CosmWasmClient,
    gas_profiler: &mut Option<GasProfiler>,
    wasm_dir: &str,
    key: &SigningKey,
    instantiate_perms: Option<AccessConfig>,
    caller_loc: &CallLoc,
) -> Result<Vec<StoreCodeResponse>, StoreError> {
    let mut responses = vec![];
    let wasm_path = Path::new(wasm_dir);

    for wasm in fs::read_dir(wasm_path).map_err(StoreError::wasmdir)? {
        let wasm_path = wasm?.path();
        if wasm_path.extension() == Some(OsStr::new("wasm")) {
            info!("Storing {:?}", wasm_path);

            let wasm = fs::read(&wasm_path).map_err(StoreError::wasmfile)?;

            let res = client.store(wasm, key, instantiate_perms.clone()).await?;

            let mut contract = wasm_path
                .file_stem()
                .ok_or(StoreError::InvalidWasmFileName)?
                .to_str()
                .ok_or(StoreError::InvalidWasmFileName)?;

            // parse out OS architecture if optimizoor was used:
            let arch_suffix = format!("-{}", ARCH);
            if contract.to_string().ends_with(&arch_suffix) {
                contract = contract.trim_end_matches(&arch_suffix);
            }

            contract_map.register_contract(contract.to_string(), res.code_id);

            if let Some(p) = gas_profiler {
                p.instrument(
                    contract.to_string(),
                    "Store".to_string(),
                    CommandType::Store,
                    &res.res,
                    caller_loc,
                );
            }

            responses.push(res);
        }
    }
    Ok(responses)
}

// TODO: Clean up this internal_api interface and remove these clippy allows
#[allow(clippy::too_many_arguments)]
pub(crate) async fn instantiate<S, T>(
    contract_map: &mut ContractMap,
    client: &CosmWasmClient,
    gas_profiler: &mut Option<GasProfiler>,
    contract_name: S,
    op_name: S,
    msg: &T,
    key: &SigningKey,
    admin: Option<String>,
    funds: Vec<Coin>,
    caller_loc: &CallLoc,
) -> Result<InstantiateResponse, ProcessError>
where
    S: Into<String>,
    T: Serialize,
{
    let contract_name = contract_name.into();
    let op_name = op_name.into();

    let code_id = contract_map.code_id(&contract_name)?;

    let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

    let res = client
        .instantiate(code_id, payload, key, admin, funds)
        .await?;

    contract_map.add_address(&contract_name, res.address.clone())?;

    if let Some(p) = gas_profiler {
        p.instrument(
            contract_name,
            op_name,
            CommandType::Instantiate,
            &res.res,
            caller_loc,
        );
    }

    debug!("{:?}", res.res);

    Ok(res)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute<S, T>(
    contract_map: &mut ContractMap,
    client: &CosmWasmClient,
    gas_profiler: &mut Option<GasProfiler>,
    contract_name: S,
    op_name: S,
    msg: &T,
    key: &SigningKey,
    funds: Vec<Coin>,
    caller_loc: &CallLoc,
) -> Result<ExecResponse, ProcessError>
where
    S: Into<String>,
    T: Serialize,
{
    let contract_name = contract_name.into();
    let op_name = op_name.into();

    let addr = contract_map.address(&contract_name)?;

    let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

    let res = client.execute(addr, payload, key, funds).await?;

    if let Some(p) = gas_profiler {
        p.instrument(
            contract_name,
            op_name,
            CommandType::Execute,
            &res.res,
            caller_loc,
        );
    }

    debug!("{:?}", res.res);

    Ok(res)
}

pub(crate) async fn query<S, T>(
    contract_map: &ContractMap,
    client: &CosmWasmClient,
    contract_name: S,
    msg: &T,
) -> Result<QueryResponse, ProcessError>
where
    S: Into<String>,
    T: Serialize,
{
    let contract_name = contract_name.into();

    let addr = contract_map.address(&contract_name)?;

    let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

    let res = client.query(addr, payload).await?;

    debug!("{:?}", res.res);

    Ok(res)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn migrate<S, T>(
    contract_map: &mut ContractMap,
    client: &CosmWasmClient,
    gas_profiler: &mut Option<GasProfiler>,
    contract_name: S,
    new_code_id: u64,
    op_name: S,
    msg: &T,
    key: &SigningKey,
    caller_loc: &CallLoc,
) -> Result<MigrateResponse, ProcessError>
where
    S: Into<String>,
    T: Serialize,
{
    let contract_name = contract_name.into();
    let op_name = op_name.into();

    let addr = contract_map.address(&contract_name)?;

    let payload = serde_json::to_vec(msg).map_err(ProcessError::json)?;

    let res = client.migrate(addr, new_code_id, payload, key).await?;

    contract_map.register_contract(&contract_name, new_code_id);

    if let Some(p) = gas_profiler {
        p.instrument(
            contract_name,
            op_name,
            CommandType::Migrate,
            &res.res,
            caller_loc,
        );
    }

    debug!("{:?}", res.res);

    Ok(res)
}

pub(crate) async fn poll_for_n_blocks<T: Into<Duration> + Send>(
    client: &CosmWasmClient,
    n: u64,
    timeout: T,
    is_first_block: bool,
) -> Result<(), PollBlockError> {
    _timeout(timeout.into(), client.poll_for_n_blocks(n, is_first_block)).await??;

    Ok(())
}
