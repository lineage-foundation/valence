// main.rs
pub mod auth;
pub mod config;
pub mod messages;
pub mod store;

use std::sync::Arc;

use axum::{routing::get, Router};
use messages::MessagesState;
use store::RedisStore;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cfg = config::load();

    info!(
        "Config loaded: extern_port={} cache_url={} cache_ttl_secs={} body_limit_bytes={} debug={}",
        cfg.extern_port, cfg.cache_url, cfg.cache_ttl_secs, cfg.body_limit_bytes, cfg.debug
    );

    let redis_store = match RedisStore::connect(&cfg.cache_url).await {
        Ok(store) => store,
        Err(e) => {
            error!("Failed to connect to Redis at {}: {e}", cfg.cache_url);
            return;
        }
    };

    let messages_state = MessagesState {
        store: Arc::new(redis_store),
        ttl_secs: cfg.cache_ttl_secs,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(messages::router(messages_state))
        .layer(CorsLayer::permissive())
        .layer(RequestBodyLimitLayer::new(cfg.body_limit_bytes));

    let addr = format!("[::]:{}", cfg.extern_port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to {addr}: {e}");
            return;
        }
    };

    info!("Listening on {addr}");

    if let Err(e) = axum::serve(listener, app).await {
        error!("Server error: {e}");
    }
}

async fn healthz() -> &'static str {
    "ok"
}
