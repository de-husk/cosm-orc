use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::panic::Location;

use crate::orchestrator::command::CommandType;

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
        output_json: &Value,
        caller_loc: &Location,
        msg_idx: usize,
    ) -> Result<()>;
    fn report(&self) -> Result<Report>;
}
