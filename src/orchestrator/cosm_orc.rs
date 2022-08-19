use log::{debug, info};
use mockall_double::double;
use serde::Serialize;
use std::env::consts::ARCH;
use std::ffi::OsStr;
use std::fmt::{self, Debug};
use std::fs;
use std::panic::Location;
use std::path::Path;

#[cfg(feature = "optimization")]
use super::error::OptimizeError;

use super::error::{ProcessError, ReportError, StoreError};
use crate::client::cosm_client::{tokio_block, TendermintRes};
use crate::client::error::ClientError;
use crate::config::cfg::Config;
use crate::config::key::SigningKey;
use crate::orchestrator::deploy::ContractMap;
use crate::profilers::profiler::{CommandType, Profiler, Report};

#[double]
use crate::client::cosm_client::CosmClient;

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

    /// Build and optimize all smart contracts in a given workspace.
    /// `workspace_path` is the path to the Cargo.toml or directory containing the Cargo.toml.
    #[cfg(feature = "optimization")]
    pub fn optimize_contracts(&self, workspace_path: &str) -> Result<(), OptimizeError> {
        let workspace_path = Path::new(workspace_path);
        tokio_block(async { cw_optimizoor::run(workspace_path).await })
            .map_err(|e| OptimizeError::Optimize { source: e.into() })?;
        Ok(())
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
    /// * `key` - SigningKey used to sign the tx.
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

#[cfg(test)]
mod tests {
    use super::CosmOrc;
    use crate::{
        client::{
            cosm_client::{ExecResponse, InstantiateResponse, QueryResponse},
            error::ClientError,
            TendermintRes,
        },
        config::key::{Key, SigningKey},
        orchestrator::{
            deploy::ContractMap,
            error::{ContractMapError, ProcessError, StoreError},
        },
        profilers::gas_profiler::{GasProfiler, GasReport},
    };
    use assert_matches::assert_matches;
    use cosmrs::tendermint::abci::Code;
    use mockall::predicate::{eq, function};
    use mockall_double::double;
    use serde::Serialize;
    use std::collections::HashMap;

    #[double]
    use crate::client::cosm_client::CosmClient;

    #[derive(Serialize)]
    pub struct TestMsg {}

    #[test]
    fn instantiate_not_stored() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: CosmClient::default(),
            profilers: vec![],
        };

        let res = cosm_orc.instantiate("cw_not_stored", "i_test", &TestMsg {}, &key);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ContractMapError(e) if e == ContractMapError::NotStored{name: "cw_not_stored".to_string()}
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_stored").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw_not_stored".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn instantiate_cosmossdk_error() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Err(ClientError::CosmosSdk {
                    res: TendermintRes {
                        code: Code::Err(10),
                        data: Some(vec![]),
                        log: "error log".to_string(),
                        gas_used: 1001,
                        gas_wanted: 1002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        let res = cosm_orc.instantiate("cw_test", "i_test", msg, &key);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ClientError(ClientError::CosmosSdk { .. })
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn instantiate() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn instantiate_with_profiler() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        }
        .add_profiler(Box::new(GasProfiler::new()));

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let reports = cosm_orc.profiler_reports().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "gas-profiler".to_string());

        let report: HashMap<String, HashMap<String, GasReport>> =
            serde_json::from_slice(&reports[0].json_data).unwrap();
        assert_eq!(report.keys().len(), 1);
        assert_eq!(report.get("cw_test").unwrap().keys().len(), 1);

        let r = report
            .get("cw_test")
            .unwrap()
            .get("Instantiate__i_test")
            .unwrap();
        assert_eq!(r.gas_used, 100);
        assert_eq!(r.gas_wanted, 101);
    }

    #[test]
    fn execute_not_stored() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: CosmClient::default(),
            profilers: vec![],
        };

        let res = cosm_orc.execute("cw_not_stored", "e_test", &TestMsg {}, &key);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ContractMapError(e) if e == ContractMapError::NotStored{name: "cw_not_stored".to_string()}
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_stored").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw_not_stored".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn execute_not_initialized() {
        let code_ids = HashMap::from([("cw_not_init".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: CosmClient::default(),
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc.execute("cw_not_init", "e_test", &TestMsg {}, &key);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ContractMapError(e) if e == ContractMapError::NotDeployed{name: "cw_not_init".to_string()}
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn execute_cosmossdk_error() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload.clone()),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        mock_client
            .expect_execute()
            .with(
                eq("cosmos_contract_addr".to_string()),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Err(ClientError::CosmosSdk {
                    res: TendermintRes {
                        code: Code::Err(10),
                        data: Some(vec![]),
                        log: "error log".to_string(),
                        gas_used: 1001,
                        gas_wanted: 1002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let res = cosm_orc.execute("cw_test", "e_test", msg, &key);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ClientError(ClientError::CosmosSdk { .. })
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn execute() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload.clone()),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        mock_client
            .expect_execute()
            .with(
                eq("cosmos_contract_addr".to_string()),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(ExecResponse {
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "log".to_string(),
                        gas_used: 2001,
                        gas_wanted: 2002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );
        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);

        let res = cosm_orc.execute("cw_test", "e_test", msg, &key).unwrap();
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn execute_with_profiler() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload.clone()),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        mock_client
            .expect_execute()
            .with(
                eq("cosmos_contract_addr".to_string()),
                eq(payload),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(ExecResponse {
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "log".to_string(),
                        gas_used: 2001,
                        gas_wanted: 2002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![Box::new(GasProfiler::new())],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let reports = cosm_orc.profiler_reports().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "gas-profiler".to_string());

        let report: HashMap<String, HashMap<String, GasReport>> =
            serde_json::from_slice(&reports[0].json_data).unwrap();
        assert_eq!(report.keys().len(), 1);
        assert_eq!(report.get("cw_test").unwrap().keys().len(), 1);

        let r = report
            .get("cw_test")
            .unwrap()
            .get("Instantiate__i_test")
            .unwrap();
        assert_eq!(r.gas_used, 100);
        assert_eq!(r.gas_wanted, 101);

        let res = cosm_orc.execute("cw_test", "e_test", msg, &key).unwrap();
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        let reports = cosm_orc.profiler_reports().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "gas-profiler".to_string());

        let report: HashMap<String, HashMap<String, GasReport>> =
            serde_json::from_slice(&reports[0].json_data).unwrap();
        assert_eq!(report.keys().len(), 1);
        assert_eq!(report.get("cw_test").unwrap().keys().len(), 2);

        let r = report
            .get("cw_test")
            .unwrap()
            .get("Execute__e_test")
            .unwrap();
        assert_eq!(r.gas_used, 2001);
        assert_eq!(r.gas_wanted, 2002);
    }

    #[test]
    fn query_not_stored() {
        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&HashMap::new()),
            client: CosmClient::default(),
            profilers: vec![],
        };

        let res = cosm_orc.query("cw_not_stored", "q_test", &TestMsg {});

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ContractMapError(e) if e == ContractMapError::NotStored{name: "cw_not_stored".to_string()}
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_stored").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw_not_stored".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn query_not_initialized() {
        let code_ids = HashMap::from([("cw_not_init".to_string(), 1337)]);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: CosmClient::default(),
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc.query("cw_not_init", "q_test", &TestMsg {});

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ContractMapError(e) if e == ContractMapError::NotDeployed{name: "cw_not_init".to_string()}
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn query_cosmossdk_error() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload.clone()),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        mock_client
            .expect_query()
            .with(eq("cosmos_contract_addr".to_string()), eq(payload))
            .returning(|_, _| {
                Err(ClientError::CosmosSdk {
                    res: TendermintRes {
                        code: Code::Err(10),
                        data: Some(vec![]),
                        log: "error log".to_string(),
                        gas_used: 1001,
                        gas_wanted: 1002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let res = cosm_orc.query("cw_test", "q_test", msg);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ClientError(ClientError::CosmosSdk { .. })
        );

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn query() {
        let code_ids = HashMap::from([("cw_test".to_string(), 1337)]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmClient::default();

        mock_client
            .expect_instantiate()
            .with(
                eq(1337),
                eq(payload.clone()),
                function(move |k: &SigningKey| {
                    let Key::Mnemonic(phrase) = &k.key;
                    k.name == "test" && phrase == "word1 word2"
                }),
            )
            .returning(|_, _, _| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                })
            })
            .times(1);

        mock_client
            .expect_query()
            .with(eq("cosmos_contract_addr".to_string()), eq(payload))
            .returning(|_, _| {
                Ok(QueryResponse {
                    res: TendermintRes {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "log".to_string(),
                        gas_used: 2001,
                        gas_wanted: 2002,
                    },
                })
            })
            .times(1);

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: mock_client,
            profilers: vec![],
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key)
            .unwrap();

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );
        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);

        let res = cosm_orc.query("cw_test", "q_test", msg).unwrap();
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        assert_eq!(cosm_orc.profiler_reports().unwrap().len(), 0);
    }

    #[test]
    fn store_invalid_wasm_dir() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(&code_ids),
            client: CosmClient::default(),
            profilers: vec![],
        };

        let res = cosm_orc.store_contracts("invalid_dir", &key);
        assert_matches!(res.unwrap_err(), StoreError::WasmDirRead { .. });
    }
}
