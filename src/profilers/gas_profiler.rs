use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::orchestrator::command::CommandType;
use crate::profilers::profiler::{Profiler, Report};

#[derive(Debug, Serialize, Deserialize)]
pub struct GasProfiler {
    report: HashMap<String, GasReport>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GasReport {
    gas_wanted: u64,
    gas_used: u64,
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
    fn instrument(&mut self, op_name: String, op_type: CommandType, json: &Value) -> Result<()> {
        if op_type == CommandType::Query {
            // Wasm Query msgs don't cost gas
            return Ok(());
        }

        self.report.insert(
            op_name,
            GasReport {
                gas_used: json["gas_used"].as_str().context("not string")?.parse()?,
                gas_wanted: json["gas_wanted"].as_str().context("not string")?.parse()?,
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
