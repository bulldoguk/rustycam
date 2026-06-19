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

## Cleanup job

`/config/shell_scripts/cleanup_reolink_snapshots.sh` runs nightly at 03:15 (automation in
`/config/automations/cameras/snapshots.yaml` → `shell_command.cleanup_reolink_snapshots`).
Deletes files under `ROOT_DIR` (default `/media/ha_media/reolink`) older than `RETENTION_DAYS`
(default 7), and also deletes the matching Immich asset via the Immich API when one exists.
Must match `*.mp4.xmp` sidecars alongside `*.jpg`/`*.jpeg`/`*.mp4` in its `find`, or sidecars
orphan forever once their parent file is deleted — this bit us once (see CHANGELOG-style note
in commit history around 2026-06-19, found via a bloated Immich library crawl).

## Timezone / Immich

Clips and snapshots must have `DateTimeOriginal` set to **local time with UTC offset** so Immich places them on the correct calendar day. `chrono::Local` is used at runtime — no hardcoded timezone. See `tag_file` and `write_xmp_sidecar` in `capture.rs`.

## Capture debounce (as of 0.1.21)

Capture uses a session-based debounce, not a fixed cooldown — see `run()` in `camera.rs`. First trigger captures and starts a session; further triggers are suppressed while the gap since the last trigger stays under `idle_debounce_seconds` (default 30) AND the session hasn't run longer than `max_session_seconds` (default 60). Both are tunable per-install via add-on options / `config.toml [storage]`, since a fixed cooldown alone can't collapse a single lingering visitor into one shot — it just delays the next one.

`AI_TOPICS` in `camera.rs` controls which ONVIF detection types actually trigger a capture. `VehicleDetection` was removed from this list (0.1.21) because the property is on a main street and passing traffic produced constant false-positive captures — it's still recognized (no "unknown topic" log warning) but treated like raw motion.

## Config-only changes don't need the full release process

Adding/removing a camera or tuning `idle_debounce_seconds` / `max_session_seconds` / `pre_event_seconds` / `post_event_seconds` is an **add-on options change**, not a code change — no version bump, no CI build, no `ha store reload`. Apply it directly:

1. Get current options: `ssh homeassistant.local 'curl -s -H "Authorization: Bearer $SUPERVISOR_TOKEN" http://supervisor/addons/6dcb2f9a_rustycam/info'`
2. POST the full updated options object (Supervisor replaces the whole options blob, not a merge) to `http://supervisor/addons/6dcb2f9a_rustycam/options`
3. `ssh homeassistant.local "ha apps restart 6dcb2f9a_rustycam"`

The `ha` CLI on this box has no `options`/`set-options` subcommand for apps — the REST API + `$SUPERVISOR_TOKEN` (already in the SSH session's env) is the only way. See `reference_ha_addon_options` memory.

New camera IP/username/password come from the Reolink integration's config entry (`/config/.storage/core.config_entries`, search by camera name) — don't guess credentials.

## Camera migration status (HA automation → RustyCam)

Migrated to RustyCam (HA "Snapshot" automation turned off + `initial_state: false` added in `snapshots.yaml`): `front_door_west`, `front_door`, `front_east`.

Still on the legacy HA automations in `/config/automations/cameras/snapshots.yaml` (not yet added to RustyCam): `backyard_east_duo3`, `dogs_outside`, `garage_ptz`, `kennel`, `kids_room`. (Snapshot in time — check `snapshots.yaml` and RustyCam's options for current state before relying on this.)

**Migration checklist for each remaining camera:**
1. Add the camera to RustyCam's options (see above) and restart the add-on
2. Confirm it shows up in `ha apps logs 6dcb2f9a_rustycam` (camera task starting + ONVIF subscription)
3. Turn off the matching HA automation: `ha_call_service(domain="automation", service="turn_off", entity_id="automation.<camera>_snapshot")`
4. Add `initial_state: false` to that automation's block in `snapshots.yaml` (SSH edit) so it doesn't silently re-enable on HA Core restart, then `ha_call_service(domain="automation", service="reload")`
