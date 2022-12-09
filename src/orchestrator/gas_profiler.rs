use cosm_tome::chain::response::ChainTxResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::panic::Location;

#[derive(PartialEq, Eq, Debug)]
pub enum CommandType {
    Store,
    Instantiate,
    Query,
    Execute,
    Migrate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GasProfiler {
    report: Report,
}

pub type Report = HashMap<String, HashMap<String, GasReport>>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GasReport {
    pub gas_wanted: u64,
    pub gas_used: u64,
    pub file_name: String,
    pub line_number: u32,
}

impl Default for GasProfiler {
    fn default() -> Self {
        Self::new()
    }
}

impl GasProfiler {
    pub fn new() -> Self {
        Self {
            report: HashMap::new(),
        }
    }

    pub fn instrument(
        &mut self,
        contract: String,
        op_name: String,
        op_type: CommandType,
        response: &ChainTxResponse,
        caller_loc: &Location,
    ) {
        if op_type == CommandType::Query {
            // Wasm Query msgs don't cost gas
            return;
        }

        let caller_file_name = caller_loc.file().to_string();
        let caller_line_number = caller_loc.line();
        let op_key = format!("{op_type:?}__{op_name}");

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
    }

    pub fn report(&self) -> &Report {
        &self.report
    }
}
