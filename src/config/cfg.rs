use anyhow::Result;
use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug)]
pub struct Config {
    pub(crate) tx_flags: Vec<String>,
    cfg: Cfg,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cfg {
    pub chain_cfg: ChainCfg,
    // used to configure already stored code_id dependencies
    // TODO: Just switch out `u64` for `DeployInfo` to allow users to already have the contract addr configured as well
    pub code_ids: HashMap<String, u64>,
    // key used to sign the transactions
    pub key_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChainCfg {
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

        let cfg = settings.try_deserialize::<Cfg>()?;

        Ok(Config {
            tx_flags: Self::build_tx_flags(&cfg),
            cfg,
        })
    }

    fn build_tx_flags(cfg: &Cfg) -> Vec<String> {
        //TODO: Allow more of this to be configured
        vec![
            "--gas-prices".to_string(),
            format!("0.1{}", cfg.chain_cfg.denom),
            "--gas".to_string(),
            "auto".to_string(),
            "--gas-adjustment".to_string(),
            "1.5".to_string(),
            "-b".to_string(),
            "block".to_string(),
            "--chain-id".to_string(),
            cfg.chain_cfg.chain_id.clone(),
            "--node".to_string(),
            cfg.chain_cfg.rpc_endpoint.clone(),
            "--from".to_string(),
            cfg.key_name.clone(),
            "--output".to_string(),
            "json".to_string(),
            "-y".to_string(),
        ]
    }
}

impl std::ops::Deref for Config {
    type Target = Cfg;
    fn deref(&self) -> &Self::Target {
        &self.cfg
    }
}
