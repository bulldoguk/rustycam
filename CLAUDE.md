# RustyCam — Claude Instructions

## Release process (required before every push)

1. **Bump the version** in `rustycam/config.yaml` (`version: "x.y.z"`)
2. **Add a changelog entry** in `CHANGELOG.md` under the new version heading
3. Commit version bump and changelog together with the feature changes (or as a follow-up commit)
4. Then push

If changes were already committed without bumping the version, do a follow-up commit before pushing.

## Project layout

- `rustycam/src/` — Rust source (main binary)
- `rustycam/config.yaml` — Home Assistant add-on manifest (contains version)
- `config.toml` / `config.dev.toml` — example runtime configs
- `rustycam/CHANGELOG.md` — human-readable release notes

## Key source files

- `src/capture.rs` — ring buffer, RTSP snapshot, clip assembly, exiftool tagging
- `src/onvif.rs` — ONVIF event subscription and topic parsing
- `src/config.rs` — config structs (deserialised from TOML)
- `src/storage.rs` — file path helpers and SQLite event log

## Tag namespacing convention

All metadata tags written by exiftool follow this hierarchy:

- `camera/<cam.id>` — one per camera (id matches config)
- `event/person`, `event/animal`, `event/vehicle` — normalised from ONVIF topics
- `zone/<zone>` — optional, set per camera in config
- `site/home` — fixed
- `source/reolink` — fixed

ONVIF label mapping (in `normalize_event_label`):
- `PeopleDetect` → `person`
- `DogCatDetect` → `animal`
- `VehicleDetect` → `vehicle`

## Timezone / Immich

Clips and snapshots must have `DateTimeOriginal` set to **local time with UTC offset** so Immich places them on the correct calendar day. `chrono::Local` is used at runtime — no hardcoded timezone. See `tag_file` and `write_xmp_sidecar` in `capture.rs`.
