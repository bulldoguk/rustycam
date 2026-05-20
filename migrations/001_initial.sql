CREATE TABLE IF NOT EXISTS events (
    id          TEXT PRIMARY KEY,
    camera_id   TEXT NOT NULL,
    camera_name TEXT NOT NULL,
    timestamp   TEXT NOT NULL,
    snapshot_path TEXT,
    clip_path   TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_events_camera    ON events(camera_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
