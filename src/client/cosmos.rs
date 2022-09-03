use super::chain_res::ChainResponse;
use super::error::ClientError;
use crate::config::cfg::ChainCfg;
use cosmos_sdk_proto::cosmos::auth::v1beta1::{
    BaseAccount, QueryAccountRequest, QueryAccountResponse,
};
use cosmos_sdk_proto::cosmos::tx::v1beta1::service_client::ServiceClient;
use cosmos_sdk_proto::cosmos::tx::v1beta1::SimulateRequest;
use cosmrs::crypto::secp256k1;
use cosmrs::rpc::endpoint::broadcast::tx_commit::Response;
use cosmrs::rpc::Client;
use cosmrs::tendermint::abci::{Code, Event};
use cosmrs::tx::{Fee, SignDoc, SignerInfo};
use cosmrs::{
    rpc::HttpClient,
    tx::{self},
};
use cosmrs::{AccountId, Any, Coin, Denom};
use prost::Message;
use tendermint_rpc::endpoint::abci_query::AbciQuery;

pub async fn send_tx(
    client: &HttpClient,
    msg: Any,
    key: &secp256k1::SigningKey,
    account_id: AccountId,
    cfg: &ChainCfg,
) -> Result<Response, ClientError> {
    let timeout_height = 0u16; // TODO
    let account = account(client, account_id).await?;

    let tx_body = tx::Body::new(vec![msg], "MEMO", timeout_height);

    let fee = simulate_gas_fee(&tx_body, &account, key, cfg).await?;

    // NOTE: if we are making requests in parallel with the same key, we need to serialize `account.sequence` to avoid errors
    let auth_info =
        SignerInfo::single_direct(Some(key.public_key()), account.sequence).auth_info(fee);

    let sign_doc = SignDoc::new(
        &tx_body,
        &auth_info,
        &cfg.chain_id.parse().map_err(|_| ClientError::ChainId {
            chain_id: cfg.chain_id.to_string(),
        })?,
        account.account_number,
    )
    .map_err(ClientError::proto_encoding)?;

    let tx_raw = sign_doc.sign(key).map_err(ClientError::crypto)?;

    let tx_commit_response = tx_raw
        .broadcast_commit(client)
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

async fn account(client: &HttpClient, account_id: AccountId) -> Result<BaseAccount, ClientError> {
    let res = abci_query(
        client,
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
    tx: &tx::Body,
    account: &BaseAccount,
    key: &secp256k1::SigningKey,
    cfg: &ChainCfg,
) -> Result<Fee, ClientError> {
    // TODO: support passing in the exact fee too (should be on a per process_msg() call)
    let denom: Denom = cfg.denom.parse().map_err(|_| ClientError::Denom {
        name: cfg.denom.clone(),
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
        &cfg.chain_id.parse().map_err(|_| ClientError::ChainId {
            chain_id: cfg.chain_id.to_string(),
        })?,
        account.account_number,
    )
    .map_err(ClientError::proto_encoding)?;

    let tx_raw = sign_doc.sign(key).map_err(ClientError::crypto)?;

    let mut client = ServiceClient::connect(cfg.grpc_endpoint.clone()).await?;

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

    let gas_limit = (gas_info.gas_used as f64 * cfg.gas_adjustment).ceil();
    let amount = Coin {
        denom: denom.clone(),
        amount: ((gas_limit * cfg.gas_prices).ceil() as u64).into(),
    };

    Ok(Fee::from_amount_and_gas(amount, gas_limit as u64))
}

pub fn find_event(res: &Response, key_name: &str) -> Option<Event> {
    for event in &res.deliver_tx.events {
        if event.type_str == key_name {
            return Some(event.clone());
        }
    }
    None
}
