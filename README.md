# Cosm-Orc

[![cosm-orc on crates.io](https://img.shields.io/crates/v/cosm-orc.svg)](https://crates.io/crates/cosm-orc) [![Docs](https://docs.rs/cosm-orc/badge.svg)](https://docs.rs/cosm-orc)

Rust Cosmwasm smart contract integration testing and gas profiling library. 

Store, instantiate, execute, and query [Cosmwasm](https://github.com/CosmWasm/cosmwasm) smart contracts against a configured [Cosmos](https://github.com/cosmos/cosmos-sdk) based chain. 

Optionally, profile gas usage of the smart contract operations.

If you need a more general Cosmos SDK client library try [cosm-tome](https://github.com/de-husk/cosm-tome), which we use here under the hood.

Potential uses:
* Integration tests
* Deployments / Bootstrapping environments
* Gas profiling

This project is not intended to be used for mainnet.

## Quick Start

 ```rust
// juno_local.yaml has the `cw20_base` code_id already stored
// If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?, false)?;
let key = SigningKey {
    name: "validator".to_string(),
    key: Key::Mnemonic("word1 word2 ...".to_string()),
};

cosm_orc.instantiate(
    "cw20_base",
    "meme_token_test",
    &InstantiateMsg {
        name: "Meme Token".to_string(),
        symbol: "MEME".to_string(),
        decimals: 6,
        initial_balances: vec![],
        mint: None,
        marketing: None,
    },
    &key,
    None,
    vec![]
)?;

let res = cosm_orc.query(
    "cw20_base",
    &QueryMsg::TokenInfo {},
)?;
let res: TokenInfoResponse = res.data()?;
```

See [here](https://github.com/de-husk/cosm-orc-examples) for example usages.

## Optimize and Store Contracts

If `config.yaml` doesn't have the pre-stored contract code ids, you can call `optimize_contracts()` and `store_contracts()`:
 ```rust
let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?, false)?;
let key = SigningKey {
    name: "validator".to_string(),
    key: Key::Mnemonic("word1 word2 ...".to_string()),
};

// Build + optimize all smart contracts in current workspace
// This will save the optimized wasm files in `./artifacts`
cosm_orc.optimize_contracts("./Cargo.toml")?;

// NOTE: currently cosm-orc is expecting a wasm filed called: `cw20_base.wasm`
// to be in `/artifacts`, since `cw20_base` is used as the contract name in the instantiate()/query() calls below:
cosm_orc.store_contracts("./artifacts", &key, None)?;

cosm_orc.instantiate(
    "cw20_base",
    "meme_token_test",
    &InstantiateMsg {
        name: "Meme Token".to_string(),
        symbol: "MEME".to_string(),
        decimals: 6,
        initial_balances: vec![],
        mint: None,
        marketing: None,
    },
    &key,
    None,
    vec![]
)?;

let res = cosm_orc.query(
    "cw20_base",
    &QueryMsg::TokenInfo {},
)?;
let res: TokenInfoResponse = res.data()?;
```

## Gas Profiling

 ```rust
let mut cosm_orc = CosmOrc::new(Config::from_yaml("config.yaml")?, true)?;

cosm_orc.instantiate(
    "cw20_base",
    "meme_token_test",
    &InstantiateMsg {
        name: "Meme Token".to_string(),
        symbol: "MEME".to_string(),
        decimals: 6,
        initial_balances: vec![],
        mint: None,
        marketing: None,
    },
    &key,
    None,
    vec![]
)?;

let reports = cosm_orc.gas_profiler_report();
```

### Gas Report Github Action

Use the [cosm-orc-github-action](https://github.com/de-husk/cosm-orc-gas-diff-action) to view the cosm-orc gas usage as a PR comment.

Github action also supports showing the diff between 2 different reports.

Examples:
 * https://github.com/de-husk/cosm-orc-examples/pull/7

## Configuration

See [./example-configs](./example-configs/) directory for example yaml configs.

