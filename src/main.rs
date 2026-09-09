// main.rs
pub mod auth;
pub mod config;

// TODO(next slice): re-introduce request/response types and Redis storage
// modules (previously `interfaces`, `api`, `db`, `constants`, `utils`)
// rebuilt on axum + Redis instead of warp + valence_core + Mongo.

use auth::AuthedAddress;
use axum::{routing::get, Router};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cfg = config::load();

    info!(
        "Config loaded: extern_port={} cache_url={} cache_ttl_secs={} body_limit_bytes={} debug={}",
        cfg.extern_port, cfg.cache_url, cfg.cache_ttl_secs, cfg.body_limit_bytes, cfg.debug
    );

    // TODO(next slice): wire up Redis connection using cfg.cache_url / cfg.cache_ttl_secs
    // TODO(next slice): add /messages routes using the AuthedAddress extractor

    let app = Router::new()
        .route("/healthz", get(healthz))
        // Temporary authed probe route demonstrating the AuthedAddress
        // extractor ahead of the /messages handlers landing.
        .route("/whoami", get(whoami))
        .layer(CorsLayer::permissive())
        .layer(RequestBodyLimitLayer::new(cfg.body_limit_bytes));

    let addr = format!("[::]:{}", cfg.extern_port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to bind to {addr}: {e}");
            return;
        }
    };

    info!("Listening on {addr}");

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Server error: {e}");
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn whoami(AuthedAddress(address): AuthedAddress) -> String {
    address
}
