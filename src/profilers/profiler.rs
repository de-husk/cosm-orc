use cosmrs::rpc::endpoint::broadcast::tx_commit::TxResult;
use serde::{Deserialize, Serialize};
use std::{error::Error, panic::Location};

#[derive(PartialEq, Eq, Debug)]
pub enum CommandType {
    Store,
    Instantiate,
    Query,
    Execute,
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
    ) -> Result<(), Box<dyn Error>>;
    fn report(&self) -> Result<Report, Box<dyn Error>>;
}
