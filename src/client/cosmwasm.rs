use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    QuerySmartContractStateRequest, QuerySmartContractStateResponse,
};
use cosmrs::cosmwasm::{MsgExecuteContract, MsgInstantiateContract, MsgMigrateContract};
use cosmrs::crypto::secp256k1;
use cosmrs::rpc::Client;
use cosmrs::tendermint::abci::tag::Key;
use cosmrs::tx::Msg;
use cosmrs::{cosmwasm::MsgStoreCode, rpc::HttpClient};
use prost::Message;
use std::str::FromStr;
use std::time::Duration;
use tokio::time;

use super::chain_res::ChainResponse;
use super::cosmos::{abci_query, find_event, send_tx};
use super::error::ClientError;
use crate::config::cfg::{ChainCfg, Coin};
use crate::config::key::SigningKey;
use crate::orchestrator::AccessConfig;

#[cfg_attr(test, faux::create)]
#[derive(Clone, Debug)]
pub struct CosmWasmClient {
    // http tendermint RPC client
    rpc_client: HttpClient,
    cfg: ChainCfg,
}

#[cfg_attr(test, faux::methods)]
impl CosmWasmClient {
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
        instantiate_perms: Option<AccessConfig>,
    ) -> Result<StoreCodeResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let msg = MsgStoreCode {
            sender: account_id.clone(),
            wasm_byte_code: payload,
            instantiate_permission: instantiate_perms
                .map(|p| p.try_into())
                .transpose()
                .map_err(|e| ClientError::InstantiatePerms { source: e })?,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = send_tx(&self.rpc_client, msg, &signing_key, account_id, &self.cfg).await?;

        let res = find_event(&tx_res, "store_code").unwrap();

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
        admin: Option<String>,
        funds: Vec<Coin>,
    ) -> Result<InstantiateResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let mut cosm_funds = vec![];
        for fund in funds {
            cosm_funds.push(fund.try_into()?);
        }

        let msg = MsgInstantiateContract {
            sender: account_id.clone(),
            admin: admin
                .map(|s| s.parse())
                .transpose()
                .map_err(|_| ClientError::AdminAddress)?,
            code_id,
            label: Some("cosm-orc".to_string()),
            msg: payload,
            funds: cosm_funds,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = send_tx(&self.rpc_client, msg, &signing_key, account_id, &self.cfg).await?;

        let res = find_event(&tx_res, "instantiate").unwrap();

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
        funds: Vec<Coin>,
    ) -> Result<ExecResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let mut cosm_funds = vec![];
        for fund in funds {
            cosm_funds.push(fund.try_into()?);
        }

        let msg = MsgExecuteContract {
            sender: account_id.clone(),
            contract: address.parse().unwrap(),
            msg: payload,
            funds: cosm_funds,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = send_tx(&self.rpc_client, msg, &signing_key, account_id, &self.cfg).await?;

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

    pub async fn migrate(
        &self,
        address: String,
        new_code_id: u64,
        payload: Vec<u8>,
        key: &SigningKey,
    ) -> Result<MigrateResponse, ClientError> {
        let signing_key: secp256k1::SigningKey = key.try_into()?;
        let account_id = key.to_account(&self.cfg.prefix)?;

        let msg = MsgMigrateContract {
            sender: account_id.clone(),
            contract: address.parse().unwrap(),
            code_id: new_code_id,
            msg: payload,
        }
        .to_any()
        .map_err(ClientError::proto_encoding)?;

        let tx_res = send_tx(&self.rpc_client, msg, &signing_key, account_id, &self.cfg).await?;

        Ok(MigrateResponse {
            res: tx_res.deliver_tx.into(),
        })
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
}

#[derive(Clone, Debug)]
pub struct StoreCodeResponse {
    pub code_id: u64,
    pub res: ChainResponse,
}

#[derive(Clone, Debug)]
pub struct InstantiateResponse {
    pub address: String,
    pub res: ChainResponse,
}

#[derive(Clone, Debug)]
pub struct ExecResponse {
    pub res: ChainResponse,
}

#[derive(Clone, Debug)]
pub struct QueryResponse {
    pub res: ChainResponse,
}

#[derive(Clone, Debug)]
pub struct MigrateResponse {
    pub res: ChainResponse,
}
