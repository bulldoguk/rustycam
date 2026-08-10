# RustyCam — Claude Instructions

type: coding

## Roadmap: deprecating the HA snapshot-automation path
As of 2026-06-21, plan is to phase out the HA camera-snapshot automations
(`camera.snapshot`/`camera.record` → `shell_command.immich_ingest_snapshot`,
see "Camera snapshot automation pattern" in
[[projects/home-assistant/CLAUDE|home-assistant]]) over the coming weeks,
moving fully onto RustyCam's own capture pipeline. Once that happens:
- The dual-path Immich ingestion note above (API push vs. filesystem watcher)
  collapses to just the watcher path, since RustyCam never calls the Immich
  API directly.
- The HA automations in `/config/automations/cameras/snapshots.yaml` and
  `immich_import_and_tag.sh` become candidates for removal — don't delete
  them preemptively; wait until RustyCam is confirmed covering all cameras
  cleanly.

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

### Where storage_path points (updated 2026-08-09 — ADR 0010)

`storage_path` is **`/media/reolink_box`** on this install. That is a Supervisor `media`-usage
CIFS mount of `//192.168.20.90/reolink` — a Samba share on the **brain box** exposing
`/home/gary/reolink`. Immich then reads that same directory as **local disk**, so only the
RustyCam write crosses SMB and all thumbnail/encode work stays local.

> ⚠️ **The NAS is not in this path.** An older version of this doc said to set `storage_path` to
> `/media/ha_media/reolink` (the Buffalo NAS at `192.168.20.7`). That was correct pre-ADR-0010 and
> is now wrong — following it sends clips somewhere nothing collects them. `/media/ha_media` is
> still mounted for other purposes; don't confuse the two.

Set `require_mount: true` (0.1.25+) alongside it. Without the guard, if that SMB mount drops,
writes silently land on the HA box's internal eMMC — see the "Mount guard" note below.

### Mount guard (0.1.25+)

`require_mount: true` makes RustyCam refuse to start, and drop events rather than write, whenever
`storage_path` is not actually a mountpoint. This exists because on 2026-08-08 the mount dropped
after a power loss and RustyCam filled the 114 GB internal disk to **0 bytes free** at ~6.7 GB/day —
a dead CIFS mount does not fail writes, it redirects them to local disk. The local files then
blocked Supervisor from remounting (`existing data at /data/media/reolink_box`), turning a transient
blip into a nine-day outage. Keeping the mountpoint empty is what makes recovery automatic.
`GET /status` reports `{storage_mounted, events_dropped}` for alerting.

## Cleanup job

`/config/shell_scripts/cleanup_reolink_snapshots.sh` runs nightly at 03:15 (automation in
`/config/automations/cameras/snapshots.yaml` → `shell_command.cleanup_reolink_snapshots`).
Deletes files under `ROOT_DIR` (default **`/media/reolink_box`**) older than `RETENTION_DAYS`
(default **10**), and also deletes the matching Immich asset via the Immich API when one exists.
Both defaults were corrected 2026-08-09: `ROOT_DIR` had still pointed at the old NAS path, so the
job was faithfully pruning a tree that no longer held the captures while the real library grew to
132 GB unchecked.

Its `find` must match every file type that can appear in a day folder, or `rmdir` fails and that
day **wedges permanently** — the script deliberately refuses to `rm -rf`. Known suffixes that have
each had to be added after biting us: `*.mp4.xmp` sidecars (2026-06-19), `*_exiftool_tmp`
(underscore, so `*.tmp` never matched it) and `.__smb*` Samba orphans (both 2026-08-09).

Bulk deletes over this CIFS mount need **multiple passes** — a single pass reports success while
leaving most files behind. The script's "leaving for a future run" behaviour handles this correctly
on a nightly cadence; don't "fix" it.

## Immich library scan schedule (fixed 2026-06-21)
Immich's `system-config` `library.scan.cronExpression` was misconfigured to `*/5 * * * *` (every 5 minutes) instead of a nightly schedule. Every 5-minute run re-queued a full re-check of the entire library (~30K assets) plus a disk crawl, faster than the thumbnail/metadata workers (concurrency 3 and 5) could drain — this kept `thumbnailGeneration` stuck around ~26K waiting jobs for at least two days, looking like a stalled pipeline when it was actually an infinite top-up loop.

**New files don't depend on this scan.** Two separate real-time paths cover ingestion, neither of which is the cron scan:
- HA's camera-snapshot automations (the `camera.snapshot`/`camera.record` pattern) call `shell_command.immich_ingest_snapshot` → `immich_import_and_tag.sh`, which POSTs directly to `/assets` right after capture.
- **RustyCam itself does NOT call this script or the Immich API** — `capture.rs` writes clips straight to disk under `storage_path`. Those files are picked up by Immich's filesystem **watcher** (`library.watch.enabled: true`), which ingests on fs-change events, not by the API push.

The scheduled scan is just a periodic consistency sweep (catches anything the watcher missed, e.g. if Immich was down when a file landed) — it's not anyone's primary ingestion path.

**Fixed:** `cronExpression` set to `"30 3 * * *"` (3:30 AM, staggered after the 2:00 AM Immich DB backup and before the 3:15 AM `cleanup_reolink_snapshots.sh` job). Changed via `PUT /api/system-config` (full-object PUT required — partial payloads 400). Immich API URL/key are in HA `secrets.yaml` (`immich_api_url`, `immich_api_key`), readable the same way `immich_import_and_tag.sh` does (`read_secret` awk helper) — no need to print the key to inspect or change config, pipe straight from secrets file into curl on the HA host via SSH.

**To check job queue health:** `GET /api/jobs` (same auth) returns per-queue `active`/`waiting`/`failed`/`completed` counts — `active > 0` and `waiting` trending down means it's actually draining; flat `waiting` across days despite nonzero `active` is the signature of this exact bug (a scan/recheck re-adding jobs faster than workers clear them).

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

**All 8 cameras migrated to RustyCam** as of 2026-07-10 (HA "Snapshot" automations all turned off + `initial_state: false` in `snapshots.yaml`): `front_door_west`, `front_door`, `front_east`, `backyard_east_duo3`, `dogs_outside`, `garage_ptz`, `kennel`, `kids_room`. The HA snapshot-automation path is now fully idle — candidates for removal per the roadmap above once RustyCam has a few clean days across all cameras. (Snapshot in time — check `snapshots.yaml` and RustyCam's options for current state before relying on this.)

### kids_room TLS gotcha (resolved 2026-07-10)
`kids_room` (RLC-520A, newer fw `v3.2.0.5180` / hw `IPC_MS4NA45MP`) initially crash-looped in RustyCam with `invalid peer certificate: Other(OtherError(UnsupportedCertVersion))`. Root cause was **camera-side, not RustyCam**: the camera had `httpEnable: 0` (plain HTTP disabled, HTTPS-only), so its port-80 ONVIF endpoint returned `302 → https://<ip>`, and RustyCam's reqwest/rustls client rejected the camera's old self-signed cert. The kennel — same RLC-520A model but older fw `v3.0.0.4348` — ships `httpEnable: 1` and never redirects. **Fix:** re-enabled HTTP via the Reolink API (`SetNetPort` with `httpEnable: 1`, HTTPS left on) so ONVIF stays on plain HTTP like every other camera. Reolink NetPort query/set over `cgi-bin/api.cgi` (Login → token → GetNetPort/SetNetPort). Newer Reolink firmware may default to HTTPS-only — check `httpEnable` first if a new camera hits this rustls error. A durable RustyCam-side fix (accept self-signed ONVIF certs / don't follow http→https redirects) is still worth doing so this can't recur on the next firmware bump.

**Migration checklist for each remaining camera:**
1. Add the camera to RustyCam's options (see above) and restart the add-on
2. Confirm it shows up in `ha apps logs 6dcb2f9a_rustycam` (camera task starting + ONVIF subscription)
3. Turn off the matching HA automation: `ha_call_service(domain="automation", service="turn_off", entity_id="automation.<camera>_snapshot")`
4. Add `initial_state: false` to that automation's block in `snapshots.yaml` (SSH edit) so it doesn't silently re-enable on HA Core restart, then `ha_call_service(domain="automation", service="reload")`
