use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::info;
use uuid::Uuid;

use crate::config::{CameraConfig, StorageConfig};
use crate::storage;

pub async fn start_ring_buffer(cam: &CameraConfig, cfg: &StorageConfig) -> Result<Child> {
    let buf_dir = PathBuf::from(&cfg.ring_buffer_dir).join(&cam.id);
    tokio::fs::create_dir_all(&buf_dir).await?;

    let pattern = buf_dir.join("seg%03d.ts");

    let child = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "warning",
            "-rtsp_transport",
            "tcp",
            "-i",
            &cam.rtsp_url(),
            "-c",
            "copy",
            "-f",
            "segment",
            "-segment_time",
            &cfg.ring_segment_seconds.to_string(),
            "-segment_wrap",
            &cfg.ring_segments_kept.to_string(),
            "-reset_timestamps",
            "1",
            pattern.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn ffmpeg — is ffmpeg installed?")?;

    info!("Ring buffer started for camera {} ({})", cam.id, cam.rtsp_url());
    Ok(child)
}

pub async fn capture_event(
    cam: &CameraConfig,
    cfg: &StorageConfig,
    db: &SqlitePool,
    timestamp: DateTime<Utc>,
) -> Result<()> {
    let event_id = Uuid::new_v4().to_string();

    // Snapshot: pull immediately before the subject moves away
    let snapshot = storage::snapshot_path(&cfg.base_path, &cam.id, &event_id, &timestamp);
    tokio::fs::create_dir_all(snapshot.parent().unwrap()).await?;
    fetch_snapshot(cam, &snapshot).await?;
    info!("Snapshot saved: {}", snapshot.display());

    // Wait for post-event footage to accumulate in the ring buffer
    tokio::time::sleep(tokio::time::Duration::from_secs(cfg.post_event_seconds)).await;

    // Extract clip from ring buffer segments
    let clip = storage::clip_path(&cfg.base_path, &cam.id, &event_id, &timestamp);
    tokio::fs::create_dir_all(clip.parent().unwrap()).await?;
    extract_clip(cam, cfg, &clip).await?;
    info!("Clip saved: {}", clip.display());

    storage::insert_event(db, &event_id, cam, &timestamp, &snapshot, &clip).await?;

    Ok(())
}

async fn fetch_snapshot(cam: &CameraConfig, dest: &PathBuf) -> Result<()> {
    let bytes = reqwest::get(cam.snapshot_url())
        .await
        .context("Snapshot HTTP request failed")?
        .bytes()
        .await?;

    tokio::fs::write(dest, &bytes).await?;
    Ok(())
}

async fn extract_clip(cam: &CameraConfig, cfg: &StorageConfig, output: &PathBuf) -> Result<()> {
    let buf_dir = PathBuf::from(&cfg.ring_buffer_dir).join(&cam.id);

    // Collect segments sorted by mtime — ring buffer wraps numerically so
    // mtime order is the only reliable way to get chronological sequence.
    // Skip segments with zero size: ffmpeg briefly truncates a file to zero
    // at the start of each new segment, creating a race window where the
    // file exists in the directory but is unreadable.
    let mut segments: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut entries = tokio::fs::read_dir(&buf_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let p = entry.path();
        if p.extension().map_or(false, |e| e == "ts") {
            if let Ok(meta) = entry.metadata().await {
                if meta.len() > 0 {
                    segments.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), p));
                }
            }
        }
    }
    segments.sort_by_key(|(mtime, _)| *mtime);
    let mut segments: Vec<PathBuf> = segments.into_iter().map(|(_, p)| p).collect();

    if segments.is_empty() {
        bail!("No ring buffer segments for camera {}", cam.id);
    }

    // Limit to segments covering the clip window. We called this after
    // post_event_seconds, so the ring buffer now contains pre+post footage.
    // Taking more segments than the window just adds irrelevant history.
    let window_secs = cfg.pre_event_seconds + cfg.post_event_seconds;
    let max_segs = (window_secs / cfg.ring_segment_seconds as u64 + 2) as usize;
    if segments.len() > max_segs {
        segments.drain(0..segments.len() - max_segs);
    }

    // Write ffmpeg concat manifest
    let concat_path = buf_dir.join("concat.txt");
    let manifest: String = segments
        .iter()
        .map(|p| format!("file '{}'\n", p.display()))
        .collect();
    tokio::fs::write(&concat_path, &manifest).await?;

    let status = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "warning",
            "-fflags",
            "+discardcorrupt+igndts",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            concat_path.to_str().unwrap(),
            "-c",
            "copy",
            "-fflags",
            "+igndts",
            output.to_str().unwrap(),
        ])
        .status()
        .await?;

    if !status.success() {
        bail!("ffmpeg clip extraction failed for camera {}", cam.id);
    }

    Ok(())
}
