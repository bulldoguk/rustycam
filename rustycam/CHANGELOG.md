# Changelog

## 0.1.7 (2026-05-24)

### Fixed
- Switch to Alpine-based build and runtime — musl libc by default, no glibc anywhere
- `cameras` config field is now optional (defaults to empty list) so the add-on starts cleanly before any cameras are configured

## 0.1.6 (2026-05-24)

### Fixed
- Build as a statically-linked musl binary — eliminates all glibc version dependencies
- Switch `reqwest` to `rustls-tls` (removes openssl dependency, compatible with musl)
- Runtime reverts to `debian:bookworm-slim` (glibc version no longer relevant)

## 0.1.5 (2026-05-24)

### Fixed
- Upgrade runtime to `debian:trixie-slim` (glibc 2.39) to match what `rust:bookworm` builder actually links against

## 0.1.4 (2026-05-24)

### Fixed
- Pin builder to `rust:bookworm` to match the `debian:bookworm-slim` runtime — `rust:slim` moved to trixie (glibc 2.39) causing a glibc mismatch at startup

## 0.1.3 (2026-05-24)

### Fixed
- Switch Rust builder to `rust:slim` (tracks latest stable) to avoid recurring rustc version floor issues
- Remove deprecated `armv7` arch value from add-on config

## 0.1.2 (2026-05-24)

### Fixed
- Bumped Rust builder image to 1.86 — `icu_properties_data`, `icu_provider`, and `idna_adapter` require rustc 1.86

## 0.1.1 (2026-05-24)

### Fixed
- Bumped Rust builder image to 1.85 to support `edition2024` required by `base64ct 1.8.3`
- Dockerfile now correctly installs `jq` and uses `run.sh` as the container entrypoint

## 0.1.0 (2026-05-24)

Initial release.

### Features
- Continuous RTSP ring buffer per camera via ffmpeg
- ONVIF pull-point event subscription with WS-Security digest authentication
- AI-only event triggering — people, face, vehicle, pet, field detector, zone, custom rules; raw motion alarm excluded to reduce false positives
- Event capture: RTSP snapshot at trigger time + clip assembled from ring buffer segments
- Configurable pre/post event window
- EXIF metadata tagging on snapshots and clips (camera name, event type, keywords)
- SQLite event database at `/data/rustycam.db`
- ONVIF subscription auto-renewal every 30 minutes
- Per-camera crash recovery with automatic restart
- 5-second event debounce
- Health endpoint at `/health`
- Home Assistant add-on packaging with NAS storage via `/share`
- Camera credentials configured through HA UI (passwords masked)
