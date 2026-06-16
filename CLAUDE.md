# RustyCam — Claude Instructions

type: coding

## Repo
- Local path: ~/Documents/claude/projects/rustycam  (symlinked from ~/Documents/rustycam)
- Remote: https://github.com/bulldoguk/rustycam.git
- Primary branch: main

## Release process (required before every push)

1. **Bump the version** in `rustycam/config.yaml` (`version: "x.y.z"`)
2. **Add a changelog entry** in `CHANGELOG.md` under the new version heading
3. Commit version bump and changelog together with the feature changes (or as a follow-up commit)
4. Then push — GitHub Actions builds the Docker image automatically

If changes were already committed without bumping the version, do a follow-up commit before pushing.

## CI / deployment (as of 0.1.19)

GitHub Actions (`.github/workflows/build.yml`) builds a multi-arch image (`linux/amd64` + `linux/arm64`) on every push to `main` that touches `rustycam/`, and pushes to `ghcr.io/bulldoguk/rustycam:{version}` + `:latest`. Layer cache makes builds 2-3 min after the first run.

`config.yaml` has `image: ghcr.io/bulldoguk/rustycam` — the Supervisor pulls instead of compiling from source.

**To deploy an update to HA:**
```bash
ssh homeassistant.local "ha store reload"
```
Then via MCP: `ha_manage_addon(slug="6dcb2f9a_rustycam", action="update")`

Wait for the Actions run to go green before triggering the store reload.

**ghcr.io package visibility:** must be Public for HA to pull without credentials. Check at `https://github.com/bulldoguk/rustycam/pkgs/container/rustycam` → Package Settings → Change visibility.

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

## File naming convention (as of 0.1.18)

Matches the HA automation convention so RustyCam is a drop-in replacement:
```
{storage_path}/{camera_id}/{detection_type}/{YYYY}/{MM}/{DD}/
  {camera_id}_{detection_type}_{YYYY-MM-DD_HH-MM-SS}.jpg
  {camera_id}_{detection_type}_{YYYY-MM-DD_HH-MM-SS}.mp4
```
Timestamps use local time (`chrono::Local`). Detection type is the first normalised ONVIF topic (`person`, `animal`, `vehicle`), falling back to `motion`.

Set `storage_path` to `/media/ha_media/reolink` to share the same directory tree as the HA snapshot automations.

## Timezone / Immich

Clips and snapshots must have `DateTimeOriginal` set to **local time with UTC offset** so Immich places them on the correct calendar day. `chrono::Local` is used at runtime — no hardcoded timezone. See `tag_file` and `write_xmp_sidecar` in `capture.rs`.
