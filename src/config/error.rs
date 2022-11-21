use thiserror::Error;

pub use config::ConfigError as BuilderError;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error(transparent)]
    Config(#[from] BuilderError),
}
