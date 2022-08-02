# Cosm-Orc

[![cosm-orc on crates.io](https://img.shields.io/crates/v/cosm-orc.svg)](https://crates.io/crates/cosm-orc) [![Docs](https://docs.rs/cosm-orc/badge.svg)](https://docs.rs/cosm-orc)

Rust Cosmwasm smart contract orchestration and gas profiling library.

Store, instantiate, execute, and query [Cosmwasm](https://github.com/CosmWasm/cosmwasm) smart contracts against a configured [Cosmos](https://github.com/cosmos/cosmos-sdk) based chain. 

Optionally, profile gas usage of the smart contract operations.

Potential uses:
* Integration tests
* Deployments / Bootstrapping environments
* Gas profiling

This project is not yet intended to be used for mainnet.

## Quick Start
 ```rust
use cosm_orc::config::key::Key;
use cosm_orc::config::key::SigningKey;
use cosm_orc::{
    config::cfg::Config,
    orchestrator::cosm_orc::{CosmOrc, WasmMsg},
};
use cw20_base::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use std::error::Error;
fn main() -> Result<(), Box<dyn Error>> {
    // juno_local.yaml has the cw20_base code_id already stored
    // If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
    let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?)?;
    let key = SigningKey {
        name: "validator".to_string(),
        key: Key::Mnemonic("word1 word2 ...".to_string()),
    };

    let msgs: Vec<WasmMsg<InstantiateMsg, ExecuteMsg, QueryMsg>> = vec![
        WasmMsg::InstantiateMsg(InstantiateMsg {
            name: "Meme Token".to_string(),
            symbol: "MEME".to_string(),
            decimals: 6,
            initial_balances: vec![],
            mint: None,
            marketing: None,
        }),
        WasmMsg::QueryMsg(QueryMsg::TokenInfo {}),
    ];
    cosm_orc.process_msgs("cw20_base", "meme_token_test", &msgs, &key)?;
    Ok(())
}
```

See [here](https://github.com/de-husk/cosm-orc-examples) for example usages.


## Configuration


See [./example-configs](./example-configs/) directory for example yaml configs.

