use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::orchestrator::command::CommandType;

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub name: String,
    pub json_data: Vec<u8>,
}

pub trait Profiler {
    fn instrument(&mut self, op_name: String, op_type: CommandType, json: &Value) -> Result<()>;
    fn report(&self) -> Result<Report>;
}
