# Changelog

## 0.1.14
- Tags now use namespaced hierarchy: `camera/`, `event/`, `zone/`, `site/`, `source/`
- ONVIF event labels normalised: `PeopleDetect` → `person`, `DogCatDetect` → `animal`, `VehicleDetect` → `vehicle`
- Added optional `zone` field per camera in config; written as `zone/<value>` tag
- Fixed Immich timezone issue: `DateTimeOriginal` now written with local time + UTC offset so clips and snapshots appear on the correct calendar day (was rolling over at 7pm CDT)
- Replaced bare `rustycam` tag with `source/reolink` and `site/home`

## 0.1.13
- Restored full ONVIF topic logging for diagnostics

## 0.1.12
- Silenced known-but-unhandled ONVIF topics to reduce log noise

## 0.1.11
- XMP sidecars written for video clips only; snapshots (JPGs) use EXIF directly

## 0.1.10
- Write XMP sidecar files (`.mp4.xmp`) alongside clips for Immich tag compatibility

## 0.1.9
- Map `media:rw` in add-on config so `/media` storage paths can reach the NAS

## 0.1.8
- Add `exiftool` to the Alpine runtime image

## 0.1.7
- Switch to Alpine build and runtime images; fix crash when `cameras` list is empty in config

## 0.1.6
- Switch to static musl build to eliminate glibc dependency on the HA host

## 0.1.5
- Upgrade runtime to `debian:trixie-slim` to fix glibc version mismatch

## 0.1.4
- Pin builder to `rust:bookworm` to fix glibc mismatch

## 0.1.3
- Use `rust:slim` builder image; drop deprecated `armv7` architecture

## 0.1.2
- Bump Rust builder to 1.86

## 0.1.1
- Bump Rust builder to 1.85 for `edition2024` support
- Fix Dockerfile: add `jq` and wire up `run.sh` entrypoint
- Fix repository URLs to match actual GitHub remote

## 0.1.0
- Initial working implementation: ONVIF event subscription, ring buffer via ffmpeg, RTSP snapshot on trigger, clip assembly, EXIF tagging, SQLite event log
- Packaged as a Home Assistant add-on
