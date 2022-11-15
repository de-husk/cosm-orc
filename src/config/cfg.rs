use config::Config as _Config;
use cosm_tome::config::cfg::ChainConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::ConfigError;
use crate::orchestrator::deploy::DeployInfo;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub chain_cfg: ChainConfig,
    // used to configure already stored contract code_id and deployed addresses
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

impl Config {
    /// Reads a yaml file containing a `ConfigInput` and converts it to a useable `Config` object.
    pub fn from_yaml(file: &str) -> Result<Config, ConfigError> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .build()?;

        Ok(settings.try_deserialize::<Config>()?)
    }
}
