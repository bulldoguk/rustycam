use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub cameras: Vec<CameraConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub bind: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub base_path: String,
    pub ring_buffer_dir: String,
    pub ring_segment_seconds: u32,
    pub ring_segments_kept: u32,
    pub pre_event_seconds: u64,
    pub post_event_seconds: u64,
    #[serde(default = "default_idle_debounce_seconds")]
    pub idle_debounce_seconds: u64,
    #[serde(default = "default_max_session_seconds")]
    pub max_session_seconds: u64,
    /// Treat `base_path` as a network mount: refuse to start, and drop events
    /// rather than write, whenever it is not actually mounted. Defaults to false
    /// so a plain local `storage_path` keeps working unchanged.
    #[serde(default)]
    pub require_mount: bool,
}

fn default_idle_debounce_seconds() -> u64 {
    30
}

fn default_max_session_seconds() -> u64 {
    60
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CameraConfig {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub username: String,
    pub password: String,
    pub rtsp_stream: String,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub excluded_topics: Vec<String>,
}

impl CameraConfig {
    pub fn rtsp_url(&self) -> String {
        let stream = match self.rtsp_stream.as_str() {
            "sub" => "h264Preview_01_sub",
            _ => "h264Preview_01_main",
        };
        format!(
            "rtsp://{}:{}@{}/{}",
            self.username, self.password, self.ip, stream
        )
    }

    pub fn onvif_event_url(&self) -> String {
        format!("http://{}/onvif/event_service", self.ip)
    }

}

pub fn load(path: &str) -> Result<Config> {
    let text = fs::read_to_string(path).with_context(|| format!("Cannot read config: {path}"))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Config> {
    toml::from_str(text).context("Failed to parse config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[server]
port = 8090
bind = "0.0.0.0"

[storage]
base_path = "/data"
ring_buffer_dir = "/tmp/ring"
ring_segment_seconds = 5
ring_segments_kept = 12
pre_event_seconds = 15
post_event_seconds = 15

[database]
path = "/data/rustycam.db"

[[cameras]]
id = "front_east"
name = "Front East"
ip = "192.168.20.20"
username = "admin"
password = "secret"
rtsp_stream = "main"
excluded_topics = ["VehicleDetect"]
"#;

    #[test]
    fn parses_camera_with_excluded_topics() {
        let config = parse(SAMPLE).unwrap();
        assert_eq!(config.cameras.len(), 1);
        let cam = &config.cameras[0];
        assert_eq!(cam.id, "front_east");
        assert_eq!(cam.excluded_topics, vec!["VehicleDetect".to_string()]);
    }

    #[test]
    fn excluded_topics_defaults_to_empty() {
        let text = SAMPLE.replace("excluded_topics = [\"VehicleDetect\"]\n", "");
        let config = parse(&text).unwrap();
        assert!(config.cameras[0].excluded_topics.is_empty());
    }

    #[test]
    fn idle_debounce_and_max_session_default_when_omitted() {
        let config = parse(SAMPLE).unwrap();
        assert_eq!(config.storage.idle_debounce_seconds, 30);
        assert_eq!(config.storage.max_session_seconds, 60);
    }

    #[test]
    fn rtsp_url_uses_sub_stream_path() {
        let cam = CameraConfig {
            id: "front_east".into(),
            name: "Front East".into(),
            ip: "192.168.20.20".into(),
            username: "admin".into(),
            password: "secret".into(),
            rtsp_stream: "sub".into(),
            zone: None,
            excluded_topics: vec![],
        };
        assert_eq!(
            cam.rtsp_url(),
            "rtsp://admin:secret@192.168.20.20/h264Preview_01_sub"
        );
    }

    #[test]
    fn rtsp_url_defaults_to_main_stream_for_unknown_value() {
        let mut cam_main = CameraConfig {
            id: "front_east".into(),
            name: "Front East".into(),
            ip: "192.168.20.20".into(),
            username: "admin".into(),
            password: "secret".into(),
            rtsp_stream: "main".into(),
            zone: None,
            excluded_topics: vec![],
        };
        let main_url = cam_main.rtsp_url();
        cam_main.rtsp_stream = "bogus".into();
        assert_eq!(cam_main.rtsp_url(), main_url);
    }

    #[test]
    fn onvif_event_url_format() {
        let cam = CameraConfig {
            id: "front_east".into(),
            name: "Front East".into(),
            ip: "192.168.20.20".into(),
            username: "admin".into(),
            password: "secret".into(),
            rtsp_stream: "main".into(),
            zone: None,
            excluded_topics: vec![],
        };
        assert_eq!(cam.onvif_event_url(), "http://192.168.20.20/onvif/event_service");
    }
}

#[cfg(test)]
mod require_mount_tests {
    use super::*;

    #[test]
    fn require_mount_parses_from_toml_and_defaults_off() {
        let with = r#"
[server]
port = 8090
bind = "0.0.0.0"
[storage]
base_path = "/media/reolink_box"
ring_buffer_dir = "/tmp/ring"
ring_segment_seconds = 5
ring_segments_kept = 12
pre_event_seconds = 15
post_event_seconds = 15
require_mount = true
[database]
path = "/data/rustycam.db"
"#;
        let cfg: Config = toml::from_str(with).unwrap();
        assert!(cfg.storage.require_mount, "require_mount must round-trip from run.sh TOML");

        let without = with.replace("require_mount = true\n", "");
        let cfg: Config = toml::from_str(&without).unwrap();
        assert!(!cfg.storage.require_mount, "must default off for existing installs");
    }
}
