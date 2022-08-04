use super::error::ClientError;
use crate::config::cfg::ChainCfg;
use cosmos_sdk_proto::cosmos::auth::v1beta1::{
    BaseAccount, QueryAccountRequest, QueryAccountResponse,
};
use cosmos_sdk_proto::cosmos::tx::v1beta1::{SimulateRequest, SimulateResponse};
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    QuerySmartContractStateRequest, QuerySmartContractStateResponse,
};
use cosmrs::cosmwasm::{MsgExecuteContract, MsgInstantiateContract};
use cosmrs::rpc::endpoint::broadcast::tx_commit::{Response, TxResult};
use cosmrs::rpc::Client;
use cosmrs::tendermint::abci::tag::Key;
use cosmrs::tendermint::abci::{Code, Event};
use cosmrs::tx::{Fee, Msg, SignDoc, SignerInfo};
use cosmrs::{
    cosmwasm::MsgStoreCode,
    crypto::secp256k1::SigningKey,
    rpc::HttpClient,
    tx::{self},
};
use cosmrs::{AccountId, Any, Coin, Denom};
use prost::Message;
use std::future::Future;
use std::str::FromStr;
use tendermint_rpc::endpoint::abci_query::AbciQuery;

pub struct CosmClient {
    client: HttpClient,
    cfg: ChainCfg,
}

impl CosmClient {
    pub fn new(cfg: ChainCfg) -> Result<Self, ClientError> {
        Ok(Self {
            client: HttpClient::new(cfg.rpc_endpoint.as_str())?,
            cfg,
        })
    }

    pub async fn store(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
    ) -> Result<StoreCodeResponse, ClientError> {
        let signing_public_key = signing_key.public_key();

        let sender_account_id = signing_public_key
            .account_id(&self.cfg.prefix)
            .map_err(ClientError::crypto)?;

        let msg = MsgStoreCode {
            sender: sender_account_id.clone(),
            wasm_byte_code: payload,
            instantiate_permission: None,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, signing_key, sender_account_id).await?;

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
            data: tx_res.deliver_tx,
        })
    }

    pub async fn instantiate(
        &self,
        code_id: u64,
        payload: Vec<u8>,
        signing_key: &SigningKey,
    ) -> Result<InstantiateResponse, ClientError> {
        let signing_public_key = signing_key.public_key();
        let sender_account_id = signing_public_key
            .account_id(&self.cfg.prefix)
            .map_err(ClientError::crypto)?;

        let msg = MsgInstantiateContract {
            sender: sender_account_id.clone(),
            admin: None, // TODO
            code_id,
            label: Some("cosm-orc".to_string()),
            msg: payload,
            funds: vec![], // TODO
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, signing_key, sender_account_id).await?;

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
            data: tx_res.deliver_tx,
        })
    }

    pub async fn execute(
        &self,
        address: String,
        payload: Vec<u8>,
        signing_key: &SigningKey,
    ) -> Result<ExecResponse, ClientError> {
        let signing_public_key = signing_key.public_key();
        let sender_account_id = signing_public_key
            .account_id(&self.cfg.prefix)
            .map_err(ClientError::crypto)?;

        let msg = MsgExecuteContract {
            sender: sender_account_id.clone(),
            contract: address.parse().unwrap(),
            msg: payload,
            funds: vec![], // TODO
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = self.send_tx(msg, signing_key, sender_account_id).await?;

        Ok(ExecResponse {
            data: tx_res.deliver_tx,
        })
    }

    pub async fn query(
        &self,
        address: String,
        payload: Vec<u8>,
    ) -> Result<QueryResponse, ClientError> {
        let res = self
            .abci_query(
                QuerySmartContractStateRequest {
                    address: address.parse().unwrap(),
                    query_data: payload,
                },
                "/cosmwasm.wasm.v1.Query/SmartContractState",
            )
            .await?;

        let res = QuerySmartContractStateResponse::decode(res.value.as_slice())
            .map_err(ClientError::prost_proto_de)?;

        // TODO: I shouldnt expose TxResult from this file, I should make my own type instead of re-exporting too
        //  * also Query is not a tx so this doesnt really make sense to conform to this type
        Ok(QueryResponse {
            data: TxResult {
                code: Code::Ok,
                data: Some(res.data.into()),
                ..Default::default()
            },
        })
    }

    async fn send_tx(
        &self,
        msg: Any,
        key: &SigningKey,
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
            .broadcast_commit(&self.client)
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

    async fn abci_query<T: Message>(&self, req: T, path: &str) -> Result<AbciQuery, ClientError> {
        let mut buf = Vec::with_capacity(req.encoded_len());
        req.encode(&mut buf).map_err(ClientError::prost_proto_en)?;

        let res = self
            .client
            .abci_query(Some(path.parse().unwrap()), buf, None, false)
            .await?;

        if res.code != Code::Ok {
            return Err(ClientError::CosmosSdk { res: res.into() });
        }

        Ok(res)
    }

    async fn account(&self, account_id: AccountId) -> Result<BaseAccount, ClientError> {
        let res = self
            .abci_query(
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
        key: &SigningKey,
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

        let res = self
            .abci_query(
                SimulateRequest {
                    tx: None,
                    tx_bytes: tx_raw.to_bytes().map_err(ClientError::proto_encoding)?,
                },
                "/cosmos.tx.v1beta1.Service/Simulate",
            )
            .await?;

        let gas_info = SimulateResponse::decode(res.value.as_slice())
            .map_err(ClientError::prost_proto_de)?
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

pub fn tokio_block<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[derive(Debug)]
pub struct StoreCodeResponse {
    pub code_id: u64,
    pub data: TxResult,
}

#[derive(Debug)]
pub struct InstantiateResponse {
    pub address: String,
    pub data: TxResult,
}

#[derive(Debug)]
pub struct ExecResponse {
    pub data: TxResult,
}

#[derive(Debug)]
pub struct QueryResponse {
    pub data: TxResult,
}
