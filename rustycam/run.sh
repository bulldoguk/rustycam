#!/bin/bash
set -e

OPTIONS=/data/options.json

STORAGE_PATH=$(jq -r '.storage_path' "$OPTIONS")
PRE_EVENT=$(jq -r '.pre_event_seconds' "$OPTIONS")
POST_EVENT=$(jq -r '.post_event_seconds' "$OPTIONS")

mkdir -p "$STORAGE_PATH"

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

    cat <<TOML
[[cameras]]
id = "$id"
name = "$name"
ip = "$ip"
username = "$username"
password = "$password"
rtsp_stream = "$rtsp_stream"

TOML
  done
} > /data/config.toml

export RUSTYCAM_CONFIG=/data/config.toml
exec /app/rustycam
