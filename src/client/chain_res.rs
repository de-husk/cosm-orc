use super::error::DeserializeError;
use cosmos_sdk_proto::cosmwasm::wasm::v1::QuerySmartContractStateResponse;
use cosmrs::rpc::endpoint::broadcast::tx_commit::TxResult;
use cosmrs::tendermint::abci::Code;
use serde::Deserialize;
use tendermint_rpc::endpoint::abci_query::AbciQuery;

#[derive(Clone, Debug)]
pub struct StoreCodeResponse {
    pub code_id: u64,
    pub res: ChainResponse,
    pub tx_hash: String,
    pub height: u64,
}
impl StoreCodeResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        self.res.data()
    }
}

#[derive(Clone, Debug)]
pub struct InstantiateResponse {
    pub address: String,
    pub res: ChainResponse,
    pub tx_hash: String,
    pub height: u64,
}
impl InstantiateResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        self.res.data()
    }
}

#[derive(Clone, Debug)]
pub struct ExecResponse {
    pub res: ChainResponse,
    pub tx_hash: String,
    pub height: u64,
}
impl ExecResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        self.res.data()
    }
}

#[derive(Clone, Debug)]
pub struct QueryResponse {
    pub res: ChainResponse,
}
impl QueryResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        self.res.data()
    }
}

#[derive(Clone, Debug)]
pub struct MigrateResponse {
    pub res: ChainResponse,
    pub tx_hash: String,
    pub height: u64,
}
impl MigrateResponse {
    pub fn data<'a, T: Deserialize<'a>>(&'a self) -> Result<T, DeserializeError> {
        self.res.data()
    }
}

#[derive(Clone, Debug, Default)]
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
