# Changelog

## 0.1.18 (2026-06-16)

### Changed
- File paths now match the HA automation convention: `{camera_id}/{detection_type}/{YYYY}/{MM}/{DD}/{camera_id}_{detection_type}_{YYYY-MM-DD_HH-MM-SS}.jpg/.mp4`
- Timestamps in filenames use local time (via `chrono::Local`) instead of UTC, consistent with how HA automations call `now().strftime()`
- Detection type is derived from the first ONVIF topic on each event (same normalisation: `PeopleDetect`→`person`, `DogCatDetect`→`animal`, `VehicleDetect`→`vehicle`), falling back to `motion` if no topic is present

## 0.1.17 (2026-05-28)

### Fixed
- Clips were being deleted or truncated by exiftool: `tag_file` used `-overwrite_original` on the MP4, which works by writing a temp file and swapping it in — when exiftool failed partway through, it deleted or corrupted the source clip. Clips now use the XMP sidecar exclusively for metadata; `tag_file` (EXIF in-place edit) is only called on JPG snapshots where it is safe and standard
- Make, Model, and Description fields moved into the XMP sidecar for clips so no metadata is lost

## 0.1.16 (2026-05-28)

### Fixed
- Concurrent captures (events firing >5s apart within the same post-event window) shared a single `concat.txt` manifest file — each capture now writes its own `concat_<uuid>.txt`, preventing races where one capture's ffmpeg reads another event's segment list and produces an empty or wrong clip
- Ring buffer warmup guard was not reset when ffmpeg died and restarted mid-session — events could trigger immediately after a restart before the ring buffer had accumulated any pre-event footage, producing clips with no pre-event content

## 0.1.15 (2026-05-28)

### Fixed
- Orphaned XMP sidecars with no corresponding MP4: ffmpeg with `-fflags +discardcorrupt` can exit 0 but produce an empty or header-only container when ring buffer packets are corrupt. Rustycam now checks the output file size after extraction and discards clips smaller than 1 KB, preventing the XMP sidecar from being written for invalid clips
- Partial output files left on disk when clip extraction fails are now deleted before the error is returned

## 0.1.14 (2026-05-28)

### Added
- Optional `zone` field per camera in config; written as `zone/<value>` tag in EXIF and XMP

### Changed
- Tags now use namespaced hierarchy: `camera/`, `event/`, `zone/`, `site/home`, `source/reolink` (replaces bare `rustycam` tag)
- ONVIF event labels normalised: `PeopleDetect` → `person`, `DogCatDetect` → `animal`, `VehicleDetect` → `vehicle`

### Fixed
- Immich timezone issue: `DateTimeOriginal` now written with local time + UTC offset so clips and snapshots appear on the correct calendar day (was rolling over at 7pm CDT)

## 0.1.13 (2026-05-25)

### Changed
- Restore full ONVIF topic logging so noisy cameras can be identified

## 0.1.12 (2026-05-25)

### Changed
- Silence log warnings for known-but-ignored ONVIF topics (`CellMotionDetector`, `AudioAlarm`) — these are intentionally excluded, not unknown

## 0.1.11 (2026-05-25)

### Changed
- XMP sidecar files written for clips only — JPG snapshots use EXIF metadata directly

## 0.1.10 (2026-05-25)

### Added
- Write XMP sidecar files (`.xmp`) alongside every snapshot and clip for Immich tag compatibility — tags include `camera/<id>`, `event/<type>`, and `rustycam`

## 0.1.9 (2026-05-25)

### Fixed
- Map `media:rw` in add-on config so `/media/...` storage paths correctly reach the HA media folder and NAS

## 0.1.8 (2026-05-25)

### Fixed
- Add `exiftool` to Alpine runtime image for EXIF metadata tagging

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
