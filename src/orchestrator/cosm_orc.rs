use log::{debug, info};
use serde::Serialize;
use std::env::consts::ARCH;
use std::ffi::OsStr;
use std::fmt::{self, Debug};
use std::fs;
use std::future::Future;
use std::panic::Location;
use std::path::Path;
use std::time::Duration;
use tokio::time::{self, timeout as _timeout};

use cosm_tome::chain::coin::Coin;
use cosm_tome::chain::error::ChainError;
use cosm_tome::chain::request::TxOptions;
use cosm_tome::clients::client::{CosmTome, CosmosClient};
use cosm_tome::clients::cosmos_grpc::CosmosgRPC;
use cosm_tome::clients::tendermint_rpc::TendermintRPC;
use cosm_tome::modules::auth::model::Address;
use cosm_tome::modules::cosmwasm::model::{
    ExecRequest, ExecResponse, InstantiateRequest, InstantiateResponse, MigrateRequest,
    MigrateResponse, QueryResponse, StoreCodeRequest, StoreCodeResponse,
};
use cosm_tome::modules::tendermint::error::TendermintError;
use cosm_tome::signing_key::key::SigningKey;

use super::error::{PollBlockError, ProcessError, StoreError};
use crate::config::cfg::Config;
use crate::orchestrator::deploy::ContractMap;
use crate::orchestrator::gas_profiler::{CommandType, GasProfiler, Report};
use crate::orchestrator::AccessConfig;

#[cfg(feature = "optimize")]
use super::error::OptimizeError;

/// Stores cosmwasm contracts and executes their messages against the configured chain.
#[derive(Clone)]
pub struct CosmOrc<C: CosmosClient> {
    pub contract_map: ContractMap,
    client: CosmTome<C>,
    gas_profiler: Option<GasProfiler>,
    tx_options: TxOptions,
}

impl<C: CosmosClient> Debug for CosmOrc<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.contract_map)
    }
}

impl CosmOrc<CosmosgRPC> {
    /// Creates a CosmOrc object from the supplied Config, using the CosmosgRPC backing api
    /// optionally using a gas profiler
    pub fn new(cfg: Config, use_gas_profiler: bool) -> Result<CosmOrc<CosmosgRPC>, ChainError> {
        let gas_profiler = if use_gas_profiler {
            Some(GasProfiler::new())
        } else {
            None
        };

        Ok(CosmOrc {
            contract_map: ContractMap::new(cfg.contract_deploy_info),
            client: CosmTome::with_cosmos_grpc(cfg.chain_cfg)?,
            gas_profiler,
            tx_options: TxOptions::default(),
        })
    }
}

impl CosmOrc<TendermintRPC> {
    /// Creates a CosmOrc object from the supplied Config, using the tendermint RPC backing api
    /// optionally using a gas profiler
    pub fn new_tendermint_rpc(
        cfg: Config,
        use_gas_profiler: bool,
    ) -> Result<CosmOrc<TendermintRPC>, ChainError> {
        let gas_profiler = if use_gas_profiler {
            Some(GasProfiler::new())
        } else {
            None
        };

        Ok(CosmOrc {
            contract_map: ContractMap::new(cfg.contract_deploy_info),
            client: CosmTome::with_tendermint_rpc(cfg.chain_cfg)?,
            gas_profiler,
            tx_options: TxOptions::default(),
        })
    }
}

impl<C: CosmosClient> CosmOrc<C> {
    /// Build and optimize all smart contracts in a given workspace.
    /// `workspace_path` is the path to the Cargo.toml or directory containing the Cargo.toml.
    #[cfg(feature = "optimize")]
    pub fn optimize_contracts(&self, workspace_path: &str) -> Result<(), OptimizeError> {
        let workspace_path = Path::new(workspace_path);
        tokio_block(async { cw_optimizoor::run(workspace_path).await })
            .map_err(|e| OptimizeError::Optimize { source: e.into() })?;
        Ok(())
    }

    // TODO: Implement store_contract() that stores a single contract

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
        instantiate_perms: Option<AccessConfig>,
    ) -> Result<Vec<StoreCodeResponse>, StoreError> {
        let mut responses = vec![];
        let wasm_path = Path::new(wasm_dir);

        for wasm in fs::read_dir(wasm_path).map_err(StoreError::wasmdir)? {
            let wasm_path = wasm?.path();
            if wasm_path.extension() == Some(OsStr::new("wasm")) {
                info!("Storing {:?}", wasm_path);

                let wasm = fs::read(&wasm_path).map_err(StoreError::wasmfile)?;

                let res = tokio_block(async {
                    self.client
                        .wasm_store(
                            StoreCodeRequest {
                                wasm_data: wasm,
                                instantiate_perms: instantiate_perms.clone(),
                            },
                            key,
                            &self.tx_options,
                        )
                        .await
                })?;

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

                if let Some(p) = &mut self.gas_profiler {
                    p.instrument(
                        contract.to_string(),
                        "Store".to_string(),
                        CommandType::Store,
                        &res.res,
                        Location::caller(),
                    );
                }

                responses.push(res);
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
    /// * `admin` - Optional admin address for contract migration.
    /// * `funds` - Optional tokens transferred to the contract after instantiation.
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
        admin: Option<Address>,
        funds: Vec<Coin>,
    ) -> Result<InstantiateResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let code_id = self.contract_map.code_id(&contract_name)?;

        let res = tokio_block(async {
            self.client
                .wasm_instantiate(
                    InstantiateRequest {
                        code_id,
                        msg,
                        label: "cosm-orc".to_string(),
                        admin,
                        funds,
                    },
                    key,
                    &self.tx_options,
                )
                .await
        })?;

        self.contract_map
            .add_address(&contract_name, res.address.clone())?;

        if let Some(p) = &mut self.gas_profiler {
            p.instrument(
                contract_name,
                op_name,
                CommandType::Instantiate,
                &res.res,
                Location::caller(),
            );
        }

        debug!("{:?}", res.res);

        Ok(res)
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
    pub fn execute<S, T>(
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
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let addr = self.contract_map.address(&contract_name)?;

        let res = tokio_block(async {
            self.client
                .wasm_execute(
                    ExecRequest {
                        address: addr.parse()?,
                        msg,
                        funds,
                    },
                    key,
                    &self.tx_options,
                )
                .await
        })?;

        if let Some(p) = &mut self.gas_profiler {
            p.instrument(
                contract_name,
                op_name,
                CommandType::Execute,
                &res.res,
                Location::caller(),
            );
        }

        debug!("{:?}", res.res);

        Ok(res)
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
    pub fn query<S, T>(&self, contract_name: S, msg: &T) -> Result<QueryResponse, ProcessError>
    where
        S: Into<String>,
        T: Serialize,
    {
        let contract_name = contract_name.into();

        let addr = self.contract_map.address(&contract_name)?;

        let res = tokio_block(async { self.client.wasm_query(addr.parse()?, msg).await })?;

        debug!("{:?}", res.res);

        Ok(res)
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
    pub fn migrate<S, T>(
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
        let contract_name = contract_name.into();
        let op_name = op_name.into();

        let addr = self.contract_map.address(&contract_name)?;

        let res = tokio_block(async {
            self.client
                .wasm_migrate(
                    MigrateRequest {
                        address: addr.parse()?,
                        new_code_id,
                        msg,
                    },
                    key,
                    &self.tx_options,
                )
                .await
        })?;

        self.contract_map
            .register_contract(&contract_name, new_code_id);

        if let Some(p) = &mut self.gas_profiler {
            p.instrument(
                contract_name,
                op_name,
                CommandType::Migrate,
                &res.res,
                Location::caller(),
            );
        }

        debug!("{:?}", res.res);

        Ok(res)
    }

    /// Blocks the current thread until `n` blocks have been processed.
    /// # Arguments
    /// * `n` - Wait for this number of blocks to process
    /// * `timeout` - Throws `PollBlockError` once `timeout` has elapsed.
    /// * `is_first_block` - Set to true if waiting for the first block to process for new test nodes.
    pub fn poll_for_n_blocks<T: Into<Duration> + Send>(
        &self,
        n: u64,
        timeout: T,
        is_first_block: bool,
    ) -> Result<(), PollBlockError> {
        tokio_block(async {
            _timeout(timeout.into(), async {
                if is_first_block {
                    while let Err(e) = self.client.tendermint_query_latest_block().await {
                        if !matches!(e, TendermintError::ChainError { .. }) {
                            return Err(PollBlockError::TendermintError(e));
                        }
                        time::sleep(Duration::from_millis(500)).await;
                    }
                }

                let mut curr_height = self
                    .client
                    .tendermint_query_latest_block()
                    .await?
                    .block
                    .header
                    .unwrap()
                    .height as u64;

                let target_height = curr_height + n;

                while curr_height < target_height {
                    time::sleep(Duration::from_millis(500)).await;

                    curr_height = self
                        .client
                        .tendermint_query_latest_block()
                        .await?
                        .block
                        .header
                        .unwrap()
                        .height as u64;
                }

                Ok(())
            })
            .await
        })??;

        Ok(())
    }

    /// Get gas usage report
    pub fn gas_profiler_report(&self) -> Option<&Report> {
        self.gas_profiler.as_ref().map(|p| p.report())
    }
}

pub(crate) fn tokio_block<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[cfg(test)]
mod tests {
    use super::CosmOrc;
    use crate::orchestrator::deploy::DeployInfo;
    use crate::orchestrator::gas_profiler::GasProfiler;
    use crate::orchestrator::{
        deploy::ContractMap,
        error::{ContractMapError, ProcessError, StoreError},
    };
    use assert_matches::assert_matches;
    use cosm_tome::chain::error::ChainError;
    use cosm_tome::chain::fee::GasInfo;
    use cosm_tome::chain::request::TxOptions;
    use cosm_tome::chain::response::{ChainResponse, ChainTxResponse, Code, Event, Tag};
    use cosm_tome::clients::client::{CosmTome, MockCosmosClient};
    use cosm_tome::config::cfg::ChainConfig;
    use cosm_tome::modules::auth::error::AccountError;
    use cosm_tome::modules::cosmwasm::error::CosmwasmError;
    use cosm_tome::modules::tx::error::TxError;
    use cosm_tome::signing_key::key::SigningKey;
    use cosmos_sdk_proto::cosmos::auth::v1beta1::{
        BaseAccount, QueryAccountRequest, QueryAccountResponse,
    };
    use cosmos_sdk_proto::cosmwasm::wasm::v1::{
        QuerySmartContractStateRequest, QuerySmartContractStateResponse,
    };
    use cosmos_sdk_proto::traits::MessageExt;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::vec;

    #[derive(Serialize)]
    pub struct TestMsg {}

    pub fn test_cfg() -> ChainConfig {
        ChainConfig {
            denom: "utest".to_string(),
            prefix: "test".to_string(),
            chain_id: "test-1".to_string(),
            rpc_endpoint: None,
            grpc_endpoint: Some("localhost:12690".to_string()),
            gas_prices: 0.1,
            gas_adjustment: 1.5,
        }
    }

    #[test]
    fn instantiate_not_stored() {
        let cfg = test_cfg();
        let code_ids = HashMap::new();
        let key = SigningKey::random_mnemonic("test".to_string());

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.instantiate("cw_not_stored", "i_test", &TestMsg {}, &key, None, vec![]);

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

    #[test]
    fn instantiate_cosmossdk_error() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, _| {
                Err(ChainError::CosmosSdk {
                    res: ChainResponse {
                        code: Code::Err(1),
                        data: None,
                        log: "error".to_string(),
                    },
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.instantiate("cw_test", "i_test", msg, &key, None, vec![]);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::CosmwasmError(CosmwasmError::TxError(TxError::AccountError(
                AccountError::ChainError(ChainError::CosmosSdk { .. })
            )))
        );

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn instantiate() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: None,
                        log: "log log log".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, None);
        assert_eq!(res.res.log, "log log log".to_string());
        assert_eq!(res.height, 1234);
        assert_eq!(res.tx_hash, "TX_HASH_0".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn instantiate_with_profiler() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: None,
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, mock_client),
            gas_profiler: Some(GasProfiler::new()),
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, None);
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);
        assert_eq!(res.height, 1234);
        assert_eq!(res.tx_hash, "TX_HASH_0".to_string());

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
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

    #[test]
    fn execute_not_stored() {
        let cfg = test_cfg();
        let code_ids = HashMap::new();
        let key = SigningKey::random_mnemonic("test".to_string());

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.execute("cw_not_stored", "e_test", &TestMsg {}, &key, vec![]);

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

    #[test]
    fn execute_not_initialized() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_not_init".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg, MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc.execute("cw_not_init", "e_test", &TestMsg {}, &key, vec![]);

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

    #[test]
    fn execute_cosmossdk_error() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, _| {
                Err(ChainError::CosmosSdk {
                    res: ChainResponse {
                        code: Code::Err(1),
                        data: None,
                        log: "error".to_string(),
                    },
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: cosm_orc.contract_map,
            client: CosmTome::new(cfg, mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.execute("cw_test", "e_test", msg, &key, vec![]);

        assert_matches!(
            res.unwrap_err(),
            ProcessError::CosmwasmError(CosmwasmError::TxError(TxError::AccountError(
                AccountError::ChainError { .. }
            )))
        );

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn execute() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(2)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(2).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(2)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );
        assert_eq!(cosm_orc.gas_profiler_report(), None);

        let res = cosm_orc
            .execute("cw_test", "e_test", msg, &key, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn execute_with_profiler() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(2)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(2).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(2)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: Some(GasProfiler::new()),
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
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
            .unwrap()
            .res;
        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        let report = cosm_orc.gas_profiler_report().unwrap();
        assert_eq!(report.keys().len(), 1);
        assert_eq!(report.get("cw_test").unwrap().keys().len(), 2);

        let r = report
            .get("cw_test")
            .unwrap()
            .get("Execute__e_test")
            .unwrap();
        assert_eq!(r.gas_used, 100);
        assert_eq!(r.gas_wanted, 101);
    }

    #[test]
    fn query_not_stored() {
        let cfg = test_cfg();
        let cosm_orc = CosmOrc {
            contract_map: ContractMap::new(HashMap::new()),
            client: CosmTome::new(cfg.clone(), MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.query("cw_not_stored", &TestMsg {});

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

    #[test]
    fn query_not_initialized() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_not_init".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);

        let cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_not_init").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_not_init".to_string()
            }
        );

        let res = cosm_orc.query("cw_not_init", &TestMsg {});

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

    #[test]
    fn query_cosmossdk_error() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        mock_client
            .expect_query::<QuerySmartContractStateRequest, QuerySmartContractStateResponse>()
            .times(1)
            .returning(move |_, _| {
                Err(ChainError::CosmosSdk {
                    res: ChainResponse {
                        code: Code::Err(1),
                        data: None,
                        log: "error".to_string(),
                    },
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );

        let res = cosm_orc.query("cw_test", msg);

        assert_matches!(res.unwrap_err(), ProcessError::CosmwasmError(..));

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn query() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: None,
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        mock_client
            .expect_query::<QuerySmartContractStateRequest, QuerySmartContractStateResponse>()
            .times(1)
            .returning(move |_, _| Ok(QuerySmartContractStateResponse { data: vec![] }));

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap_err(),
            ContractMapError::NotDeployed {
                name: "cw_test".to_string()
            }
        );

        let res = cosm_orc
            .instantiate("cw_test", "i_test", msg, &key, None, vec![])
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );
        assert_eq!(cosm_orc.gas_profiler_report(), None);

        let res = cosm_orc.query("cw_test", msg).unwrap();
        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }

    #[test]
    fn store_invalid_wasm_dir() {
        let cfg = test_cfg();
        let code_ids = HashMap::new();
        let key = SigningKey::random_mnemonic("test".to_string());

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), MockCosmosClient::new()),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc.store_contracts("invalid_dir", &key, None);
        assert_matches!(res.unwrap_err(), StoreError::WasmDirRead { .. });
    }

    #[test]
    fn migrate() {
        let cfg = test_cfg();
        let code_ids = HashMap::from([(
            "cw_test".to_string(),
            DeployInfo {
                code_id: Some(1337),
                address: Some("juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()),
            },
        )]);
        let key = SigningKey::random_mnemonic("test".to_string());

        let msg = &TestMsg {};

        let mut mock_client = MockCosmosClient::new();

        let new_code_id = 1338;

        mock_client
            .expect_query::<QueryAccountRequest, QueryAccountResponse>()
            .times(1)
            .returning(move |_, t: &str| {
                Ok(QueryAccountResponse {
                    account: Some(cosmos_sdk_proto::Any {
                        type_url: t.to_owned(),
                        value: BaseAccount {
                            address: "juno10j9gpw9t4jsz47qgnkvl5n3zlm2fz72k67rxsg".to_string(),
                            pub_key: None,
                            account_number: 1221,
                            sequence: 1,
                        }
                        .to_bytes()
                        .unwrap(),
                    }),
                })
            });

        mock_client.expect_simulate_tx().times(1).returning(|_| {
            Ok(GasInfo {
                gas_wanted: 200u16.into(),
                gas_used: 100u16.into(),
            })
        });

        mock_client
            .expect_broadcast_tx_block()
            .times(1)
            .returning(|_| {
                Ok(ChainTxResponse {
                    res: ChainResponse {
                        code: Code::Ok,
                        data: Some(vec![]),
                        log: "".to_string(),
                    },
                    events: vec![Event {
                        type_str: "instantiate".to_string(),
                        attributes: vec![Tag {
                            key: "_contract_address".to_string(),
                            value: "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string(),
                        }],
                    }],
                    gas_wanted: 101,
                    gas_used: 100,
                    tx_hash: "TX_HASH_0".to_string(),
                    height: 1234,
                })
            });

        let mut cosm_orc = CosmOrc {
            contract_map: ContractMap::new(code_ids),
            client: CosmTome::new(cfg.clone(), mock_client),
            gas_profiler: None,
            tx_options: TxOptions::default(),
        };

        let res = cosm_orc
            .migrate("cw_test", new_code_id, "migrate_op", msg, &key)
            .unwrap()
            .res;

        assert_eq!(res.res.code, Code::Ok);
        assert_eq!(res.res.data, Some(vec![]));
        assert_eq!(res.res.log, "".to_string());
        assert_eq!(res.gas_used, 100);
        assert_eq!(res.gas_wanted, 101);

        assert_eq!(
            cosm_orc.contract_map.address("cw_test").unwrap(),
            "juno1ft5zfffrgtm2u72cup9e2ecfxjwz8ztc929cgj".to_string()
        );

        // code_id is the newly migrated id:
        assert_eq!(cosm_orc.contract_map.code_id("cw_test").unwrap(), 1338);

        assert_eq!(cosm_orc.gas_profiler_report(), None);
    }
}
