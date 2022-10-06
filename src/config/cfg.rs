use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::chain_registry::{parse_url, ChainCfg};
use super::error::ConfigError;
use crate::client::error::ClientError;
use crate::orchestrator::deploy::DeployInfo;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub chain_cfg: ChainCfg,
    // used to configure already stored contract code_id and deployed addresses
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

impl Config {
    pub fn from_config_input(cfg_input: ConfigInput) -> Result<Self, ConfigError> {
        Ok(Self {
            contract_deploy_info: cfg_input.contract_deploy_info.clone(),
            chain_cfg: cfg_input.to_chain_cfg()?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigInput {
    pub chain_cfg: ChainConfig,
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

impl ConfigInput {
    /// Converts a ConfigInput into a ChainCfg
    #[allow(clippy::infallible_destructuring_match)]
    pub fn to_chain_cfg(self) -> Result<ChainCfg, ConfigError> {
        let chain_cfg = match self.chain_cfg {
            ChainConfig::Custom(chain_cfg) => {
                // parse and optionally fix scheme for configured api endpoints:
                let rpc_endpoint = parse_url(&chain_cfg.rpc_endpoint)?;
                let grpc_endpoint = parse_url(&chain_cfg.grpc_endpoint)?;

                ChainCfg {
                    denom: chain_cfg.denom,
                    prefix: chain_cfg.prefix,
                    chain_id: chain_cfg.chain_id,
                    gas_prices: chain_cfg.gas_prices,
                    gas_adjustment: chain_cfg.gas_adjustment,
                    rpc_endpoint,
                    grpc_endpoint,
                }
            }

            #[cfg(feature = "chain-reg")]
            ChainConfig::ChainRegistry(chain_id) => {
                use crate::config::chain_registry::chain_info;
                use crate::orchestrator::cosm_orc::tokio_block;
                // TODO: expose an async version of this API
                tokio_block(chain_info(chain_id))?
            }
        };

        Ok(chain_cfg)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChainConfig {
    /// Allows you to manually configure any cosmos based chain
    Custom(ChainCfg),
    /// Uses the cosmos chain registry to auto-populate ChainCfg based on given chain_id string
    /// Enable `chain-reg` feature to use.
    #[cfg(feature = "chain-reg")]
    ChainRegistry(String),
}

impl Config {
    /// Reads a yaml file containing a `ConfigInput` and converts it to a useable `Config` object.
    pub fn from_yaml(file: &str) -> Result<Config, ConfigError> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .build()?;
        let cfg = settings.try_deserialize::<ConfigInput>()?;

        let contract_deploy_info = cfg.contract_deploy_info.clone();
        let chain_cfg = cfg.to_chain_cfg()?;

        Ok(Config {
            chain_cfg,
            contract_deploy_info,
        })
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
