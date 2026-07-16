//! Lenient deserializers for Xtream panel JSON.
//!
//! Panels serialize the same field as a number on one install and a string on
//! the next (`"port": "80"` vs `stream_id: 2012667`), and use `null` or `""`
//! interchangeably for "absent". Every numeric/boolean field in
//! [`super::types`] goes through one of these helpers.

use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum Flex {
    Num(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl Flex {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Flex::Num(n) => Some(*n),
            Flex::Float(f) => Some(*f as i64),
            Flex::Bool(b) => Some(i64::from(*b)),
            Flex::Str(s) => {
                let s = s.trim();
                s.parse::<i64>().ok().or_else(|| {
                    // Some panels emit epoch timestamps as floats-in-strings.
                    s.parse::<f64>().ok().map(|f| f as i64)
                })
            }
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Flex::Num(n) => *n != 0,
            Flex::Float(f) => *f != 0.0,
            Flex::Bool(b) => *b,
            Flex::Str(s) => matches!(s.trim(), "1" | "true" | "TRUE" | "True"),
        }
    }

    fn as_string(&self) -> String {
        match self {
            Flex::Num(n) => n.to_string(),
            Flex::Float(f) => f.to_string(),
            Flex::Bool(b) => b.to_string(),
            Flex::Str(s) => s.clone(),
        }
    }
}

/// Number, numeric string, or bool → `i64`. `null`, `""`, or garbage → `None`.
pub fn opt_i64_flex<'de, D: Deserializer<'de>>(de: D) -> Result<Option<i64>, D::Error> {
    let value = Option::<Flex>::deserialize(de)?;
    Ok(value.as_ref().and_then(Flex::as_i64))
}

/// Number or numeric string → `u64`. Errors on missing/negative/garbage —
/// used for identifiers that must exist (e.g. `stream_id`).
pub fn u64_flex<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    let value = Flex::deserialize(de)?;
    value
        .as_i64()
        .and_then(|n| u64::try_from(n).ok())
        .ok_or_else(|| {
            serde::de::Error::custom("expected a non-negative integer or numeric string")
        })
}

/// `0`/`1`, `"0"`/`"1"`, `true`/`false` → `bool`. `null` → `false`.
/// Pair with `#[serde(default)]` so a missing field is also `false`.
pub fn bool_flex<'de, D: Deserializer<'de>>(de: D) -> Result<bool, D::Error> {
    let value = Option::<Flex>::deserialize(de)?;
    Ok(value.as_ref().map(Flex::as_bool).unwrap_or(false))
}

/// Number or string → `String`. `null` or `""` → `None`.
pub fn opt_string_flex<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    let value = Option::<Flex>::deserialize(de)?;
    Ok(value
        .as_ref()
        .map(Flex::as_string)
        .filter(|s| !s.trim().is_empty()))
}

/// EPG text fields (`title`, `description`) are base64-encoded on most
/// panels but sent as plain text on some — decode when it round-trips to
/// valid UTF-8, otherwise pass the raw value through.
pub fn opt_b64_string<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    use base64::Engine;
    let Some(raw) = opt_string_flex(de)? else {
        return Ok(None);
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|s| !s.trim().is_empty());
    Ok(Some(decoded.unwrap_or(raw)))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Probe {
        #[serde(default, deserialize_with = "super::opt_i64_flex")]
        opt_num: Option<i64>,
        #[serde(default, deserialize_with = "super::bool_flex")]
        flag: bool,
        #[serde(default, deserialize_with = "super::opt_string_flex")]
        text: Option<String>,
    }

    fn probe(json: &str) -> Probe {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn opt_i64_accepts_number_string_null_and_garbage() {
        assert_eq!(probe(r#"{"opt_num": 42}"#).opt_num, Some(42));
        assert_eq!(probe(r#"{"opt_num": "42"}"#).opt_num, Some(42));
        assert_eq!(
            probe(r#"{"opt_num": "1784305682"}"#).opt_num,
            Some(1784305682)
        );
        assert_eq!(probe(r#"{"opt_num": null}"#).opt_num, None);
        assert_eq!(probe(r#"{"opt_num": ""}"#).opt_num, None);
        assert_eq!(probe(r#"{"opt_num": "abc"}"#).opt_num, None);
        assert_eq!(probe(r#"{}"#).opt_num, None);
    }

    #[test]
    fn bool_accepts_every_panel_spelling() {
        assert!(probe(r#"{"flag": 1}"#).flag);
        assert!(probe(r#"{"flag": "1"}"#).flag);
        assert!(probe(r#"{"flag": true}"#).flag);
        assert!(!probe(r#"{"flag": 0}"#).flag);
        assert!(!probe(r#"{"flag": "0"}"#).flag);
        assert!(!probe(r#"{"flag": false}"#).flag);
        assert!(!probe(r#"{"flag": null}"#).flag);
        assert!(!probe(r#"{"flag": ""}"#).flag);
        assert!(!probe(r#"{}"#).flag);
    }

    #[test]
    fn opt_string_normalizes_numbers_and_drops_empties() {
        assert_eq!(probe(r#"{"text": "abc"}"#).text.as_deref(), Some("abc"));
        assert_eq!(probe(r#"{"text": 2343}"#).text.as_deref(), Some("2343"));
        assert_eq!(probe(r#"{"text": ""}"#).text, None);
        assert_eq!(probe(r#"{"text": null}"#).text, None);
        assert_eq!(probe(r#"{}"#).text, None);
    }

    #[test]
    fn b64_decodes_encoded_and_passes_plain_through() {
        #[derive(Deserialize)]
        struct Epg {
            #[serde(default, deserialize_with = "super::opt_b64_string")]
            title: Option<String>,
        }
        // "Live: The Herd" base64-encoded, as real panels send it.
        let enc: Epg = serde_json::from_str(r#"{"title": "TGl2ZTogVGhlIEhlcmQ="}"#).unwrap();
        assert_eq!(enc.title.as_deref(), Some("Live: The Herd"));
        // Some panels send plain text — passed through unchanged. ("Match of
        // the Day!" is not valid base64 thanks to spaces/punctuation.)
        let plain: Epg = serde_json::from_str(r#"{"title": "Match of the Day!"}"#).unwrap();
        assert_eq!(plain.title.as_deref(), Some("Match of the Day!"));
        let missing: Epg = serde_json::from_str(r#"{"title": null}"#).unwrap();
        assert_eq!(missing.title, None);
    }

    #[test]
    fn u64_flex_rejects_negative_and_garbage() {
        #[derive(Deserialize)]
        struct Id {
            #[serde(deserialize_with = "super::u64_flex")]
            id: u64,
        }
        let ok: Id = serde_json::from_str(r#"{"id": "2012667"}"#).unwrap();
        assert_eq!(ok.id, 2012667);
        let ok: Id = serde_json::from_str(r#"{"id": 7}"#).unwrap();
        assert_eq!(ok.id, 7);
        assert!(serde_json::from_str::<Id>(r#"{"id": -1}"#).is_err());
        assert!(serde_json::from_str::<Id>(r#"{"id": "x"}"#).is_err());
    }
}
