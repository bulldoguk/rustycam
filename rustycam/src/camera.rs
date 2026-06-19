use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::capture;
use crate::config::{CameraConfig, StorageConfig};
use crate::onvif;

const RENEW_INTERVAL_SECS: u64 = 1800; // 30 min
const PULL_TIMEOUT_SECS: u32 = 5;      // long-poll window

// Only AI-classification events trigger captures. Raw MotionAlarm fires on
// bugs, leaves, etc. and is intentionally excluded here. VehicleDetection is
// also excluded — this property is on a main street, so passing traffic
// makes it too noisy to capture on.
const AI_TOPICS: &[&str] = &[
    "PeopleDetection",
    "DogCatDetection",
    "FaceDetection",
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
    let mut warmup_until = Instant::now() + Duration::from_secs(storage_cfg.pre_event_seconds);

    // Session tracking for capture debounce: a "session" is a run of triggers
    // close enough together to be the same visit. We capture once at session
    // start, then suppress further triggers until either the session goes
    // idle (no triggers for idle_debounce_seconds) or runs long enough that
    // a new visitor may have arrived (max_session_seconds), at which point
    // we capture again and start a new session.
    let mut session_start: Option<Instant> = None;
    let mut last_seen: Option<Instant> = None;
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
                    Ok(r) => {
                        ring = r;
                        // Reset warmup — new ring buffer has no pre-event footage yet
                        warmup_until = Instant::now() + Duration::from_secs(storage_cfg.pre_event_seconds);
                    }
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

        // Only act on active AI-classification events; raw MotionAlarm is excluded.
        // Per-camera excluded_topics matches against the topic's final segment
        // (e.g. "VehicleDetect"), so a noisy detector can be silenced on one
        // camera without affecting others that rely on the same AI_TOPICS entry.
        let triggered = events.iter().any(|e| {
            e.is_active
                && AI_TOPICS.iter().any(|t| e.topic.contains(t))
                && !config.excluded_topics.iter().any(|t| e.topic.ends_with(t.as_str()))
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

        // Session-based debounce
        let now = Instant::now();
        let idle_gap = last_seen.map_or(Duration::from_secs(0), |t| now.duration_since(t));
        let session_age = session_start.map_or(Duration::from_secs(0), |t| now.duration_since(t));
        last_seen = Some(now);

        let in_session = session_start.is_some()
            && idle_gap < Duration::from_secs(storage_cfg.idle_debounce_seconds);

        if in_session && session_age < Duration::from_secs(storage_cfg.max_session_seconds) {
            info!("[{}] Debounced (same session)", config.id);
            continue;
        }

        // Either a fresh session (idle gap exceeded) or the current session
        // ran long enough that a new visitor may have shown up — capture and
        // start a new session clock.
        session_start = Some(now);

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
