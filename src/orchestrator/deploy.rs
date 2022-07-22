use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ContractName = String;

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractMap {
    map: HashMap<ContractName, DeployInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeployInfo {
    code_id: u64,
    address: Option<String>,
}

impl ContractMap {
    /// Creates a new ContractMap from a given map of ContractName -> CodeIDs
    pub fn new(code_map: &HashMap<String, u64>) -> Self {
        let mut map = HashMap::new();
        for (name, code_id) in code_map {
            map.insert(
                name.clone(),
                DeployInfo {
                    code_id: *code_id,
                    address: None,
                },
            );
        }
        Self { map }
    }

    /// Registers a new code id and contract name with the contract map
    pub fn register_contract(&mut self, name: String, code_id: u64) {
        self.map.insert(
            name,
            DeployInfo {
                code_id,
                address: None,
            },
        );
    }

    /// Returns the stored code id for a given contract name
    pub fn code_id(&self, name: &str) -> Result<u64> {
        let info = self.map.get(name).context("contract not stored")?;
        Ok(info.code_id)
    }

    /// Returns the stored contract address for a given contract name
    pub fn address(&self, name: &str) -> Result<String> {
        self.map
            .get(name)
            .context("contract not stored")?
            .address
            .clone()
            .context("contract not deployed")
    }

    /// Registers a contract address with an already stored contract
    pub fn add_address(&mut self, name: &str, address: String) -> Result<()> {
        self.map
            .get_mut(name)
            .context("contract not stored")?
            .address = Some(address);
        Ok(())
    }

    /// Removes all configured contract addresses, leaving the stored contract ids intact
    pub fn clear_addresses(&mut self) {
        for (_n, d) in self.map.iter_mut() {
            d.address = None
        }
    }
}
