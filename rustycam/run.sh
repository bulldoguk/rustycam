#!/bin/bash
set -e

OPTIONS=/data/options.json

STORAGE_PATH=$(jq -r '.storage_path' "$OPTIONS")
PRE_EVENT=$(jq -r '.pre_event_seconds' "$OPTIONS")
POST_EVENT=$(jq -r '.post_event_seconds' "$OPTIONS")
IDLE_DEBOUNCE=$(jq -r '.idle_debounce_seconds' "$OPTIONS")
MAX_SESSION=$(jq -r '.max_session_seconds' "$OPTIONS")
REQUIRE_MOUNT=$(jq -r '.require_mount // false' "$OPTIONS")

# When storage_path is meant to be a network mount, do NOT mkdir it. Creating
# the directory here is precisely what turns a missing mount into a local
# directory, after which writes silently fill the host disk and Supervisor
# refuses to remount ("existing data at ..."). Fail loudly instead.
if [ "$REQUIRE_MOUNT" = "true" ]; then
  # `mountpoint` is not present in every base image; compare device ids instead.
  dev_self=$(stat -c %d "$STORAGE_PATH" 2>/dev/null || echo "")
  dev_parent=$(stat -c %d "$(dirname "$STORAGE_PATH")" 2>/dev/null || echo "")
  if [ -z "$dev_self" ] || [ "$dev_self" = "$dev_parent" ]; then
    echo "FATAL: storage_path $STORAGE_PATH is not a mountpoint and require_mount is enabled." >&2
    echo "       Refusing to start rather than write camera footage to local disk." >&2
    exit 1
  fi
else
  mkdir -p "$STORAGE_PATH"
fi

{
  cat <<TOML
[server]
port = 8090
bind = "0.0.0.0"

[storage]
base_path = "$STORAGE_PATH"
ring_buffer_dir = "/tmp/rustycam_ring"
ring_segment_seconds = 5
ring_segments_kept = 12
pre_event_seconds = $PRE_EVENT
post_event_seconds = $POST_EVENT
idle_debounce_seconds = $IDLE_DEBOUNCE
max_session_seconds = $MAX_SESSION
require_mount = $REQUIRE_MOUNT

[database]
path = "/data/rustycam.db"

TOML

  jq -c '.cameras[]' "$OPTIONS" | while IFS= read -r cam; do
    id=$(printf '%s' "$cam" | jq -r '.id')
    name=$(printf '%s' "$cam" | jq -r '.name')
    ip=$(printf '%s' "$cam" | jq -r '.ip')
    username=$(printf '%s' "$cam" | jq -r '.username')
    password=$(printf '%s' "$cam" | jq -r '.password')
    rtsp_stream=$(printf '%s' "$cam" | jq -r '.rtsp_stream')
    excluded_topics_raw=$(printf '%s' "$cam" | jq -r '.excluded_topics // ""')

    cat <<TOML
[[cameras]]
id = "$id"
name = "$name"
ip = "$ip"
username = "$username"
password = "$password"
rtsp_stream = "$rtsp_stream"
TOML

    if [ -n "$excluded_topics_raw" ]; then
      # Comma-separated string option -> TOML array, e.g. "VehicleDetect, Foo" -> ["VehicleDetect", "Foo"]
      excluded_topics_toml=$(printf '%s' "$excluded_topics_raw" | tr ',' '\n' | sed 's/^ *//;s/ *$//' | jq -R . | jq -s -c .)
      echo "excluded_topics = $excluded_topics_toml"
    fi
    echo ""
  done
} > /data/config.toml

export RUSTYCAM_CONFIG=/data/config.toml
exec /app/rustycam
