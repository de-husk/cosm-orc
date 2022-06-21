use anyhow::{bail, Result};
use log::error;
use serde_json::Value;
use std::process::Command;

pub enum CommandType {
    Store,
    Instantiate,
    Query,
    Execute,
}

pub fn exec_msg(binary: &str, cmd_type: CommandType, args: &[String]) -> Result<Value> {
    let base_args = match cmd_type {
        CommandType::Store => vec!["tx", "wasm", "store"],
        CommandType::Instantiate => vec!["tx", "wasm", "instantiate"],
        CommandType::Query => vec!["query", "wasm", "contract-state", "smart"],
        CommandType::Execute => vec!["tx", "wasm", "execute"],
    };

    let res = Command::new(binary).args(&base_args).args(args).output()?;

    if !res.status.success() {
        error!("{}", String::from_utf8(res.stderr)?);
        bail!("invalid args");
    }

    let json: Value = serde_json::from_slice(&res.stdout)?;
    if json["code"].is_number() && json["code"] != 0 {
        error!("{}", json["raw_log"]);
        bail!("error processing message on chain");
    }

    Ok(json)
}
