use axum::{routing::get, Router};
use tracing::info;

use crate::config::ServerConfig;

pub async fn run(cfg: ServerConfig) -> anyhow::Result<()> {
    let app = Router::new().route("/health", get(|| async { "ok" }));

    let addr = format!("{}:{}", cfg.bind, cfg.port);
    info!("Health endpoint on http://{addr}/health");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
