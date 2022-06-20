use std::collections::HashMap;

// TODO: Read these config values in from a yaml config file
pub struct Config {
  pub chain_cfg: ChainConfig,
  pub tx_flags: Vec<String>,

  // used to configure already stored code_id dependencies
  pub code_ids: HashMap<String, u64>,
  // key used to sign the transactions
  pub key_name: String,
  // the path to the rust optimized wasm contract binaries
  pub wasm_dir: String,
}

pub struct ChainConfig {
  pub binary: String,
  pub denom: String,
  pub chain_id: String,
  pub rpc_endpoint: String,
}
