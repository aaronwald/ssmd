use std::sync::Arc;

use ssmd_metadata::{CacheType, Environment, StorageType, TransportType};

use crate::cache::Cache;
use crate::journal::Journal;
use crate::memory::{InMemoryCache, InMemoryJournal, InMemoryStorage, InMemoryTransport};
use crate::storage::Storage;
use crate::transport::Transport;

/// Error creating middleware
#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("unsupported transport type: {0:?}")]
    UnsupportedTransport(TransportType),
    #[error("unsupported storage type: {0:?}")]
    UnsupportedStorage(StorageType),
    #[error("unsupported cache type: {0:?}")]
    UnsupportedCache(CacheType),
    #[error("configuration error: {0}")]
    ConfigError(String),
}

/// Factory for creating middleware instances based on environment config
pub struct MiddlewareFactory;

impl MiddlewareFactory {
    /// Create a transport based on environment configuration
    pub fn create_transport(env: &Environment) -> Result<Arc<dyn Transport>, FactoryError> {
        match env.transport.transport_type {
            TransportType::Memory => Ok(Arc::new(InMemoryTransport::new())),
            TransportType::Nats => {
                // NATS implementation will come later
                Err(FactoryError::UnsupportedTransport(TransportType::Nats))
            }
            TransportType::Mqtt => {
                Err(FactoryError::UnsupportedTransport(TransportType::Mqtt))
            }
        }
    }

    /// Create a storage based on environment configuration
    pub fn create_storage(env: &Environment) -> Result<Arc<dyn Storage>, FactoryError> {
        match env.storage.storage_type {
            StorageType::Local => {
                // Local storage maps to in-memory for now
                // Real LocalStorage (file-based) will come later
                Ok(Arc::new(InMemoryStorage::new()))
            }
            StorageType::S3 => {
                Err(FactoryError::UnsupportedStorage(StorageType::S3))
            }
        }
    }

    /// Create a cache based on environment configuration
    pub fn create_cache(env: &Environment) -> Result<Arc<dyn Cache>, FactoryError> {
        let cache_type = env
            .cache
            .as_ref()
            .map(|c| c.cache_type.clone())
            .unwrap_or(CacheType::Memory);

        match cache_type {
            CacheType::Memory => Ok(Arc::new(InMemoryCache::new())),
            CacheType::Redis => {
                Err(FactoryError::UnsupportedCache(CacheType::Redis))
            }
        }
    }

    /// Create a journal (always in-memory for now)
    pub fn create_journal() -> Arc<dyn Journal> {
        Arc::new(InMemoryJournal::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssmd_metadata::{StorageConfig, TransportConfig};

    fn make_test_env() -> Environment {
        Environment {
            name: "test".to_string(),
            feed: "kalshi".to_string(),
            schema: "trade:v1".to_string(),
            schedule: None,
            keys: None,
            transport: TransportConfig {
                transport_type: TransportType::Memory,
                url: None,
            },
            storage: StorageConfig {
                storage_type: StorageType::Local,
                path: Some("/tmp/test".to_string()),
                bucket: None,
                region: None,
            },
            cache: None,
        }
    }

    #[test]
    fn test_create_memory_transport() {
        let env = make_test_env();
        let transport = MiddlewareFactory::create_transport(&env).unwrap();
        drop(transport);
    }

    #[test]
    fn test_create_local_storage() {
        let env = make_test_env();
        let storage = MiddlewareFactory::create_storage(&env).unwrap();
        drop(storage);
    }

    #[test]
    fn test_create_memory_cache() {
        let env = make_test_env();
        let cache = MiddlewareFactory::create_cache(&env).unwrap();
        drop(cache);
    }

    #[test]
    fn test_create_journal() {
        let journal = MiddlewareFactory::create_journal();
        drop(journal);
    }

    #[test]
    fn test_unsupported_transport() {
        let mut env = make_test_env();
        env.transport.transport_type = TransportType::Nats;

        let result = MiddlewareFactory::create_transport(&env);
        assert!(matches!(result, Err(FactoryError::UnsupportedTransport(_))));
    }
}
