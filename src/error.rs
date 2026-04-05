use std::fmt;

#[derive(Debug)]
pub enum ValenceError {
    Config(String),
    Database(String),
    Cache(String),
    Filter(String),
    RetryLimitExceeded(String),
}

// The choice to not use `thiserror` was to reduce compile times and dependencies
impl fmt::Display for ValenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValenceError::Config(e) => write!(f, "Configuration error: {}", e),
            ValenceError::Database(e) => write!(f, "Database connection error: {}", e),
            ValenceError::Cache(e) => write!(f, "Cache connection error: {}", e),
            ValenceError::Filter(e) => write!(f, "Cuckoo filter error: {}", e),
            ValenceError::RetryLimitExceeded(e) => write!(f, "Retry limit exceeded: {}", e),
        }
    }
}

impl std::error::Error for ValenceError {}

// Manual From implementations to maintain the String-based variants while enabling '?'
impl From<config::ConfigError> for ValenceError {
    fn from(error: config::ConfigError) -> Self {
        ValenceError::Config(error.to_string())
    }
}

impl From<redis::RedisError> for ValenceError {
    fn from(error: redis::RedisError) -> Self {
        ValenceError::Cache(error.to_string())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ValenceError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        ValenceError::Database(error.to_string())
    }
}

impl From<serde_json::Error> for ValenceError {
    fn from(error: serde_json::Error) -> Self {
        ValenceError::Database(error.to_string())
    }
}

impl From<mongodb::error::Error> for ValenceError {
    fn from(error: mongodb::error::Error) -> Self {
        ValenceError::Database(error.to_string())
    }
}

impl From<mongodb::bson::ser::Error> for ValenceError {
    fn from(error: mongodb::bson::ser::Error) -> Self {
        ValenceError::Database(error.to_string())
    }
}

impl From<mongodb::bson::de::Error> for ValenceError {
    fn from(error: mongodb::bson::de::Error) -> Self {
        ValenceError::Database(error.to_string())
    }
}
