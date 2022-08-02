use config::ConfigError as _ConfigError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error(transparent)]
    Config(#[from] _ConfigError),
}
