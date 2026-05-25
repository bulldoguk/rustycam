use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{info, warn};
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
    topics: &[String],
) -> Result<()> {
    let event_id = Uuid::new_v4().to_string();

    // Snapshot: grab a frame directly from the RTSP stream at trigger time.
    // Faster than the camera's HTTP snapshot API and captures the exact moment.
    let snapshot = storage::snapshot_path(&cfg.base_path, &cam.id, &event_id, &timestamp);
    tokio::fs::create_dir_all(snapshot.parent().unwrap()).await?;
    frame_from_rtsp(cam, &snapshot).await?;
    tag_file(cam, &snapshot, topics).await;
    write_xmp_sidecar(cam, &snapshot, topics).await;
    info!("Snapshot saved: {}", snapshot.display());

    // Wait for post-event footage to accumulate in the ring buffer
    tokio::time::sleep(tokio::time::Duration::from_secs(cfg.post_event_seconds)).await;

    // Extract clip from ring buffer segments
    let clip = storage::clip_path(&cfg.base_path, &cam.id, &event_id, &timestamp);
    tokio::fs::create_dir_all(clip.parent().unwrap()).await?;
    extract_clip(cam, cfg, &clip).await?;
    tag_file(cam, &clip, topics).await;
    write_xmp_sidecar(cam, &clip, topics).await;
    info!("Clip saved: {}", clip.display());

    storage::insert_event(db, &event_id, cam, &timestamp, &snapshot, &clip).await?;

    Ok(())
}

async fn write_xmp_sidecar(cam: &CameraConfig, path: &PathBuf, topics: &[String]) {
    let labels: Vec<String> = topics
        .iter()
        .map(|t| t.rsplit('/').next().unwrap_or(t.as_str()).to_string())
        .collect();

    let sidecar = PathBuf::from(format!("{}.xmp", path.to_string_lossy()));

    // First tag sets the value, subsequent tags append with +=
    let mut args: Vec<String> = vec![
        format!("-XMP-digiKam:TagsList=camera/{}", cam.id),
    ];
    for label in &labels {
        args.push(format!("-XMP-digiKam:TagsList+=event/{}", label));
    }
    args.push("-XMP-digiKam:TagsList+=rustycam".into());
    args.push("-o".into());
    args.push(sidecar.to_string_lossy().into_owned());
    args.push(path.to_string_lossy().into_owned());

    let result = Command::new("exiftool")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(s) if s.success() => {}
        Ok(_) => warn!("exiftool sidecar write returned non-zero for {}", path.display()),
        Err(e) => warn!("exiftool not available, skipping XMP sidecar: {e}"),
    }
}

async fn tag_file(cam: &CameraConfig, path: &PathBuf, topics: &[String]) {
    // Extract the last path segment of each ONVIF topic as a human-readable label
    // e.g. "tns1:RuleEngine/MyRuleDetector/PeopleDetect" → "PeopleDetect"
    let labels: Vec<String> = topics
        .iter()
        .map(|t| t.rsplit('/').next().unwrap_or(t.as_str()).to_string())
        .collect();

    let description = format!("{} — {}", cam.name, labels.join(", "));

    let mut args: Vec<String> = vec![
        format!("-Make=Reolink"),
        format!("-Model={}", cam.id),
        format!("-ImageDescription={}", description),
        format!("-Comment={}", description),
        "-overwrite_original".into(),
    ];
    for label in &labels {
        args.push(format!("-Keywords+={}", label));
        args.push(format!("-Subject+={}", label));
    }
    args.push(format!("-Keywords+={}", cam.id));
    args.push(format!("-Subject+={}", cam.id));
    args.push(format!("-Keywords+=rustycam"));
    args.push(format!("-Subject+=rustycam"));
    args.push(path.to_string_lossy().into_owned());

    let result = Command::new("exiftool")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(s) if s.success() => {}
        Ok(_) => warn!("exiftool returned non-zero for {}", path.display()),
        Err(e) => warn!("exiftool not available, skipping metadata tagging: {e}"),
    }
}

async fn frame_from_rtsp(cam: &CameraConfig, dest: &PathBuf) -> Result<()> {
    // Pull one frame directly from the live RTSP stream at the moment of trigger.
    // -strict unofficial: allows MJPEG output from limited-range H.264 YUV.
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-rtsp_transport", "tcp",
            "-i", &cam.rtsp_url(),
            "-frames:v", "1",
            "-update", "1",
            "-strict", "unofficial",
            "-q:v", "2",
            dest.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;

    if !status.success() {
        bail!("ffmpeg RTSP snapshot failed for camera {}", cam.id);
    }

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
