// main.rs
pub mod api;
pub mod constants;
pub mod db;
pub mod error;
pub mod interfaces;
pub mod utils;

#[cfg(test)]
pub mod tests;

use crate::api::routes::*;
use crate::utils::{
    construct_mongodb_conn, construct_redis_conn, init_cuckoo_filter, load_config, print_welcome,
    retry_with_backoff,
};

use futures::lock::Mutex;
use std::sync::Arc;
use tracing::{error, info};
use valence_core::api::utils::handle_rejection;

use warp::Filter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Fatal: Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    let cache_addr = format!("{}:{}", config.cache_url, config.cache_port);
    let db_addr = format!(
        "{}{}:{}@{}:{}",
        config.db_protocol, config.db_user, config.db_password, config.db_url, config.db_port
    );

    info!(
        "Initializing system components (max_retries: {})...",
        config.max_retries
    );

    // Retry Redis connection
    let cache_conn = match retry_with_backoff(
        "Redis Connection",
        || construct_redis_conn(&cache_addr),
        config.max_retries,
        std::time::Duration::from_secs(1),
    )
    .await
    {
        Ok(conn) => conn,
        Err(e) => {
            error!("Fatal: {}", e);
            std::process::exit(1);
        }
    };

    // Retry MongoDB connection
    let db_conn = match retry_with_backoff(
        "MongoDB Connection",
        || construct_mongodb_conn(&db_addr),
        config.max_retries,
        std::time::Duration::from_secs(1),
    )
    .await
    {
        Ok(conn) => conn,
        Err(e) => {
            error!("Fatal: {}", e);
            std::process::exit(1);
        }
    };

    let cf_import = match init_cuckoo_filter(db_conn.clone()).await {
        Ok(cf) => cf,
        Err(e) => {
            error!("Fatal: Failed to initialize cuckoo filter: {}", e);
            std::process::exit(1);
        }
    };
    let cuckoo_filter = Arc::new(Mutex::new(cf_import));

    info!("All system components initialized successfully");

    let routes = get_data_with_id(db_conn.clone(), cache_conn.clone(), cuckoo_filter.clone())
        .or(get_data(
            db_conn.clone(),
            cache_conn.clone(),
            cuckoo_filter.clone(),
        ))
        .or(set_data(
            db_conn.clone(),
            cache_conn.clone(),
            cuckoo_filter.clone(),
            config.body_limit,
            config.cache_ttl,
        ))
        .or(del_data(
            db_conn.clone(),
            cache_conn.clone(),
            cuckoo_filter.clone(),
        ))
        .recover(handle_rejection);

    print_welcome(&db_addr, &cache_addr);

    info!("Server running at localhost:{}", config.extern_port);

    warp::serve(routes)
        .run(([0, 0, 0, 0], config.extern_port))
        .await;
}
