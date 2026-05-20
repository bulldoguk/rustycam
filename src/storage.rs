use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tracing::info;

use crate::config::{CameraConfig, DatabaseConfig, StorageConfig};

pub async fn init_dirs(cfg: &StorageConfig) -> Result<()> {
    tokio::fs::create_dir_all(&cfg.base_path).await?;
    tokio::fs::create_dir_all(&cfg.ring_buffer_dir).await?;
    Ok(())
}

pub async fn init_db(cfg: &DatabaseConfig) -> Result<SqlitePool> {
    let url = format!("sqlite://{}?mode=rwc", cfg.path);
    let pool = SqlitePool::connect(&url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database ready at {}", cfg.path);
    Ok(pool)
}

pub fn snapshot_path(base: &str, camera_id: &str, event_id: &str, ts: &DateTime<Utc>) -> PathBuf {
    PathBuf::from(base)
        .join("snapshots")
        .join(ts.format("%Y-%m-%d").to_string())
        .join(camera_id)
        .join(format!("{event_id}.jpg"))
}

pub fn clip_path(base: &str, camera_id: &str, event_id: &str, ts: &DateTime<Utc>) -> PathBuf {
    PathBuf::from(base)
        .join("clips")
        .join(ts.format("%Y-%m-%d").to_string())
        .join(camera_id)
        .join(format!("{event_id}.mp4"))
}

pub async fn insert_event(
    db: &SqlitePool,
    event_id: &str,
    cam: &CameraConfig,
    timestamp: &DateTime<Utc>,
    snapshot: &PathBuf,
    clip: &PathBuf,
) -> Result<()> {
    let ts = timestamp.to_rfc3339();
    let snap = snapshot.to_string_lossy().to_string();
    let clip = clip.to_string_lossy().to_string();

    sqlx::query(
        "INSERT INTO events (id, camera_id, camera_name, timestamp, snapshot_path, clip_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(event_id)
    .bind(&cam.id)
    .bind(&cam.name)
    .bind(&ts)
    .bind(&snap)
    .bind(&clip)
    .execute(db)
    .await?;

    Ok(())
}
