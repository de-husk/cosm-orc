use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::ConfigError;
use crate::{client::error::ClientError, orchestrator::deploy::DeployInfo};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub chain_cfg: ChainCfg,
    // used to configure already stored contract code_id and deployed addresses
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainCfg {
    pub denom: String,
    pub prefix: String,
    pub chain_id: String,
    pub rpc_endpoint: String,
    pub grpc_endpoint: String,
    pub gas_prices: f64,
    pub gas_adjustment: f64,
}

impl Config {
    pub fn from_yaml(file: &str) -> Result<Config, ConfigError> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .build()?;

        Ok(settings.try_deserialize::<Config>()?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Coin {
    pub denom: String,
    pub amount: u64,
}

impl TryFrom<Coin> for cosmrs::Coin {
    type Error = ClientError;

    fn try_from(value: Coin) -> Result<Self, ClientError> {
        Ok(Self {
            denom: value.denom.parse().map_err(|_| ClientError::Denom {
                name: value.denom.clone(),
            })?,
            amount: value.amount.into(),
        })
    }
}
