use serde::{Deserialize, Serialize};
use std::panic::Location;
use std::{collections::HashMap, error::Error};

use crate::client::cosm_client::TendermintRes;
use crate::profilers::profiler::{Profiler, Report};

use super::profiler::CommandType;

#[derive(Debug, Serialize, Deserialize)]
pub struct GasProfiler {
    report: HashMap<String, HashMap<String, GasReport>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct GasReport {
    pub gas_wanted: u64,
    pub gas_used: u64,
    pub file_name: String,
    pub line_number: u32,
}

impl GasProfiler {
    pub fn new() -> Self {
        Self {
            report: HashMap::new(),
        }
    }
}

impl Default for GasProfiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler for GasProfiler {
    fn instrument(
        &mut self,
        contract: String,
        op_name: String,
        op_type: CommandType,
        response: &TendermintRes,
        caller_loc: &Location,
    ) -> Result<(), Box<dyn Error>> {
        if op_type == CommandType::Query {
            // Wasm Query msgs don't cost gas
            return Ok(());
        }

        let caller_file_name = caller_loc.file().to_string();
        let caller_line_number = caller_loc.line();
        let op_key = format!("{:?}__{}", op_type, op_name);

        let m = self.report.entry(contract).or_default();
        m.insert(
            op_key,
            GasReport {
                gas_used: response.gas_used,
                gas_wanted: response.gas_wanted,
                file_name: caller_file_name,
                line_number: caller_line_number,
            },
        );

        Ok(())
    }

    fn report(&self) -> Result<Report, Box<dyn Error>> {
        let json = serde_json::to_vec(&self.report)?;
        Ok(Report {
            name: "gas-profiler".to_string(),
            json_data: json,
        })
    }
}
