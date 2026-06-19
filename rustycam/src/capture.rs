use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Utc};
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

    let detection_type = topics
        .iter()
        .map(|t| normalize_event_label(t.rsplit('/').next().unwrap_or(t.as_str())))
        .next()
        .unwrap_or("motion");

    // Snapshot: grab a frame directly from the RTSP stream at trigger time.
    // Faster than the camera's HTTP snapshot API and captures the exact moment.
    let snapshot = storage::snapshot_path(&cfg.base_path, &cam.id, detection_type, &timestamp);
    tokio::fs::create_dir_all(snapshot.parent().unwrap()).await?;
    frame_from_rtsp(cam, &snapshot).await?;
    tag_file(cam, &snapshot, topics, timestamp).await;
    info!("Snapshot saved: {}", snapshot.display());

    // Wait for post-event footage to accumulate in the ring buffer
    tokio::time::sleep(tokio::time::Duration::from_secs(cfg.post_event_seconds)).await;

    // Extract clip from ring buffer segments
    let clip = storage::clip_path(&cfg.base_path, &cam.id, detection_type, &timestamp);
    tokio::fs::create_dir_all(clip.parent().unwrap()).await?;
    match extract_clip(cam, cfg, &clip, &event_id).await {
        Ok(()) => {}
        Err(e) => {
            // Clean up any partial output file ffmpeg may have created before failing
            let _ = tokio::fs::remove_file(&clip).await;
            return Err(e);
        }
    }
    // Guard against ffmpeg exiting 0 but producing an empty/header-only container
    // (can happen with -fflags +discardcorrupt when all ring buffer packets are bad)
    let clip_size = tokio::fs::metadata(&clip).await.map(|m| m.len()).unwrap_or(0);
    if clip_size < 1024 {
        let _ = tokio::fs::remove_file(&clip).await;
        bail!("ffmpeg produced a near-empty clip ({} bytes) for camera {} — discarding", clip_size, cam.id);
    }
    // Clips use XMP sidecar only — writing EXIF directly into an MP4 with
    // -overwrite_original can delete or truncate the file if exiftool fails
    write_xmp_sidecar(cam, &clip, topics, timestamp).await;
    info!("Clip saved: {} ({} bytes)", clip.display(), clip_size);

    storage::insert_event(db, &event_id, cam, &timestamp, &snapshot, &clip).await?;

    Ok(())
}

fn normalize_event_label(raw: &str) -> &str {
    match raw {
        "PeopleDetect" => "person",
        "DogCatDetect" => "animal",
        "VehicleDetect" => "vehicle",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_known_labels() {
        assert_eq!(normalize_event_label("PeopleDetect"), "person");
        assert_eq!(normalize_event_label("DogCatDetect"), "animal");
        assert_eq!(normalize_event_label("VehicleDetect"), "vehicle");
    }

    #[test]
    fn passes_through_unknown_labels() {
        assert_eq!(normalize_event_label("FieldDetector"), "FieldDetector");
    }
}

async fn write_xmp_sidecar(cam: &CameraConfig, path: &PathBuf, topics: &[String], timestamp: DateTime<Utc>) {
    let labels: Vec<&str> = topics
        .iter()
        .map(|t| normalize_event_label(t.rsplit('/').next().unwrap_or(t.as_str())))
        .collect();

    let local = timestamp.with_timezone(&Local);
    // ISO 8601 with offset, e.g. "2026-05-27T22:19:11-05:00"
    let xmp_dt = local.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

    let sidecar = PathBuf::from(format!("{}.xmp", path.to_string_lossy()));

    let description = format!("{} — {}", cam.name, labels.join(", "));

    // First tag sets the value, subsequent tags append with +=
    let mut args: Vec<String> = vec![
        format!("-XMP-exif:DateTimeOriginal={}", xmp_dt),
        format!("-XMP-tiff:Make=Reolink"),
        format!("-XMP-tiff:Model={}", cam.id),
        format!("-XMP-dc:Description={}", description),
        format!("-XMP-digiKam:TagsList=camera/{}", cam.id),
    ];
    for label in &labels {
        args.push(format!("-XMP-digiKam:TagsList+=event/{}", label));
    }
    if let Some(zone) = &cam.zone {
        args.push(format!("-XMP-digiKam:TagsList+=zone/{}", zone));
    }
    args.push("-XMP-digiKam:TagsList+=site/home".into());
    args.push("-XMP-digiKam:TagsList+=source/reolink".into());
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

async fn tag_file(cam: &CameraConfig, path: &PathBuf, topics: &[String], timestamp: DateTime<Utc>) {
    // Extract the last path segment of each ONVIF topic as a human-readable label
    // e.g. "tns1:RuleEngine/MyRuleDetector/PeopleDetect" → "PeopleDetect"
    let labels: Vec<&str> = topics
        .iter()
        .map(|t| normalize_event_label(t.rsplit('/').next().unwrap_or(t.as_str())))
        .collect();

    let description = format!("{} — {}", cam.name, labels.join(", "));

    let local = timestamp.with_timezone(&Local);
    // EXIF DateTimeOriginal uses "YYYY:MM:DD HH:MM:SS" format; offset is separate
    let exif_dt = local.format("%Y:%m:%d %H:%M:%S").to_string();
    let exif_offset = local.format("%:z").to_string();

    let mut args: Vec<String> = vec![
        format!("-Make=Reolink"),
        format!("-Model={}", cam.id),
        format!("-ImageDescription={}", description),
        format!("-Comment={}", description),
        format!("-DateTimeOriginal={}", exif_dt),
        format!("-OffsetTimeOriginal={}", exif_offset),
        "-overwrite_original".into(),
    ];
    for label in &labels {
        args.push(format!("-Keywords+=event/{}", label));
        args.push(format!("-Subject+=event/{}", label));
    }
    args.push(format!("-Keywords+=camera/{}", cam.id));
    args.push(format!("-Subject+=camera/{}", cam.id));
    if let Some(zone) = &cam.zone {
        args.push(format!("-Keywords+=zone/{}", zone));
        args.push(format!("-Subject+=zone/{}", zone));
    }
    args.push("-Keywords+=site/home".into());
    args.push("-Subject+=site/home".into());
    args.push("-Keywords+=source/reolink".into());
    args.push("-Subject+=source/reolink".into());
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

async fn extract_clip(cam: &CameraConfig, cfg: &StorageConfig, output: &PathBuf, event_id: &str) -> Result<()> {
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

    // Write ffmpeg concat manifest — use event_id to avoid concurrent captures
    // overwriting each other's manifest (captures are spawned as background tasks)
    let concat_path = buf_dir.join(format!("concat_{}.txt", event_id));
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

    let _ = tokio::fs::remove_file(&concat_path).await;

    if !status.success() {
        bail!("ffmpeg clip extraction failed for camera {}", cam.id);
    }

    Ok(())
}
