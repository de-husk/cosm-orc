use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::ContractMapError;

pub type ContractName = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractMap {
    map: HashMap<ContractName, DeployInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeployInfo {
    pub code_id: Option<u64>,
    pub address: Option<String>,
}

impl ContractMap {
    /// Creates a new ContractMap from an existing configured ContractMap
    pub fn new(contract_deploys: HashMap<ContractName, DeployInfo>) -> Self {
        Self {
            map: contract_deploys,
        }
    }

    /// Registers a new code id and contract name with the contract map
    pub fn register_contract<S: Into<String>>(&mut self, name: S, code_id: u64) {
        self.map
            .entry(name.into())
            .or_insert(DeployInfo {
                code_id: None,
                address: None,
            })
            .code_id = Some(code_id);
    }

    /// Returns the stored code id for a given contract name
    pub fn code_id(&self, name: &str) -> Result<u64, ContractMapError> {
        let info = self
            .map
            .get(name)
            .ok_or(ContractMapError::NotStored { name: name.into() })?;

        let code_id = info
            .code_id
            .ok_or(ContractMapError::NotStored { name: name.into() })?;

        Ok(code_id)
    }

    /// Returns the stored contract address for a given contract name
    pub fn address(&self, name: &str) -> Result<String, ContractMapError> {
        self.map
            .get(name)
            .ok_or(ContractMapError::NotStored { name: name.into() })?
            .address
            .clone()
            .ok_or(ContractMapError::NotDeployed { name: name.into() })
    }

    /// Registers a contract address with an already stored contract
    pub fn add_address<S: Into<String>>(
        &mut self,
        name: &str,
        address: S,
    ) -> Result<(), ContractMapError> {
        self.map
            .entry(name.into())
            .or_insert(DeployInfo {
                code_id: None,
                address: None,
            })
            .address = Some(address.into());
        Ok(())
    }

    /// Returns current deploy info
    pub fn deploy_info(&self) -> &HashMap<String, DeployInfo> {
        &self.map
    }
}

#[cfg(test)]
mod tests {
    use crate::orchestrator::error::ContractMapError;

    use super::ContractMap;
    use std::collections::HashMap;

    #[test]
    fn can_register_new_code_id() {
        let mut map = ContractMap::new(HashMap::new());

        assert_eq!(
            map.code_id("cw-test").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw-test".to_string()
            }
        );

        map.register_contract("cw-test", 1337);
        assert_eq!(map.code_id("cw-test").unwrap(), 1337);
    }

    #[test]
    fn can_register_addr_without_storing() {
        let mut map = ContractMap::new(HashMap::new());
        assert_eq!(
            map.address("cw-test").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw-test".to_string()
            }
        );

        map.add_address("cw-test", "addr1").unwrap();
        assert_eq!(map.address("cw-test").unwrap(), "addr1");

        // Can register code id after registering address:
        assert_eq!(
            map.code_id("cw-test").unwrap_err(),
            ContractMapError::NotStored {
                name: "cw-test".to_string()
            }
        );

        map.register_contract("cw-test", 1337);
        assert_eq!(map.code_id("cw-test").unwrap(), 1337);
        assert_eq!(map.address("cw-test").unwrap(), "addr1");
    }
}
