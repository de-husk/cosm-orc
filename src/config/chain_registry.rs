use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tendermint_rpc::error::ErrorDetail::UnsupportedScheme;
use tendermint_rpc::{Error, Url};

#[cfg(feature = "chain-reg")]
use super::error::ConfigError;
#[cfg(feature = "chain-reg")]
use rand::Rng;

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

// Get usable chain info from chain registry api
#[cfg(feature = "chain-reg")]
pub(crate) async fn chain_info(chain_id: String) -> Result<ChainCfg, ConfigError> {
    let chain = chain_registry::get::get_chain(&chain_id)
        .await
        .map_err(|e| ConfigError::ChainRegistryAPI { source: e })?
        .ok_or_else(|| ConfigError::ChainID {
            chain_id: chain_id.clone(),
        })?;

    let fee_token = chain
        .fees
        .fee_tokens
        .get(0)
        .ok_or_else(|| ConfigError::MissingFee {
            chain_id: chain_id.clone(),
        })?;

    let mut rng = rand::thread_rng();

    let mut rpc_endpoint = chain
        .apis
        .rpc
        .get(rng.gen_range(0..chain.apis.rpc.len()))
        .ok_or_else(|| ConfigError::MissingRPC {
            chain_id: chain_id.clone(),
        })?
        .address
        .clone();

    let mut grpc_endpoint = chain
        .apis
        .grpc
        .get(rng.gen_range(0..chain.apis.grpc.len()))
        .ok_or_else(|| ConfigError::MissingGRPC {
            chain_id: chain_id.clone(),
        })?
        .address
        .clone();

    // parse and optionally fix scheme for configured api endpoints:
    rpc_endpoint = parse_url(&rpc_endpoint)?;
    grpc_endpoint = parse_url(&grpc_endpoint)?;

    Ok(ChainCfg {
        denom: fee_token.denom.clone(),
        prefix: chain.bech32_prefix,
        chain_id: chain.chain_id,
        gas_prices: fee_token.average_gas_price.into(),
        // TODO: We should probably let the user configure `gas_adjustment` for this path as well
        gas_adjustment: 1.5,
        rpc_endpoint,
        grpc_endpoint,
    })
}

// Attempt to parse the configured url to ensure that it is valid.
// If url is missing the Scheme then default to https.
pub(crate) fn parse_url(url: &str) -> Result<String, Error> {
    let u = Url::from_str(url);

    if let Err(Error(UnsupportedScheme(detail), report)) = u {
        // if url is missing the scheme, then we will default to https:
        if !url.contains("://") {
            return Ok(format!("https://{}", url));
        }

        return Err(Error(UnsupportedScheme(detail), report));
    }

    Ok(u?.to_string())
}
