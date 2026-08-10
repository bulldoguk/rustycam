use anyhow::{bail, Result};
use chrono::{DateTime, Local, Utc};
use sqlx::SqlitePool;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::{error, info};

use crate::config::{CameraConfig, DatabaseConfig, StorageConfig};

/// Tracks whether we are currently in the "storage went away" state, so the
/// error is logged once per transition rather than once per event. A single
/// camera can fire hundreds of events a day; flooding the log helps nobody.
static STORAGE_UNMOUNTED: AtomicBool = AtomicBool::new(false);
static EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);

/// True if `p` is the root of a mount.
///
/// A mount root and its parent live on different devices, so comparing st_dev
/// is enough and needs no external crate or `mountpoint(1)`. "/" has no parent
/// and is always a mount.
pub fn is_mountpoint(p: &Path) -> std::io::Result<bool> {
    let here = std::fs::metadata(p)?;
    let parent = match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => return Ok(true),
    };
    let up = std::fs::metadata(parent)?;
    Ok(here.dev() != up.dev())
}

/// Whether it is safe to write to storage right now.
///
/// When `require_mount` is set and the mount has gone away, writing would land
/// on the host's local disk: a dead CIFS mount does not fail writes, it silently
/// redirects them. That fills the local disk *and* leaves data in the mountpoint,
/// which then blocks Supervisor from ever remounting it. Dropping the event keeps
/// the mountpoint empty so recovery is automatic once the mount returns.
pub fn storage_available(cfg: &StorageConfig) -> bool {
    if !cfg.require_mount {
        return true;
    }

    if matches!(is_mountpoint(Path::new(&cfg.base_path)), Ok(true)) {
        if STORAGE_UNMOUNTED.swap(false, Ordering::Relaxed) {
            let dropped = EVENTS_DROPPED.swap(0, Ordering::Relaxed);
            info!(
                "Storage remounted at {} — recording resumed ({} event(s) dropped while unmounted)",
                cfg.base_path, dropped
            );
        }
        return true;
    }

    EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed);
    if !STORAGE_UNMOUNTED.swap(true, Ordering::Relaxed) {
        error!(
            "storage_path {} is not a mountpoint — refusing to write. Recording is PAUSED until the \
             mount returns; events are being dropped rather than written to local disk.",
            cfg.base_path
        );
    }
    false
}

/// (mounted, events dropped since it went away) — for the health endpoint.
pub fn storage_status() -> (bool, u64) {
    (
        !STORAGE_UNMOUNTED.load(Ordering::Relaxed),
        EVENTS_DROPPED.load(Ordering::Relaxed),
    )
}

pub async fn init_dirs(cfg: &StorageConfig) -> Result<()> {
    if cfg.require_mount {
        // Deliberately checked *before* create_dir_all: creating the directory
        // ourselves is what turns a missing mount into a local directory.
        match is_mountpoint(Path::new(&cfg.base_path)) {
            Ok(true) => {}
            Ok(false) => bail!(
                "storage_path {} exists but is not a mountpoint, and require_mount is set. \
                 Refusing to start so nothing is written to local disk.",
                cfg.base_path
            ),
            Err(e) => bail!(
                "storage_path {} is not accessible ({e}), and require_mount is set. \
                 Refusing to start so nothing is written to local disk.",
                cfg.base_path
            ),
        }
    } else {
        tokio::fs::create_dir_all(&cfg.base_path).await?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(base: &str, require_mount: bool) -> StorageConfig {
        StorageConfig {
            base_path: base.to_string(),
            ring_buffer_dir: format!("{base}/ring"),
            ring_segment_seconds: 5,
            ring_segments_kept: 12,
            pre_event_seconds: 15,
            post_event_seconds: 15,
            idle_debounce_seconds: 30,
            max_session_seconds: 60,
            require_mount,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustycam_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn root_is_a_mountpoint() {
        assert!(is_mountpoint(Path::new("/")).unwrap());
    }

    #[test]
    fn ordinary_directory_is_not_a_mountpoint() {
        let dir = scratch("plain");
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(!is_mountpoint(&nested).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn init_dirs_refuses_and_creates_nothing_when_mount_missing() {
        let dir = scratch("guard");
        let base = dir.join("not_a_mount");

        // Path does not exist at all -> must fail, and must not be created.
        let err = init_dirs(&cfg(base.to_str().unwrap(), true)).await;
        assert!(err.is_err(), "expected refusal when storage_path is absent");
        assert!(!base.exists(), "guard must not create the mountpoint directory");

        // Path exists but is a plain directory -> must still fail.
        std::fs::create_dir_all(&base).unwrap();
        let err = init_dirs(&cfg(base.to_str().unwrap(), true)).await;
        assert!(err.is_err(), "expected refusal when storage_path is not a mount");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn init_dirs_creates_normally_when_require_mount_is_off() {
        let dir = scratch("backcompat");
        let base = dir.join("local_storage");
        init_dirs(&cfg(base.to_str().unwrap(), false)).await.unwrap();
        assert!(base.exists(), "default behaviour must be unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_available_is_always_true_when_guard_disabled() {
        let dir = scratch("disabled");
        let base = dir.join("plain");
        std::fs::create_dir_all(&base).unwrap();
        assert!(storage_available(&cfg(base.to_str().unwrap(), false)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
