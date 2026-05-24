use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::capture;
use crate::config::{CameraConfig, StorageConfig};
use crate::onvif;

const DEBOUNCE_SECS: u64 = 5;
const RENEW_INTERVAL_SECS: u64 = 1800; // 30 min
const PULL_TIMEOUT_SECS: u32 = 5;      // long-poll window

// Only AI-classification events trigger captures. Raw MotionAlarm fires on
// bugs, leaves, etc. and is intentionally excluded here.
const AI_TOPICS: &[&str] = &[
    "PeopleDetection",
    "DogCatDetection",
    "FaceDetection",
    "VehicleDetection",
    "FieldDetector",
    "ObjectsInside",
    "MyRuleDetector",
];

pub async fn run(config: CameraConfig, storage_cfg: StorageConfig, db: SqlitePool) -> Result<()> {
    info!("[{}] Camera task starting ({})", config.id, config.ip);

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs((PULL_TIMEOUT_SECS + 5) as u64))
        .build()?;

    let mut ring = capture::start_ring_buffer(&config, &storage_cfg).await?;
    let mut subscription = onvif::subscribe(&http, &config).await?;

    // Wait for the ring buffer to accumulate pre_event_seconds of footage
    // before accepting triggers, so clips always have full pre-event context.
    let warmup_until = Instant::now() + Duration::from_secs(storage_cfg.pre_event_seconds);

    let mut last_event: Option<Instant> = None;
    let mut last_renew = Instant::now();

    loop {
        // Renew ONVIF subscription before it expires
        if last_renew.elapsed() > Duration::from_secs(RENEW_INTERVAL_SECS) {
            if let Err(e) = onvif::renew(&http, &subscription, &config).await {
                warn!("[{}] Renew failed ({e}), re-subscribing", config.id);
                match onvif::subscribe(&http, &config).await {
                    Ok(s) => subscription = s,
                    Err(e) => {
                        warn!("[{}] Re-subscribe failed: {e}", config.id);
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        continue;
                    }
                }
            }
            last_renew = Instant::now();
        }

        // Restart ring buffer if ffmpeg died
        match ring.try_wait() {
            Ok(Some(_)) => {
                warn!("[{}] Ring buffer process exited, restarting", config.id);
                match capture::start_ring_buffer(&config, &storage_cfg).await {
                    Ok(r) => ring = r,
                    Err(e) => {
                        warn!("[{}] Failed to restart ring buffer: {e}", config.id);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
            Ok(None) => {} // still running
            Err(e) => warn!("[{}] Ring buffer status check error: {e}", config.id),
        }

        // Long-poll: blocks here up to PULL_TIMEOUT_SECS, returns immediately on event
        let events = match onvif::pull_messages(&http, &subscription, &config, PULL_TIMEOUT_SECS).await {
            Ok(e) => e,
            Err(e) => {
                warn!("[{}] pull_messages error: {e}, re-subscribing", config.id);
                match onvif::subscribe(&http, &config).await {
                    Ok(s) => { subscription = s; }
                    Err(e) => warn!("[{}] Re-subscribe failed: {e}", config.id),
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };

        // Only act on active AI-classification events; raw MotionAlarm is excluded
        let triggered = events.iter().any(|e| {
            e.is_active && AI_TOPICS.iter().any(|t| e.topic.contains(t))
        });
        if !triggered {
            continue;
        }

        // Don't capture until the ring buffer has enough pre-event footage
        if Instant::now() < warmup_until {
            info!("[{}] Triggered but ring buffer still warming up, skipping", config.id);
            continue;
        }

        let active_topics: Vec<String> = events.iter()
            .filter(|e| e.is_active)
            .map(|e| e.topic.clone())
            .collect();
        info!("[{}] Triggered — topics: {:?}", config.id, active_topics);

        // Debounce
        if last_event.map_or(false, |t| t.elapsed() < Duration::from_secs(DEBOUNCE_SECS)) {
            info!("[{}] Debounced", config.id);
            continue;
        }
        last_event = Some(Instant::now());

        let timestamp = Utc::now();
        let config = config.clone();
        let storage_cfg = storage_cfg.clone();
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = capture::capture_event(&config, &storage_cfg, &db, timestamp, &active_topics).await {
                tracing::error!("[{}] Capture error: {e:#}", config.id);
            }
        });
    }
}
