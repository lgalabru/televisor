//! Credential redaction for logs and error messages.
//!
//! Xtream panels embed the username/password in both API URLs
//! (`player_api.php?username=U&password=P`) and playback URLs
//! (`/live/USER/PASS/ID.m3u8`), so every URL that reaches a log line or a
//! `Display` impl must pass through [`redact_url`] first.

use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

const MASK: &str = "redacted";

/// A secret string whose `Debug`/`Display` output is always masked.
///
/// Serde passes the inner value through untouched (so it round-trips in
/// config files); reading it in code requires an explicit [`expose`] call.
///
/// [`expose`]: Redacted::expose
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Redacted(String);

impl Redacted {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// Access the secret. Call sites are the audit surface — keep them few.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl fmt::Display for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl From<String> for Redacted {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Render a URL with credentials masked: userinfo, `username`/`password`/
/// `token` query parameters, and the two path segments following
/// `/live/`, `/movie/`, or `/series/` (the Xtream credential-in-path layout).
pub fn redact_url(url: &Url) -> String {
    let mut safe = url.clone();

    if !safe.username().is_empty() {
        let _ = safe.set_username(MASK);
    }
    if safe.password().is_some() {
        let _ = safe.set_password(Some(MASK));
    }

    if url.query().is_some() {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in url.query_pairs() {
            let sensitive = matches!(
                key.to_ascii_lowercase().as_str(),
                "username" | "password" | "token"
            );
            query.append_pair(&key, if sensitive { MASK } else { &value });
        }
        safe.set_query(Some(&query.finish()));
    }

    if let Some(segments) = url.path_segments() {
        let mut segments: Vec<String> = segments.map(str::to_string).collect();
        if segments.len() >= 4 && matches!(segments[0].as_str(), "live" | "movie" | "series") {
            segments[1] = MASK.to_string();
            segments[2] = MASK.to_string();
            if let Ok(mut path) = safe.path_segments_mut() {
                path.clear().extend(&segments);
            }
        }
    }

    safe.to_string()
}

/// [`redact_url`] for strings. Unparseable inputs (e.g. local file paths
/// handed to `televisor open`) are returned unchanged — credential-bearing URLs
/// built by this crate are always absolute and parseable.
pub fn redact_url_str(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) => redact_url(&parsed),
        Err(_) => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_never_prints_secret() {
        let secret = Redacted::new("hunter2");
        assert!(!format!("{secret}").contains("hunter2"));
        assert!(!format!("{secret:?}").contains("hunter2"));
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn redacted_serializes_transparently() {
        let secret = Redacted::new("hunter2");
        assert_eq!(serde_json::to_string(&secret).unwrap(), "\"hunter2\"");
        let back: Redacted = serde_json::from_str("\"hunter2\"").unwrap();
        assert_eq!(back, secret);
    }

    #[test]
    fn redacts_query_credentials() {
        let url = Url::parse(
            "http://host.example/player_api.php?username=alice&password=hunter2&action=get_live_streams",
        )
        .unwrap();
        let safe = redact_url(&url);
        assert!(!safe.contains("alice"));
        assert!(!safe.contains("hunter2"));
        assert!(safe.contains("action=get_live_streams"));
    }

    #[test]
    fn redacts_live_path_credentials() {
        let url = Url::parse("http://host.example/live/alice/hunter2/2012600.m3u8").unwrap();
        let safe = redact_url(&url);
        assert!(!safe.contains("alice"));
        assert!(!safe.contains("hunter2"));
        assert!(safe.contains("2012600.m3u8"));
    }

    #[test]
    fn redacts_userinfo() {
        let url = Url::parse("http://alice:hunter2@host.example/stream").unwrap();
        let safe = redact_url(&url);
        assert!(!safe.contains("alice"));
        assert!(!safe.contains("hunter2"));
    }

    #[test]
    fn leaves_plain_urls_and_paths_alone() {
        assert_eq!(
            redact_url_str("https://example.com/live.m3u8"),
            "https://example.com/live.m3u8"
        );
        assert_eq!(redact_url_str("/tmp/video.mp4"), "/tmp/video.mp4");
    }
}
