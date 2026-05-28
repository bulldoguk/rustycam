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
    toml::from_str(&text).context("Failed to parse config.toml")
}
