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
// juno_local.yaml has the `cw20_base` code_id already stored
// If the smart contract has not been stored on the chain yet use: `cosm_orc::store_contracts()`
let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?)?;
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
)?;

let res = cosm_orc.query(
    "cw20_base",
    "meme_token_test",
    &QueryMsg::TokenInfo {},
)?;
let res: TokenInfoResponse = res.data()?;
```

See [here](https://github.com/de-husk/cosm-orc-examples) for example usages.

## Store Contracts

If `config.yaml` doesn't have the pre-stored contract code ids, you can call `store_contracts()`:
 ```rust
let mut cosm_orc = CosmOrc::new(Config::from_yaml("./example-configs/juno_local.yaml")?)?;
let key = SigningKey {
    name: "validator".to_string(),
    key: Key::Mnemonic("word1 word2 ...".to_string()),
};

// `./artifacts` is a directory that contains the rust optimized wasm files.
//
// NOTE: currently cosm-orc is expecting a wasm filed called: `cw20_base.wasm`
// to be in `/artifacts`, since `cw20_base` is used as the contract name in the instantiate()/query() calls below:
cosm_orc.store_contracts("./artifacts", &key)?;

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
)?;

let res = cosm_orc.query(
    "cw20_base",
    "meme_token_test",
    &QueryMsg::TokenInfo {},
)?;
let res: TokenInfoResponse = res.data()?;
```

## Gas Profiling

 ```rust
let mut cosm_orc =
    CosmOrc::new(Config::from_yaml("config.yaml")?)?.add_profiler(Box::new(GasProfiler::new()));

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
)?;

let reports = cosm_orc.profiler_reports()?;
```

### Gas Report Github Action

Use the [cosm-orc-github-action](https://github.com/de-husk/cosm-orc-gas-diff-action) to view the cosm-orc gas usage as a PR comment.

Github action also supports showing the diff between 2 different reports.

Examples:
 * https://github.com/de-husk/cosm-orc-examples/pull/7

## Configuration

See [./example-configs](./example-configs/) directory for example yaml configs.

