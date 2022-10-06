use serde::Serialize;
use std::fmt::{self, Debug};
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

// TODO: Figure out the best way to reuse these tests between the async and blocking CosmOrc APIs
// Since they are practically identical
// <---
// <---

#[cfg(test)]
mod tests {
    use super::CosmOrc;
    use crate::client::chain_res::{
        ChainResponse, ExecResponse, InstantiateResponse, MigrateResponse, QueryResponse,
    };
    use crate::client::cosmwasm::CosmWasmClient;
    use crate::orchestrator::deploy::DeployInfo;
    use crate::orchestrator::gas_profiler::GasProfiler;
    use crate::{
        client::error::ClientError,
        config::key::{Key, SigningKey},
        orchestrator::{
            deploy::ContractMap,
            error::{ContractMapError, ProcessError, StoreError},
        },
    };
    use assert_matches::assert_matches;
    use cosmrs::tendermint::abci::Code;
    use serde::Serialize;
    use std::collections::HashMap;

    #[derive(Serialize)]
    pub struct TestMsg {}

    #[tokio::test]
    async fn instantiate_not_stored() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        let res = cosm_orc
            .instantiate("cw_not_stored", "i_test", &TestMsg {}, &key, None, vec![])
            .await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn instantiate_cosmossdk_error() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload, key.clone(), None, vec![])).then(
            |(_, _, _, _, _)| {
                Err(ClientError::CosmosSdk {
                    res: ChainResponse {
                        code: Code::Err(10),
                        data: Some(vec![]),
                        log: "error log".to_string(),
                        gas_used: 1001,
                        gas_wanted: 1002,
                    },
                })
            },
        );

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn instantiate() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload, key.clone(), None, vec![])).then(
            |(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            },
        );

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn instantiate_with_profiler() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload, key.clone(), None, vec![])).then(
            |(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            },
        );

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: Some(GasProfiler::new()),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let report = cosm_orc.gas_profiler_report().unwrap();
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

    #[tokio::test]
    async fn execute_not_stored() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        let res = cosm_orc
            .execute("cw_not_stored", "e_test", &TestMsg {}, &key, vec![])
            .await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn execute_not_initialized() {
        let code_ids = HashMap::from([(
            "cw_not_init".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc
            .execute("cw_not_init", "e_test", &TestMsg {}, &key, vec![])
            .await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn execute_cosmossdk_error() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload.clone(), key.clone(), None, vec![]))
            .then(|(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        faux::when!(mock_client.execute(
            "cosmos_contract_addr".to_string(),
            payload,
            key.clone(),
            vec![]
        ))
        .then(|(_, _, _, _)| {
            Err(ClientError::CosmosSdk {
                res: ChainResponse {
                    code: Code::Err(10),
                    data: Some(vec![]),
                    log: "error log".to_string(),
                    gas_used: 1001,
                    gas_wanted: 1002,
                },
            })
        });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let res = cosm_orc
            .execute("cw_test", "e_test", msg, &key, vec![])
            .await;

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ClientError(ClientError::CosmosSdk { .. })
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn execute() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload.clone(), key.clone(), None, vec![]))
            .then(|(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        faux::when!(mock_client.execute(
            "cosmos_contract_addr".to_string(),
            payload,
            key.clone(),
            vec![]
        ))
        .then(|(_, _, _, _)| {
            Ok(ExecResponse {
                res: ChainResponse {
                    code: Code::Ok,
                    data: Some(vec![]),
                    log: "log".to_string(),
                    gas_used: 2001,
                    gas_wanted: 2002,
                },
                height: 1234,
                tx_hash: "35AD02A".to_string(),
            })
        });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );
        assert_eq!(cosm_orc.gas_profiler_report(), None);

        let res = cosm_orc
            .execute("cw_test", "e_test", msg, &key, vec![])
            .await
            .unwrap()
            .res;
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn execute_with_profiler() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload.clone(), key.clone(), None, vec![]))
            .then(|(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        faux::when!(mock_client.execute(
            "cosmos_contract_addr".to_string(),
            payload,
            key.clone(),
            vec![]
        ))
        .then(|(_, _, _, _)| {
            Ok(ExecResponse {
                res: ChainResponse {
                    code: Code::Ok,
                    data: Some(vec![]),
                    log: "log".to_string(),
                    gas_used: 2001,
                    gas_wanted: 2002,
                },
                height: 1234,
                tx_hash: "35AD02A".to_string(),
            })
        });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: Some(GasProfiler::new()),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let report = cosm_orc.gas_profiler_report().unwrap();
        assert_eq!(report.keys().len(), 1);
        assert_eq!(report.get("cw_test").unwrap().keys().len(), 1);

        let r = report
            .get("cw_test")
            .unwrap()
            .get("Instantiate__i_test")
            .unwrap();
        assert_eq!(r.gas_used, 100);
        assert_eq!(r.gas_wanted, 101);

        let res = cosm_orc
            .execute("cw_test", "e_test", msg, &key, vec![])
            .await
            .unwrap()
            .res;
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        let report = cosm_orc.gas_profiler_report().unwrap();
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

    #[tokio::test]
    async fn query_not_stored() {
        let cosm_orc = CosmOrc {
            contract_map: ContractMap::new(HashMap::new()),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        let res = cosm_orc.query("cw_not_stored", &TestMsg {}).await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn query_not_initialized() {
        let code_ids = HashMap::from([(
            "cw_not_init".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);

        let cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc.query("cw_not_init", &TestMsg {}).await;

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

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn query_cosmossdk_error() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload.clone(), key.clone(), None, vec![]))
            .then(|(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        faux::when!(mock_client.query("cosmos_contract_addr".to_string(), payload)).then(
            |(_, _)| {
                Err(ClientError::CosmosSdk {
                    res: ChainResponse {
                        code: Code::Err(10),
                        data: Some(vec![]),
                        log: "error log".to_string(),
                        gas_used: 1001,
                        gas_wanted: 1002,
                    },
                })
            },
        );

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );

        let res = cosm_orc.query("cw_test", msg).await;

        assert_matches!(
            res.unwrap_err(),
            ProcessError::ClientError(ClientError::CosmosSdk { .. })
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn query() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        faux::when!(mock_client.instantiate(1337, payload.clone(), key.clone(), None, vec![]))
            .then(|(_, _, _, _, _)| {
                Ok(InstantiateResponse {
                    address: "cosmos_contract_addr".to_string(),
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        faux::when!(mock_client.query("cosmos_contract_addr".to_string(), payload)).then(
            |(_, _)| {
                Ok(QueryResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "log".to_string(),
                        gas_used: 2001,
                        gas_wanted: 2002,
                    },
                })
            },
        );

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "cosmos_contract_addr".to_string()
        );
        assert_eq!(cosm_orc.gas_profiler_report(), None);

        let res = cosm_orc.query("cw_test", msg).await.unwrap().res;
        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "log".to_string());
        assert_eq!(res.gas_used, 2001);
        assert_eq!(res.gas_wanted, 2002);

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[tokio::test]
    async fn store_invalid_wasm_dir() {
        let code_ids = HashMap::new();
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmWasmClient::faux(),
            gas_profiler: None,
        };

        let res = cosm_orc.store_contracts("invalid_dir", &key, None).await;
        assert_matches!(res.unwrap_err(), StoreError::WasmDirRead { .. });
    }

    #[tokio::test]
    async fn migrate() {
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: Some("addr1".to_string()),
            },
        )]);
        let key = SigningKey {
            name: "test".to_string(),
            key: Key::Mnemonic("word1 word2".to_string()),
        };

        let msg = &TestMsg {};
        let payload = serde_json::to_vec(msg).unwrap();

        let mut mock_client = CosmWasmClient::faux();

        let new_code_id = 1338;

        faux::when!(mock_client.migrate("addr1".to_string(), new_code_id, payload, key.clone()))
            .then(|(_, _, _, _)| {
                Ok(MigrateResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                        gas_used: 100,
                        gas_wanted: 101,
                    },
                    height: 1234,
                    tx_hash: "35AD02A".to_string(),
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: mock_client,
            gas_profiler: None,
        };

        let res = cosm_orc
            .migrate("cw_test", new_code_id, "migrate_op", msg, &key)
            .await
            .unwrap()
            .res;

        assert_eq!(res.code, Code::Ok);
        assert_eq!(res.data, Some(vec![]));
        assert_eq!(res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "addr1".to_string()
        );

        // code_id is the newly migrated id:
        assert_eq!(cosm_orc.contract_map.code_id("cw_test").unwrap(), 1338);

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }
}
