mod camera;
mod capture;
mod config;
mod onvif;
mod server;
mod storage;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustycam=info".into()),
        )
        .init();

    let cfg_path = std::env::var("RUSTYCAM_CONFIG").unwrap_or_else(|_| "config.toml".into());
    let cfg = config::load(&cfg_path)?;
    info!("Loaded config: {} cameras", cfg.cameras.len());

    storage::init_dirs(&cfg.storage).await?;
    let db = storage::init_db(&cfg.database).await?;

    for cam in cfg.cameras {
        let storage_cfg = cfg.storage.clone();
        let db = db.clone();

        tokio::spawn(async move {
            loop {
                if let Err(e) = camera::run(cam.clone(), storage_cfg.clone(), db.clone()).await {
                    tracing::error!("[{}] Camera task crashed: {e:#} — restarting in 10s", cam.id);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        });
    }

    server::run(cfg.server).await
}
