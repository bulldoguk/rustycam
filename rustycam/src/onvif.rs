use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use quick_xml::events::Event;
use quick_xml::Reader;
use rand::RngCore;
use sha1::{Digest, Sha1};
use tracing::{debug, warn};

use crate::config::CameraConfig;

// Topics we treat as trigger events. Reolink firmware varies — unknown
// topics are logged so you can add them here.
const TRIGGER_TOPICS: &[&str] = &[
    "MotionAlarm",
    "MotionRegionDetector",
    "PeopleDetection",
    "DogCatDetection",
    "FaceDetection",
    "VehicleDetection",
    "FieldDetector",
    "ObjectsInside",
    "MyRuleDetector",
];

// Known topics we deliberately ignore — no warning logged for these.
// CellMotionDetector is Reolink's grid-based motion; too noisy to trigger on.
const IGNORED_TOPICS: &[&str] = &[
    "CellMotionDetector",
    "AudioAlarm",
];

#[derive(Debug, Clone)]
pub struct OnvifEvent {
    pub topic: String,
    pub is_active: bool,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn ws_security_header(username: &str, password: &str) -> String {
    let mut nonce_raw = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce_raw);
    let nonce_b64 = B64.encode(nonce_raw);
    let created = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // PasswordDigest = Base64(SHA1(nonce_bytes + created + password))
    let mut hasher = Sha1::new();
    hasher.update(nonce_raw);
    hasher.update(created.as_bytes());
    hasher.update(password.as_bytes());
    let digest = B64.encode(hasher.finalize());

    // Returns only the inner Security element; soap_post wraps it in <s:Header>
    format!(
        r#"<wsse:Security xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd"
                   xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd"
                   s:mustUnderstand="1">
      <wsse:UsernameToken>
        <wsse:Username>{username}</wsse:Username>
        <wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd#PasswordDigest">{digest}</wsse:Password>
        <wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd#Base64Binary">{nonce_b64}</wsse:Nonce>
        <wsu:Created>{created}</wsu:Created>
      </wsse:UsernameToken>
    </wsse:Security>"#
    )
}

// ── SOAP helper ──────────────────────────────────────────────────────────────

async fn soap_post(
    client: &reqwest::Client,
    url: &str,
    action: &str,
    security_element: &str,
    body: &str,
) -> Result<String> {
    let envelope = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope"
            xmlns:wsa="http://www.w3.org/2005/08/addressing"
            xmlns:tev="http://www.onvif.org/ver10/events/wsdl"
            xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2"
            xmlns:tt="http://www.onvif.org/ver10/schema">
  <s:Header>
    {security_element}
    <wsa:Action s:mustUnderstand="1">{action}</wsa:Action>
    <wsa:To s:mustUnderstand="1">{url}</wsa:To>
  </s:Header>
  <s:Body>{body}</s:Body>
</s:Envelope>"#
    );

    let response = client
        .post(url)
        .header("Content-Type", "application/soap+xml; charset=utf-8")
        .header("SOAPAction", action)
        .body(envelope)
        .send()
        .await
        .with_context(|| format!("SOAP request to {url} failed"))?;

    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        bail!("SOAP {url} returned {status}: {text}");
    }

    Ok(text)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Create a pull-point subscription and return the subscription endpoint URL.
pub async fn subscribe(client: &reqwest::Client, cam: &CameraConfig) -> Result<String> {
    let url = cam.onvif_event_url();
    let auth = ws_security_header(&cam.username, &cam.password);

    let body = r#"<tev:CreatePullPointSubscription>
      <tev:InitialTerminationTime>PT1H</tev:InitialTerminationTime>
    </tev:CreatePullPointSubscription>"#;

    let resp = soap_post(
        client,
        &url,
        "http://www.onvif.org/ver10/events/wsdl/EventPortType/CreatePullPointSubscriptionRequest",
        &auth,
        body,
    )
    .await?;

    let endpoint = extract_text(&resp, "Address")
        .unwrap_or_else(|| url.clone());

    tracing::info!("[{}] ONVIF subscription endpoint: {endpoint}", cam.id);
    Ok(endpoint)
}

/// Long-poll for events. The camera holds the connection for up to `timeout_secs`
/// and returns immediately when events are available. Returns only active/trigger events.
pub async fn pull_messages(
    client: &reqwest::Client,
    endpoint: &str,
    cam: &CameraConfig,
    timeout_secs: u32,
) -> Result<Vec<OnvifEvent>> {
    let auth = ws_security_header(&cam.username, &cam.password);

    let body = format!(
        r#"<tev:PullMessages>
      <tev:Timeout>PT{timeout_secs}S</tev:Timeout>
      <tev:MessageLimit>20</tev:MessageLimit>
    </tev:PullMessages>"#
    );

    let resp = soap_post(
        client,
        endpoint,
        "http://www.onvif.org/ver10/events/wsdl/PullPointSubscription/PullMessagesRequest",
        &auth,
        &body,
    )
    .await?;

    parse_events(&resp, &cam.id)
}

/// Renew the subscription. Call this periodically (e.g. every 30 min).
pub async fn renew(client: &reqwest::Client, endpoint: &str, cam: &CameraConfig) -> Result<()> {
    let auth = ws_security_header(&cam.username, &cam.password);

    let body = r#"<wsnt:Renew>
      <wsnt:TerminationTime>PT1H</wsnt:TerminationTime>
    </wsnt:Renew>"#;

    soap_post(
        client,
        endpoint,
        "http://docs.oasis-open.org/wsn/bw-2/SubscriptionManager/RenewRequest",
        &auth,
        body,
    )
    .await?;

    debug!("[{}] ONVIF subscription renewed", cam.id);
    Ok(())
}

// ── XML parsing ───────────────────────────────────────────────────────────────

fn parse_events(xml: &str, camera_id: &str) -> Result<Vec<OnvifEvent>> {
    let mut events: Vec<OnvifEvent> = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut current_topic: Option<String> = None;
    let mut in_topic = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local = local_name(&e.name());
                if local == "Topic" {
                    in_topic = true;
                }
                // Look for SimpleItem with alarm state values
                if local == "SimpleItem" {
                    let name = attr_value(&e, b"Name").unwrap_or_default();
                    let value = attr_value(&e, b"Value").unwrap_or_default();
                    let is_active = matches!(value.to_lowercase().as_str(), "true" | "1" | "active");

                    if matches!(
                        name.as_str(),
                        "IsMotion" | "State" | "IsInside" | "Value" | "IsAlert"
                    ) {
                        if let Some(topic) = &current_topic {
                            let is_trigger = TRIGGER_TOPICS.iter().any(|t| topic.contains(t));

                            if is_trigger {
                                events.push(OnvifEvent {
                                    topic: topic.clone(),
                                    is_active,
                                });
                            } else {
                                warn!("[{camera_id}] Unknown ONVIF topic (not triggering): {topic}");
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(e)) if in_topic => {
                current_topic = Some(e.unescape().unwrap_or_default().trim().to_string());
                in_topic = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("XML parse error: {e}"),
            _ => {}
        }
    }

    Ok(events)
}

fn extract_text(xml: &str, tag: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut in_tag = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if local_name(&e.name()) == tag => {
                in_tag = true;
            }
            Ok(Event::Text(e)) if in_tag => {
                return Some(e.unescape().ok()?.trim().to_string());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {
                in_tag = false;
            }
        }
    }
    None
}

fn local_name(name: &quick_xml::name::QName) -> String {
    let bytes = name.local_name().as_ref().to_vec();
    String::from_utf8_lossy(&bytes).to_string()
}

fn attr_value(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == key)
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.to_string())
}
