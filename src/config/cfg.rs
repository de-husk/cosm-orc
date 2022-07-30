use anyhow::Result;
use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub chain_cfg: ChainCfg,
    // used to configure already stored code_id dependencies
    // TODO: Just switch out `u64` for `DeployInfo` to allow users to already have the contract addr configured as well
    #[serde(default)]
    pub code_ids: HashMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChainCfg {
    pub denom: String,
    pub prefix: String,
    pub chain_id: String,
    pub rpc_endpoint: String,
}

impl Config {
    pub fn from_yaml(file: &str) -> Result<Config> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .build()?;

        Ok(settings.try_deserialize::<Config>()?)
    }
}
