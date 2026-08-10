use axum::{routing::get, Json, Router};
use serde_json::json;
use tracing::info;

use crate::config::ServerConfig;
use crate::storage;

pub async fn run(cfg: ServerConfig) -> anyhow::Result<()> {
    // /health keeps returning a bare "ok" — anything already probing it stays working.
    // /status is the machine-readable one, so HA can alert on storage going away
    // instead of nobody noticing for days.
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/status",
            get(|| async {
                let (storage_mounted, events_dropped) = storage::storage_status();
                Json(json!({
                    "storage_mounted": storage_mounted,
                    "events_dropped": events_dropped,
                }))
            }),
        );

    let addr = format!("{}:{}", cfg.bind, cfg.port);
    info!("Health endpoint on http://{addr}/health, status on http://{addr}/status");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
