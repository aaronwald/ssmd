pub mod config;
pub mod error;
pub mod messages;
pub mod replication;
pub mod publisher;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;
