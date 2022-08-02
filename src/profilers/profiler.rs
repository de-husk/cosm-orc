use cosmrs::rpc::endpoint::broadcast::tx_commit::TxResult;
use serde::{Deserialize, Serialize};
use std::{error::Error, panic::Location};

use crate::orchestrator::cosm_orc::WasmMsg;

#[derive(PartialEq, Eq, Debug)]
pub enum CommandType {
    Store,
    Instantiate,
    Query,
    Execute,
}

impl<X, Y, Z> From<&WasmMsg<X, Y, Z>> for CommandType
where
    X: Serialize,
    Y: Serialize,
    Z: Serialize,
{
    fn from(msg: &WasmMsg<X, Y, Z>) -> CommandType {
        match msg {
            WasmMsg::InstantiateMsg(_) => CommandType::Instantiate,
            WasmMsg::ExecuteMsg(_) => CommandType::Execute,
            WasmMsg::QueryMsg(_) => CommandType::Query,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub name: String,
    pub json_data: Vec<u8>,
}

pub trait Profiler {
    fn instrument(
        &mut self,
        contract: String,
        op_name: String,
        op_type: CommandType,
        response: &TxResult,
        caller_loc: &Location,
        msg_idx: usize,
    ) -> Result<(), Box<dyn Error>>;
    fn report(&self) -> Result<Report, Box<dyn Error>>;
}
