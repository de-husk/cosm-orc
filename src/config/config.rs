use anyhow::Result;
use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub chain_cfg: ChainConfig,
    pub tx_flags: Vec<String>,
    // used to configure already stored code_id dependencies
    // TODO: Just switch out `u64` for `DeployInfo` to allow users to already have the contract addr configured as well
    pub code_ids: HashMap<String, u64>,
    // key used to sign the transactions
    pub key_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChainConfig {
    pub binary: String,
    pub denom: String,
    pub chain_id: String,
    pub rpc_endpoint: String,
}

impl Config {
    pub fn from_yaml(file: &str) -> Result<Config> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .add_source(config::Environment::with_prefix("GAS"))
            .build()?;

        let cfg = settings.try_deserialize::<Config>()?;

        Ok(cfg)
    }
}