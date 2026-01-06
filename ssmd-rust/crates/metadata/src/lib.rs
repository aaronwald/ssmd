//! ssmd-metadata: Shared metadata types mirroring Go types

pub mod error;
pub mod feed;
pub mod environment;

pub use error::MetadataError;
pub use feed::{
    AuthMethod, Calendar, CaptureLocation, Feed, FeedStatus, FeedType, FeedVersion,
    MessageProtocol, Protocol, SiteType, TransportProtocol,
};
pub use environment::{
    CacheConfig, CacheType, Environment, KeySpec, KeyType, Schedule, SecmasterConfig,
    StorageConfig, StorageType, SubscriptionConfig, TransportConfig, TransportType,
};
