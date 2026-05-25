# RustyCam NVR

A lightweight NVR (Network Video Recorder) for ONVIF cameras, built in Rust. Designed to run as a Home Assistant add-on with footage stored on a NAS.

## How it works

RustyCam connects to each camera via ONVIF and maintains a continuous ring buffer using ffmpeg. When the camera fires an AI-classification event (person, vehicle, face, pet, etc.), it:

1. Grabs a snapshot directly from the RTSP stream at the moment of trigger
2. Waits for post-event footage to accumulate in the ring buffer
3. Assembles a clip covering the pre- and post-event window
4. Tags both files with EXIF metadata (camera name, event type)
5. Records the event in a local SQLite database

Only AI-classification events trigger captures. Raw motion alarms are intentionally excluded to avoid false positives from insects, leaves, lighting changes, etc.

## Requirements

- ONVIF-compatible cameras with AI detection (tested on Reolink)
- Home Assistant OS or Supervised installation
- A NAS or network share mounted and accessible from HA (via `/share`)

## Installation

1. In Home Assistant, go to **Settings → Add-ons → Add-on Store**
2. Click the three-dot menu (⋮) → **Add repository**
3. Enter: `https://github.com/bulldoguk/rustycam`
4. Find **RustyCam NVR** in the store and click **Install**

## Configuration

| Option | Default | Description |
|---|---|---|
| `storage_path` | `/share/rustycam` | Where footage is written. `/share` maps to your HA share directory. Point this at your NAS mount. |
| `pre_event_seconds` | `15` | Seconds of footage to include before the trigger |
| `post_event_seconds` | `15` | Seconds of footage to include after the trigger |
| `cameras` | `[]` | List of cameras (see below) |

### Camera options

| Field | Description |
|---|---|
| `id` | Short identifier, used in file paths (e.g. `front_door`) |
| `name` | Display name (written to EXIF metadata) |
| `ip` | Camera IP address |
| `username` | Camera username |
| `password` | Camera password (masked in the HA UI) |
| `rtsp_stream` | `sub` (lower CPU, recommended) or `main` (full resolution) |

## Storage layout

```
/share/rustycam/
├── snapshots/
│   └── 2026-05-24/
│       └── front_door/
│           └── <uuid>.jpg
└── clips/
    └── 2026-05-24/
        └── front_door/
            └── <uuid>.mp4
```

## Supported AI event types

- People detection
- Face detection
- Vehicle detection
- Dog/cat detection
- Field detector (line crossing / zone entry)
- Objects inside
- Custom rule detector

## Web UI

The add-on exposes a health endpoint at `http://<ha-ip>:8090/health`. A full UI is planned for a future release.

## Camera compatibility

Tested with Reolink cameras. Any ONVIF camera that supports pull-point event subscriptions and exposes AI classification events should work. The RTSP stream paths (`h264Preview_01_sub` / `h264Preview_01_main`) are Reolink-specific — if you use a different brand you may need to provide the full RTSP URL directly.
