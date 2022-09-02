use super::error::{ClientError, DeserializeError};
use crate::config::cfg::ChainCfg;
use crate::config::key::SigningKey;
use cosmos_sdk_proto::cosmos::auth::v1beta1::{
    BaseAccount, QueryAccountRequest, QueryAccountResponse,
};
use cosmos_sdk_proto::cosmos::tx::v1beta1::service_client::ServiceClient;
use cosmos_sdk_proto::cosmos::tx::v1beta1::SimulateRequest;
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    QuerySmartContractStateRequest, QuerySmartContractStateResponse,
};
use cosmrs::cosmwasm::{MsgExecuteContract, MsgInstantiateContract};
use cosmrs::crypto::secp256k1;
use cosmrs::rpc::endpoint::broadcast::tx_commit::{Response, TxResult};
use cosmrs::rpc::Client;
use cosmrs::tendermint::abci::tag::Key;
use cosmrs::tendermint::abci::{Code, Event};
use cosmrs::tx::{Fee, Msg, SignDoc, SignerInfo};
use cosmrs::{
    cosmwasm::MsgStoreCode,
    rpc::HttpClient,
    tx::{self},
};
use cosmrs::{AccountId, Any, Coin, Denom};
use prost::Message;
use serde::Deserialize;
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;
use tendermint_rpc::endpoint::abci_query::AbciQuery;
use tokio::time;

#[cfg_attr(test, faux::create)]
#[derive(Clone, Debug)]
pub struct CosmClient {
    // http tendermint RPC client
    rpc_client: HttpClient,
    cfg: ChainCfg,
}

#[cfg_attr(test, faux::methods)]
impl CosmClient {
    // HACK: faux doesn't support mocking a struct wrapped in a Result
    // so we are just ignoring the constructor for this crate's tests
    #[cfg(not(test))]
    pub fn new(cfg: ChainCfg) -> Result<Self, ClientError> {
        Ok(Self {
            rpc_client: HttpClient::new(cfg.rpc_endpoint.as_str())?,
            cfg,
        })
    }

    pub async fn store(
        &self,
        payload: Vec<u8>,
        key: &SigningKey,
    ) -> Result<StoreCodeResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let msg = MsgStoreCode {
            sender: account_id.clone(),
            wasm_byte_code: payload,
            instantiate_permission: None,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, &signing_key, account_id).await?;

        let res = self.find_event(&tx_res, "store_code").unwrap();

        let code_id = res
            .attributes
            .iter()
            .find(|a| a.key == Key::from_str("code_id").unwrap())
            .unwrap()
            .value
            .as_ref()
            .parse::<u64>()
            .unwrap();

        Ok(StoreCodeResponse {
            code_id,
            res: tx_res.deliver_tx.into(),
        })
    }

    pub async fn instantiate(
        &self,
        code_id: u64,
        payload: Vec<u8>,
        key: &SigningKey,
    ) -> Result<InstantiateResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let msg = MsgInstantiateContract {
            sender: account_id.clone(),
            admin: None, // TODO
            code_id,
            label: Some("cosm-orc".to_string()),
            msg: payload,
            funds: vec![], // TODO
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, &signing_key, account_id).await?;

        let res = self.find_event(&tx_res, "instantiate").unwrap();

        let addr = res
            .attributes
            .iter()
            .find(|a| a.key == Key::from_str("_contract_address").unwrap())
            .unwrap()
            .value
            .to_string();

        Ok(InstantiateResponse {
            address: addr,
            res: tx_res.deliver_tx.into(),
        })
    }

    pub async fn execute(
        &self,
        address: String,
        payload: Vec<u8>,
        key: &SigningKey,
    ) -> Result<ExecResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let msg = MsgExecuteContract {
            sender: account_id.clone(),
            contract: address.parse().unwrap(),
            msg: payload,
            funds: vec![], // TODO
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, &signing_key, account_id).await?;

        Ok(ExecResponse {
            res: tx_res.deliver_tx.into(),
        })
    }

    pub async fn query(
        &self,
        address: String,
        payload: Vec<u8>,
    ) -> Result<QueryResponse, ClientError> {
        let res = abci_query(
            &self.rpc_client,
            QuerySmartContractStateRequest {
                address: address.parse().unwrap(),
                query_data: payload,
            },
            "/cosmwasm.wasm.v1.Query/SmartContractState",
        )
        .await?;

        let res = QuerySmartContractStateResponse::decode(res.value.as_slice())
            .map_err(ClientError::prost_proto_de)?;

        Ok(QueryResponse { res: res.into() })
    }

    pub async fn poll_for_n_blocks(&self, n: u64, is_first_block: bool) -> Result<(), ClientError> {
        if is_first_block {
            self.rpc_client
                .wait_until_healthy(Duration::from_secs(5))
                .await?;

            while let Err(e) = self.rpc_client.latest_block().await {
                if !matches!(e.detail(), cosmrs::rpc::error::ErrorDetail::Serde(_)) {
                    return Err(e.into());
                }
                time::sleep(Duration::from_millis(500)).await;
            }
        }

        let mut curr_height: u64 = self
            .rpc_client
            .latest_block()
            .await?
            .block
            .header
            .height
            .into();
        let target_height: u64 = curr_height + n;

        while curr_height < target_height {
            time::sleep(Duration::from_millis(500)).await;

            curr_height = self
                .rpc_client
                .latest_block()
                .await?
                .block
                .header
                .height
                .into();
        }

        Ok(())
    }

    async fn send_tx(
        &self,
        msg: Any,
        key: &secp256k1::SigningKey,
        account_id: AccountId,
    ) -> Result<Response, ClientError> {
        let timeout_height = 0u16; // TODO
        let account = self.account(account_id).await?;

        let tx_body = tx::Body::new(vec![msg], "MEMO", timeout_height);

        let fee = self.simulate_gas_fee(&tx_body, &account, key).await?;

        // NOTE: if we are making requests in parallel with the same key, we need to serialize `account.sequence` to avoid errors
        let auth_info =
            SignerInfo::single_direct(Some(key.public_key()), account.sequence).auth_info(fee);

        let sign_doc = SignDoc::new(
            &tx_body,
            &auth_info,
            &self
                .cfg
                .chain_id
                .parse()
                .map_err(|_| ClientError::ChainId {
                    chain_id: self.cfg.chain_id.to_string(),
                })?,
            account.account_number,
        )
        .map_err(ClientError::proto_encoding)?;

        let tx_raw = sign_doc.sign(key).map_err(ClientError::crypto)?;

        let tx_commit_response = tx_raw
            .broadcast_commit(&self.rpc_client)
            .await
            .map_err(ClientError::proto_encoding)?;

        if tx_commit_response.check_tx.code.is_err() {
            return Err(ClientError::CosmosSdk {
                res: tx_commit_response.check_tx.into(),
            });
        }
        if tx_commit_response.deliver_tx.code.is_err() {
            return Err(ClientError::CosmosSdk {
                res: tx_commit_response.deliver_tx.into(),
            });
        }

        Ok(tx_commit_response)
    }

    async fn account(&self, account_id: AccountId) -> Result<BaseAccount, ClientError> {
        let res = abci_query(
            &self.rpc_client,
            QueryAccountRequest {
                address: account_id.as_ref().into(),
            },
            "/cosmos.auth.v1beta1.Query/Account",
        )
        .await?;

        let res = QueryAccountResponse::decode(res.value.as_slice())
            .map_err(ClientError::prost_proto_de)?
            .account
            .ok_or(ClientError::AccountId {
                id: account_id.to_string(),
            })?;

        let base_account =
            BaseAccount::decode(res.value.as_slice()).map_err(ClientError::prost_proto_de)?;

        Ok(base_account)
    }

    #[allow(deprecated)]
    async fn simulate_gas_fee(
        &self,
        tx: &tx::Body,
        account: &BaseAccount,
        key: &secp256k1::SigningKey,
    ) -> Result<Fee, ClientError> {
        // TODO: support passing in the exact fee too (should be on a per process_msg() call)
        let denom: Denom = self.cfg.denom.parse().map_err(|_| ClientError::Denom {
            name: self.cfg.denom.clone(),
        })?;

        let signer_info = SignerInfo::single_direct(Some(key.public_key()), account.sequence);
        let auth_info = signer_info.auth_info(Fee::from_amount_and_gas(
            Coin {
                denom: denom.clone(),
                amount: 0u64.into(),
            },
            0u64,
        ));

        let sign_doc = SignDoc::new(
            tx,
            &auth_info,
            &self
                .cfg
                .chain_id
                .parse()
                .map_err(|_| ClientError::ChainId {
                    chain_id: self.cfg.chain_id.to_string(),
                })?,
            account.account_number,
        )
        .map_err(ClientError::proto_encoding)?;

        let tx_raw = sign_doc.sign(key).map_err(ClientError::crypto)?;

        let mut client = ServiceClient::connect(self.cfg.grpc_endpoint.clone()).await?;

        let gas_info = client
            .simulate(SimulateRequest {
                tx: None,
                tx_bytes: tx_raw.to_bytes().map_err(ClientError::proto_encoding)?,
            })
            .await
            .map_err(|e| ClientError::CosmosSdk {
                res: ChainResponse {
                    code: Code::Err(e.code() as u32),
                    log: e.message().to_string(),
                    ..Default::default()
                },
            })?
            .into_inner()
            .gas_info
            .unwrap();

        let gas_limit = (gas_info.gas_used as f64 * self.cfg.gas_adjustment).ceil();
        let amount = Coin {
            denom: denom.clone(),
            amount: ((gas_limit * self.cfg.gas_prices).ceil() as u64).into(),
        };

        Ok(Fee::from_amount_and_gas(amount, gas_limit as u64))
    }

    fn find_event(&self, res: &Response, key_name: &str) -> Option<Event> {
        for event in &res.deliver_tx.events {
            if event.type_str == key_name {
                return Some(event.clone());
            }
        }
        None
    }
}

pub async fn abci_query<T: Message>(
    client: &HttpClient,
    req: T,
    path: &str,
) -> Result<AbciQuery, ClientError> {
    let mut buf = Vec::with_capacity(req.encoded_len());
    req.encode(&mut buf).map_err(ClientError::prost_proto_en)?;

    let res = client
        .abci_query(Some(path.parse().unwrap()), buf, None, false)
        .await?;

    if res.code != Code::Ok {
        return Err(ClientError::CosmosSdk { res: res.into() });
    }

    Ok(res)
}

pub fn tokio_block<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[derive(Debug, Default)]
pub struct ChainResponse {
    pub code: Code,
    pub data: Option<Vec<u8>>,
    pub log: String,
    pub gas_wanted: u64,
    pub gas_used: u64,
}

impl From<TxResult> for ChainResponse {
    fn from(res: TxResult) -> ChainResponse {
        ChainResponse {
            code: res.code,
            data: res.data.map(|d| d.into()),
            log: res.log.to_string(),
            gas_wanted: res.gas_wanted.into(),
            gas_used: res.gas_used.into(),
        }
    }
}

impl From<AbciQuery> for ChainResponse {
    fn from(res: AbciQuery) -> ChainResponse {
        ChainResponse {
            code: res.code,
            data: Some(res.value),
            log: res.log.to_string(),
            gas_wanted: 0,
            gas_used: 0,
        }
    }
}

impl From<QuerySmartContractStateResponse> for ChainResponse {
    fn from(res: QuerySmartContractStateResponse) -> ChainResponse {
        ChainResponse {
            code: Code::Ok,
            data: Some(res.data),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub struct StoreCodeResponse {
    pub code_id: u64,
    pub res: ChainResponse,
}

#[derive(Debug)]
pub struct InstantiateResponse {
    pub address: String,
    pub res: ChainResponse,
}

#[derive(Debug)]
pub struct ExecResponse {
    pub res: ChainResponse,
}

#[derive(Debug)]
pub struct QueryResponse {
    pub res: ChainResponse,
}

impl ChainResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        let r: T = serde_json::from_slice(
            self.data
                .as_ref()
                .ok_or(DeserializeError::EmptyResponse)?
                .as_slice(),
        )?;
        Ok(r)
    }
}
