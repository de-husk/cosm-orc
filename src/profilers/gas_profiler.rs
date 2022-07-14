use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::orchestrator::command::CommandType;
use crate::profilers::profiler::{Profiler, Report};
use crate::util::key_str::op_key;

#[derive(Debug, Serialize, Deserialize)]
pub struct GasProfiler {
    report: HashMap<String, HashMap<String, GasReport>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GasReport {
    gas_wanted: u64,
    gas_used: u64,
    payload: String,
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
        input_json: &Value,
        output_json: &Value,
    ) -> Result<()> {
        if op_type == CommandType::Query {
            // Wasm Query msgs don't cost gas
            return Ok(());
        }

        let op_key = if op_type == CommandType::Instantiate {
            // Instantiate messages are not enums like query and execute
            op_name
        } else {
            format!(
                "{}__{}",
                op_name,
                op_key(input_json).context("invalid json")?,
            )
        };

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
                payload: input_json.to_string(),
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
