//! Payment-gated responses (HTTP 402).
//!
//! When a gateway fronts the panel with a pay 402 gate, requests come back
//! `402 Payment Required` with a challenge. televisor never pays — pay's
//! payer-proxy does — but it captures the challenge so the failure is legible
//! to a human or an agent (who can hand it to pay). Nothing here is secret:
//! a 402 fires *before* any upstream credential is involved.

use serde::Serialize;

/// A parsed 402 challenge. Best-effort: any field the gateway didn't advertise
/// is `None`/empty, and `raw` always holds the original body for agents.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PaymentChallenge {
    /// Advertised price in USD, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_usd: Option<f64>,
    /// Accepted payment schemes, e.g. `["mpp"]` or `["x402"]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub schemes: Vec<String>,
    /// Recipient wallet (from the `WWW-Authenticate` challenge, best-effort).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    /// Settlement network, e.g. `mainnet`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Human-readable message from the gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// ISO-8601 expiry of the challenge, if advertised.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Raw response body (capped), for tooling and debugging.
    pub raw: String,
}

const RAW_CAP: usize = 4096;

impl PaymentChallenge {
    /// Parse from the `WWW-Authenticate` header (if present) and the response
    /// body. Tolerant of shapes it doesn't recognize.
    pub fn parse(www_authenticate: Option<&str>, body: &str) -> Self {
        let mut challenge = PaymentChallenge {
            amount_usd: None,
            schemes: Vec::new(),
            recipient: None,
            network: None,
            message: None,
            expires: None,
            raw: body.chars().take(RAW_CAP).collect(),
        };

        // Body: pay emits `{ message, payment: { schemes }, pricing: {
        // dimensions: [{ price_usd }] } }`.
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            challenge.message = json
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string);
            if let Some(schemes) = json.pointer("/payment/schemes").and_then(|s| s.as_array()) {
                challenge.schemes = schemes
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
            }
            challenge.amount_usd = json
                .pointer("/pricing/dimensions/0/price_usd")
                .and_then(|p| p.as_f64());
        }

        // WWW-Authenticate: `Payment …, request="<b64 json>", expires="…"`.
        if let Some(header) = www_authenticate {
            challenge.expires = quoted_value(header, "expires");
            if let Some(request_b64) = quoted_value(header, "request")
                && let Some(decoded) = decode_b64_json(&request_b64)
            {
                challenge.recipient = decoded
                    .get("recipient")
                    .and_then(|r| r.as_str())
                    .map(str::to_string);
                challenge.network = decoded
                    .pointer("/methodDetails/network")
                    .and_then(|n| n.as_str())
                    .map(str::to_string);
            }
        }

        challenge
    }

    /// One-line human summary (no secrets — a 402 predates any credential).
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(usd) = self.amount_usd {
            parts.push(format!("${usd}"));
        }
        if !self.schemes.is_empty() {
            parts.push(format!("via {}", self.schemes.join("/")));
        }
        if let Some(recipient) = &self.recipient {
            parts.push(format!("to {recipient}"));
        }
        if parts.is_empty() {
            "payment required".to_string()
        } else {
            parts.join(" ")
        }
    }
}

/// Extract a `key="value"` value from an HTTP auth header.
fn quoted_value(header: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = header.find(&needle)? + needle.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Decode a base64 (standard, then URL-safe) string into JSON.
fn decode_b64_json(value: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value.trim()))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Errors that may wrap a [`PaymentChallenge`] somewhere in their chain.
pub trait PaymentChallengeExt {
    fn payment_challenge(&self) -> Option<&PaymentChallenge>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured verbatim from a live pay --sandbox gate.
    const WWW_AUTH: &str = r#"Payment id="W0N6bl97", realm="App", method="solana", intent="charge", request="eyJhbW91bnQiOiI1MDAwMCIsInJlY2lwaWVudCI6IkY4MkoiLCJtZXRob2REZXRhaWxzIjp7Im5ldHdvcmsiOiJsb2NhbG5ldCJ9fQ==", description="probe", expires="2026-07-17T02:27:27.470989Z""#;
    const BODY: &str = r#"{"endpoint":{"method":"GET","path":"player_api.php"},"error":"payment_required","message":"This endpoint requires payment.","payment":{"schemes":["mpp"]},"pricing":{"dimensions":[{"direction":"usage","price_usd":0.05,"scale":1,"unit":"requests"}]}}"#;

    #[test]
    fn parses_real_402() {
        let c = PaymentChallenge::parse(Some(WWW_AUTH), BODY);
        assert_eq!(c.amount_usd, Some(0.05));
        assert_eq!(c.schemes, vec!["mpp"]);
        assert_eq!(
            c.message.as_deref(),
            Some("This endpoint requires payment.")
        );
        assert_eq!(c.expires.as_deref(), Some("2026-07-17T02:27:27.470989Z"));
        // Decoded from the base64 `request` field.
        assert_eq!(c.recipient.as_deref(), Some("F82J"));
        assert_eq!(c.network.as_deref(), Some("localnet"));
        assert!(c.summary().contains("$0.05"));
        assert!(c.summary().contains("mpp"));
    }

    #[test]
    fn tolerates_missing_pieces() {
        let c = PaymentChallenge::parse(None, "not json");
        assert_eq!(c.amount_usd, None);
        assert!(c.schemes.is_empty());
        assert_eq!(c.raw, "not json");
        assert_eq!(c.summary(), "payment required");
    }

    #[test]
    fn caps_raw_body() {
        let big = "x".repeat(RAW_CAP * 2);
        let c = PaymentChallenge::parse(None, &big);
        assert_eq!(c.raw.len(), RAW_CAP);
    }
}
