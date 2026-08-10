# Changelog

## 0.1.25 (2026-08-09)

### Added
- New `require_mount` add-on option (default `false`, so existing installs and the default local `storage_path` are unchanged). When enabled, RustyCam treats `storage_path` as a network mount and **refuses to write anything unless it is actually mounted**.
  - At startup, `run.sh` no longer runs `mkdir -p "$STORAGE_PATH"` when `require_mount` is set, and `storage::init_dirs()` refuses to create the directory — it exits with a clear error instead. Creating the directory is exactly what turns a missing mount into a plain local directory.
  - At capture time, `capture_event()` re-checks before writing. This is the case that matters: the mount can disappear while RustyCam is running healthily, so a startup-only check would not catch it.
  - When storage is unavailable the event is dropped, the process **stays alive** (HTTP server and ONVIF subscriptions keep running), and one error is logged per transition rather than one per event. Recording resumes by itself when the mount returns, logging how many events were dropped.
  - Mount detection compares the `st_dev` of the path against its parent, so it needs no extra crate and does not rely on `mountpoint(1)` being present in the image (`run.sh` uses the equivalent `stat -c %d` comparison).
- New `GET /status` endpoint returning `{"storage_mounted": bool, "events_dropped": n}`, so Home Assistant can alert on storage going away rather than nobody noticing for days. `GET /health` is unchanged and still returns a bare `ok`.

### Why
On 2026-08-08 the SMB mount to the brain box (`/media/reolink_box`) dropped after a power loss. A dead CIFS mount does not fail writes — it silently redirects them to local disk. RustyCam kept recording at ~6.7 GB/day into the Home Assistant box's internal 114 GB eMMC until it hit **0 bytes free**. Worse, those local files then blocked Supervisor from ever remounting the share (`Cannot mount ... existing data at /data/media/reolink_box`), so a transient network blip became a permanent outage that starved the Immich pipeline for nine days. Keeping the mountpoint empty is what makes recovery automatic.

### Tests
- `is_mountpoint` on `/` and on an ordinary nested directory.
- `init_dirs` refuses **and creates nothing** when `require_mount` is set and the path is absent or is a plain directory.
- `init_dirs` still creates directories normally when `require_mount` is `false` (back-compat).
- `storage_available` short-circuits to `true` when the guard is disabled.

## 0.1.24 (2026-06-19)

### Added
- First unit tests: capture-trigger decision logic (`camera.rs`, extracted into a pure `is_triggered()` function), ONVIF XML event parsing (`onvif.rs`), event-label normalization (`capture.rs`), and config TOML parsing + camera URL helpers (`config.rs`). 20 tests total, runnable locally with `cargo test`.
- CI now runs `cargo test --release` in a `test` job that the Docker `build` job depends on (`needs: test`), so a broken test blocks the image push. No code/runtime behavior change.

## 0.1.23 (2026-06-19)

### Fixed
- `excluded_topics` add-on option: `list(str)?` is not a valid HA add-on schema type — `list(a|b)` syntax actually means "select with literal options a/b", so it required the value to literally be the string `"str"`. Changed to a plain comma-separated `str?` option, translated to a TOML array in `run.sh`.

## 0.1.22 (2026-06-19)

### Added
- Per-camera `excluded_topics` option: detection types matching the topic's final segment (e.g. `VehicleDetect`) are recognized but not captured for that camera, without affecting other cameras that share the same ONVIF rule name (`MyRuleDetector`). Needed because `front_east`'s custom Reolink AI rule reports vehicle detections under `MyRuleDetector`, so the existing global `VehicleDetection` exclusion from 0.1.21 didn't catch it.

## 0.1.21 (2026-06-19)

### Changed
- Replaced the fixed 5s capture debounce with a session-based debounce: the first trigger captures and starts a "session"; further triggers are suppressed until either the session goes idle (`idle_debounce_seconds`, default 30s) or runs longer than `max_session_seconds` (default 60s), at which point a new capture is taken and the session restarts. This collapses repeated AI re-detections from one lingering visitor into far fewer shots, while still capturing again if a second visitor shows up during a long, continuously-triggering session. Both values are now configurable per-install via the add-on options (and in `config.toml` under `[storage]`).
- `VehicleDetection` no longer triggers a capture. The property is on a main street, so passing traffic was generating constant false-positive captures. The topic is still recognized (no "unknown topic" log warning) but treated like raw motion — logged, not captured.

## 0.1.20 (2026-06-16)

### Fixed
- Runtime image (`alpine:3.21`) had no `tzdata` package and no `TZ` env var, so `chrono::Local` silently fell back to UTC inside the container. Captures after ~7pm CDT were being filed under tomorrow's date folder and tagged with a `DateTimeOriginal` a day ahead, causing them to go missing from Immich's timeline. Added `tzdata` to the runtime image and set `ENV TZ=America/Chicago` so local-time resolution actually works.

## 0.1.19 (2026-06-16)

### Changed
- Version bump to test pre-built image pull from ghcr.io (no code changes)

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
