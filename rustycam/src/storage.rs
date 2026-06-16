use anyhow::Result;
use chrono::{DateTime, Local, Utc};
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

// Matches the HA automation convention:
// {base}/{camera_id}/{detection_type}/{YYYY}/{MM}/{DD}/{camera_id}_{detection_type}_{YYYY-MM-DD_HH-MM-SS}.jpg
pub fn snapshot_path(base: &str, camera_id: &str, detection_type: &str, ts: &DateTime<Utc>) -> PathBuf {
    let local = ts.with_timezone(&Local);
    PathBuf::from(base)
        .join(camera_id)
        .join(detection_type)
        .join(local.format("%Y").to_string())
        .join(local.format("%m").to_string())
        .join(local.format("%d").to_string())
        .join(format!("{camera_id}_{detection_type}_{}.jpg", local.format("%Y-%m-%d_%H-%M-%S")))
}

pub fn clip_path(base: &str, camera_id: &str, detection_type: &str, ts: &DateTime<Utc>) -> PathBuf {
    let local = ts.with_timezone(&Local);
    PathBuf::from(base)
        .join(camera_id)
        .join(detection_type)
        .join(local.format("%Y").to_string())
        .join(local.format("%m").to_string())
        .join(local.format("%d").to_string())
        .join(format!("{camera_id}_{detection_type}_{}.mp4", local.format("%Y-%m-%d_%H-%M-%S")))
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
