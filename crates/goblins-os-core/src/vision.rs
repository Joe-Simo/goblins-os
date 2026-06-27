//! Visual Look Up substrate (on-device VLM identify).
//!
//! The macOS "Visual Look Up" altitude: identify the subject of an image with a
//! LOCAL vision-language model, returned as a structured identification card. Like
//! `voice.rs`/`resident.rs`, this ships the capability gate + the request/response
//! codec and relays only to a loopback runtime — no model is bundled (zero new
//! packages), so it honest-gates to "add a vision model" until the user configures
//! one. The branded region-capture surface is the deliberate GTK follow-up.

use std::time::Duration;

use axum::http::StatusCode;
use axum::Json;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

const RUNTIME_URL_ENV: &str = "GOBLINS_OS_VISION_RUNTIME_URL";
const MODEL_ENV: &str = "GOBLINS_OS_VISION_MODEL";
const DEFAULT_MODEL: &str = "llava";

#[derive(Serialize)]
pub struct VisionStatus {
    source: &'static str,
    /// Whether a loopback vision runtime is configured to identify images.
    runtime_configured: bool,
    detail: String,
}

#[derive(Deserialize)]
pub struct VisualLookupRequest {
    image_path: String,
    #[serde(default)]
    hint: Option<String>,
}

#[derive(Serialize)]
pub struct Identification {
    name: String,
    category: String,
    confidence: String,
    description: String,
    follow_ups: Vec<String>,
}

#[derive(Serialize)]
pub struct VisualLookupOutcome {
    ok: bool,
    identification: Option<Identification>,
    detail: String,
}

pub async fn vision_status() -> Json<VisionStatus> {
    let runtime_configured = runtime_url().is_some();
    Json(VisionStatus {
        source: "goblins-os-core",
        runtime_configured,
        detail: if runtime_configured {
            "Visual Look Up is ready. Capture a region to identify it.".to_string()
        } else {
            "Visual Look Up needs a local vision model. Add one to identify images.".to_string()
        },
    })
}

pub async fn visual_lookup(
    Json(request): Json<VisualLookupRequest>,
) -> (StatusCode, Json<VisualLookupOutcome>) {
    let Some(url) = runtime_url() else {
        return (
            StatusCode::OK,
            Json(VisualLookupOutcome {
                ok: false,
                identification: None,
                detail: "Visual Look Up needs a local vision model. Add one to identify images."
                    .to_string(),
            }),
        );
    };

    let image = match std::fs::read(&request.image_path) {
        Ok(bytes) => STANDARD.encode(bytes),
        Err(_) => {
            return (
                StatusCode::OK,
                Json(VisualLookupOutcome {
                    ok: false,
                    identification: None,
                    detail: "The image to identify could not be read.".to_string(),
                }),
            )
        }
    };

    match identify_via_runtime(&url, &request.hint, &image) {
        Ok(reply) => (
            StatusCode::OK,
            Json(VisualLookupOutcome {
                ok: true,
                identification: Some(parse_identification(&reply)),
                detail: "Identified on-device.".to_string(),
            }),
        ),
        Err(detail) => (
            StatusCode::OK,
            Json(VisualLookupOutcome {
                ok: false,
                identification: None,
                detail: detail.to_string(),
            }),
        ),
    }
}

fn identify_via_runtime(
    url: &str,
    hint: &Option<String>,
    image_b64: &str,
) -> Result<String, &'static str> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(90))
        .build();
    let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
    let model = std::env::var(MODEL_ENV).unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let response = agent
        .post(&endpoint)
        .send_json(ureq::json!({
            "model": model,
            "prompt": vision_prompt(hint),
            "images": [image_b64],
            "stream": false,
        }))
        .map_err(|_| "The vision model did not respond.")?;

    #[derive(Deserialize)]
    struct GenerateReply {
        response: String,
    }
    let reply: GenerateReply = response
        .into_json()
        .map_err(|_| "The vision model returned an unreadable response.")?;
    Ok(reply.response)
}

fn vision_prompt(hint: &Option<String>) -> String {
    let base = "Identify the main subject of this image. Reply ONLY with JSON: \
        {\"name\":\"\",\"category\":\"plant|animal|landmark|artwork|product|food|other\",\
        \"confidence\":\"high|medium|low\",\"description\":\"2-3 sentences\",\"follow_ups\":[]}.";
    match hint.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(hint) => format!("{base} The user adds: {hint}"),
        None => base.to_string(),
    }
}

/// Parse the model's reply into an identification card. The model is asked for JSON
/// but may wrap it in prose/fences, so the first `{...}` object is extracted; a
/// non-JSON reply degrades to an honest description. Pure + unit-tested.
fn parse_identification(reply: &str) -> Identification {
    #[derive(Deserialize)]
    struct Raw {
        name: Option<String>,
        category: Option<String>,
        confidence: Option<String>,
        description: Option<String>,
        follow_ups: Option<Vec<String>>,
    }

    if let Some(json) = extract_json_object(reply) {
        if let Ok(raw) = serde_json::from_str::<Raw>(&json) {
            return Identification {
                name: non_empty(raw.name).unwrap_or_else(|| "Unidentified".to_string()),
                category: raw.category.unwrap_or_default(),
                confidence: raw.confidence.unwrap_or_default(),
                description: raw.description.unwrap_or_default(),
                follow_ups: raw
                    .follow_ups
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(default_follow_ups),
            };
        }
    }

    Identification {
        name: "Unidentified".to_string(),
        category: String::new(),
        confidence: String::new(),
        description: reply.trim().chars().take(600).collect(),
        follow_ups: default_follow_ups(),
    }
}

/// The first balanced-ish `{...}` slice (first `{` to last `}`). Pure + unit-tested.
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(text[start..=end].to_string())
    } else {
        None
    }
}

fn default_follow_ups() -> Vec<String> {
    vec![
        "Search the web".to_string(),
        "Ask Goblin about this".to_string(),
        "Copy name".to_string(),
    ]
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// The configured loopback vision runtime URL, or None. Only `127.0.0.1`/`localhost`/
/// `::1` http(s) URLs are accepted — Visual Look Up never leaves the device.
fn runtime_url() -> Option<String> {
    let value = std::env::var(RUNTIME_URL_ENV).ok()?;
    is_loopback_url(&value).then_some(value)
}

/// Accept only loopback http(s) URLs. Pure + unit-tested.
fn is_loopback_url(value: &str) -> bool {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"));
    let Some(rest) = rest else {
        return false;
    };
    // A bracketed IPv6 literal ([::1]) holds the host up to the closing bracket;
    // otherwise the host runs to the first ':' (port) or '/' (path).
    let host = if let Some(after_bracket) = rest.strip_prefix('[') {
        after_bracket.split(']').next().unwrap_or("")
    } else {
        rest.split(['/', ':']).next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::{extract_json_object, is_loopback_url, parse_identification};

    #[test]
    fn loopback_urls_only() {
        assert!(is_loopback_url("http://127.0.0.1:11434"));
        assert!(is_loopback_url("http://localhost:8080/api"));
        assert!(is_loopback_url("https://[::1]:443"));
        assert!(!is_loopback_url("http://example.com"));
        assert!(!is_loopback_url("http://10.0.0.5:11434"));
        assert!(!is_loopback_url("ftp://localhost"));
    }

    #[test]
    fn extracts_json_object_from_prose() {
        assert_eq!(
            extract_json_object("Here you go: {\"name\":\"Rose\"} hope that helps"),
            Some("{\"name\":\"Rose\"}".to_string())
        );
        assert_eq!(extract_json_object("no json here"), None);
    }

    #[test]
    fn parses_identification_json_and_falls_back() {
        let card = parse_identification(
            r#"{"name":"Rose","category":"plant","confidence":"high","description":"A rose.","follow_ups":["A"]}"#,
        );
        assert_eq!(card.name, "Rose");
        assert_eq!(card.category, "plant");
        assert_eq!(card.follow_ups, vec!["A".to_string()]);

        // Non-JSON reply → honest fallback with default follow-ups.
        let fallback = parse_identification("I think it's a small bird.");
        assert_eq!(fallback.name, "Unidentified");
        assert!(fallback.description.contains("small bird"));
        assert_eq!(fallback.follow_ups.len(), 3);
    }
}
