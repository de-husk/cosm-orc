use anyhow::{Context, Result};
use backtrace::Backtrace;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::orchestrator::command::CommandType;
use crate::profilers::profiler::{Profiler, Report};
use crate::util::key_str::process_backtrace;

#[derive(Debug, Serialize, Deserialize)]
pub struct GasProfiler {
    report: HashMap<String, HashMap<String, GasReport>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GasReport {
    gas_wanted: u64,
    gas_used: u64,
    file_name: String,
    line_number: u64,
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
        output_json: &Value,
        backtrace: &Backtrace,
        msg_idx: usize,
    ) -> Result<()> {
        if op_type == CommandType::Query {
            // Wasm Query msgs don't cost gas
            return Ok(());
        }

        let (caller_file_name, caller_line_number) = match process_backtrace(backtrace) {
            Some(frame) => frame,
            None => ("unknown_file".to_string(), 0),
        };

        let op_key = format!(
            "{}__{}:{}[{}]",
            op_name, caller_file_name, caller_line_number, msg_idx
        );

        let m = self.report.entry(contract).or_default();
        m.insert(
            op_key,
            GasReport {
                gas_used: output_json["gas_used"]
                    .as_str()
                    .context("not string")?
                    .parse()?,
                gas_wanted: output_json["gas_wanted"]
                    .as_str()
                    .context("not string")?
                    .parse()?,
                file_name: caller_file_name,
                line_number: caller_line_number,
            },
        );

        Ok(())
    }

    fn report(&self) -> Result<Report> {
        let json = serde_json::to_vec(&self.report)?;
        Ok(Report {
            name: "gas-profiler".to_string(),
            json_data: json,
        })
    }
}
