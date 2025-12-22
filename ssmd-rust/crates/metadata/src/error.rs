use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("validation error: {0}")]
    Validation(String),
}
