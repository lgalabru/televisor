//! DTOs for the Xtream Codes `player_api.php` JSON API.
//!
//! Field typing is deliberately lenient (see [`super::de`]) and structs
//! ignore unknown fields — panels vary wildly. `user_info.username` and
//! `user_info.password` are intentionally **not** modeled: the server echoes
//! the credentials back and leaving them out of the structs makes it
//! impossible to leak them through `Debug`, logs, or `--json` output.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use super::de;

/// Numeric stream identifier (`stream_id`), tolerant of string encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct StreamId(pub u64);

impl<'de> Deserialize<'de> for StreamId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de::u64_flex(de).map(StreamId)
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Category identifier. Kept as a string so it round-trips byte-for-byte
/// into the `&category_id=` query parameter regardless of panel encoding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct CategoryId(pub String);

impl<'de> Deserialize<'de> for CategoryId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de::opt_string_flex(de)?
            .map(CategoryId)
            .ok_or_else(|| serde::de::Error::custom("expected a category id"))
    }
}

impl fmt::Display for CategoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Response of `player_api.php` with no action: account + server metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub user_info: UserInfo,
    #[serde(default)]
    pub server_info: Option<ServerInfo>,
}

/// Account state. On failed auth the server returns HTTP 200 with
/// `{"user_info": {"auth": 0}}` and most fields absent — everything except
/// `auth` must tolerate being missing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(default, deserialize_with = "de::bool_flex")]
    pub auth: bool,
    /// "Active", "Expired", "Banned", "Disabled" — free-form per panel.
    #[serde(default)]
    pub status: Option<String>,
    /// Expiry as epoch seconds; `None` means unlimited (panels send `null`).
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub exp_date: Option<i64>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub created_at: Option<i64>,
    #[serde(default, deserialize_with = "de::bool_flex")]
    pub is_trial: bool,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub active_cons: Option<i64>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub max_connections: Option<i64>,
    #[serde(default)]
    pub allowed_output_formats: Vec<String>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerInfo {
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub port: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub server_protocol: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub timezone: Option<String>,
}

/// One entry of `get_live_categories`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub category_id: CategoryId,
    pub category_name: String,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub parent_id: Option<i64>,
}

/// One entry of `get_live_streams`, slimmed to what v1 uses. Unknown fields
/// (icons, archive flags, …) are ignored on purpose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveStream {
    pub stream_id: StreamId,
    pub name: String,
    #[serde(default)]
    pub category_id: Option<CategoryId>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub epg_channel_id: Option<String>,
    #[serde(default, deserialize_with = "de::bool_flex")]
    pub is_adult: bool,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub added: Option<i64>,
}

/// One entry of `get_vod_streams`. `container_extension` (mkv/mp4/…) is
/// required to build the `/movie/` playback URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VodStream {
    pub stream_id: StreamId,
    pub name: String,
    #[serde(default)]
    pub category_id: Option<CategoryId>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub container_extension: Option<String>,
    #[serde(default, deserialize_with = "de::bool_flex")]
    pub is_adult: bool,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub added: Option<i64>,
}

/// One entry of `get_series` — a show, not a playable stream. Episodes come
/// from `get_series_info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    #[serde(deserialize_with = "de::u64_flex")]
    pub series_id: u64,
    pub name: String,
    #[serde(default)]
    pub category_id: Option<CategoryId>,
}

/// Response of `get_series_info&series_id=X`, slimmed to the episode list.
#[derive(Debug, Clone, Deserialize)]
pub struct SeriesInfo {
    #[serde(default)]
    episodes: Option<EpisodesRepr>,
}

/// Panels disagree on the `episodes` shape: most key an object by season
/// number ("1" → [episodes]), some send an array of per-season arrays.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EpisodesRepr {
    Map(std::collections::BTreeMap<String, Vec<Episode>>),
    List(Vec<Vec<Episode>>),
}

impl SeriesInfo {
    /// All episodes flattened, sorted by (season, episode number).
    pub fn episodes(&self) -> Vec<Episode> {
        let mut episodes: Vec<Episode> = match &self.episodes {
            Some(EpisodesRepr::Map(map)) => map.values().flatten().cloned().collect(),
            Some(EpisodesRepr::List(list)) => list.iter().flatten().cloned().collect(),
            None => Vec::new(),
        };
        episodes.sort_by_key(|e| (e.season, e.episode_num, e.id));
        episodes
    }
}

/// One playable episode. `id` goes into the `/series/` playback URL
/// (the panel sends it as a string).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    #[serde(deserialize_with = "de::u64_flex")]
    pub id: u64,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_flex")]
    pub container_extension: Option<String>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub season: Option<i64>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub episode_num: Option<i64>,
}

/// Wrapper of `get_short_epg` / `get_simple_data_table` responses.
#[derive(Debug, Clone, Deserialize)]
pub struct EpgResponse {
    #[serde(default)]
    pub epg_listings: Vec<EpgListing>,
}

/// One EPG programme. Panels base64-encode `title`/`description`
/// (see [`de::opt_b64_string`]); timestamps are epoch seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgListing {
    #[serde(default, deserialize_with = "de::opt_b64_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de::opt_b64_string")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub start_timestamp: Option<i64>,
    #[serde(default, deserialize_with = "de::opt_i64_flex")]
    pub stop_timestamp: Option<i64>,
}

impl EpgListing {
    /// Whether the programme is airing at `now` (epoch seconds).
    pub fn is_on_air(&self, now: i64) -> bool {
        match (self.start_timestamp, self.stop_timestamp) {
            (Some(start), Some(stop)) => start <= now && now < stop,
            _ => false,
        }
    }
}

/// Container extension for live playback URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamFormat {
    #[default]
    M3u8,
    Ts,
}

impl StreamFormat {
    pub fn ext(self) -> &'static str {
        match self {
            StreamFormat::M3u8 => "m3u8",
            StreamFormat::Ts => "ts",
        }
    }
}

impl FromStr for StreamFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "m3u8" | "hls" => Ok(StreamFormat::M3u8),
            "ts" | "mpegts" => Ok(StreamFormat::Ts),
            other => Err(format!("unknown stream format '{other}' — use m3u8 or ts")),
        }
    }
}

impl fmt::Display for StreamFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.ext())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_account_summary() {
        let summary: AccountSummary =
            serde_json::from_str(include_str!("fixtures/user_info.json")).unwrap();
        let user = &summary.user_info;
        assert!(user.auth);
        assert_eq!(user.status.as_deref(), Some("Active"));
        assert_eq!(user.exp_date, Some(1784305682)); // string epoch in the fixture
        assert!(!user.is_trial); // "0"
        assert_eq!(user.max_connections, Some(1)); // "1"
        assert_eq!(user.allowed_output_formats, vec!["m3u8", "ts", "rtmp"]);
        let server = summary.server_info.as_ref().unwrap();
        assert_eq!(server.port.as_deref(), Some("80"));
        // The fixture deliberately contains username/password keys — they
        // must not survive into any serialized form of the struct.
        let round_trip = serde_json::to_string(&summary).unwrap();
        assert!(!round_trip.contains("fixture-user"));
        assert!(!round_trip.contains("fixture-hunter2"));
    }

    #[test]
    fn parses_auth_failure_with_missing_fields() {
        let summary: AccountSummary =
            serde_json::from_str(include_str!("fixtures/user_info_auth_failed.json")).unwrap();
        assert!(!summary.user_info.auth);
        assert_eq!(summary.user_info.exp_date, None);
        assert!(summary.server_info.is_none());
    }

    #[test]
    fn null_exp_date_means_unlimited() {
        let summary: AccountSummary =
            serde_json::from_str(include_str!("fixtures/user_info_unlimited.json")).unwrap();
        assert!(summary.user_info.auth);
        assert_eq!(summary.user_info.exp_date, None);
    }

    #[test]
    fn parses_real_categories() {
        let categories: Vec<Category> =
            serde_json::from_str(include_str!("fixtures/live_categories.json")).unwrap();
        assert_eq!(categories.len(), 3);
        assert_eq!(categories[0].category_id, CategoryId("2343".into())); // string id
        assert_eq!(categories[2].category_id, CategoryId("17".into())); // numeric id
        assert_eq!(categories[0].parent_id, Some(0));
    }

    #[test]
    fn parses_real_live_streams() {
        let streams: Vec<LiveStream> =
            serde_json::from_str(include_str!("fixtures/live_streams.json")).unwrap();
        assert_eq!(streams.len(), 3);
        assert_eq!(streams[0].stream_id, StreamId(2012667));
        assert_eq!(streams[0].epg_channel_id, None); // "" in the fixture
        assert_eq!(streams[1].epg_channel_id.as_deref(), Some("foxsports1.us"));
        assert_eq!(streams[2].stream_id, StreamId(555)); // string-encoded id
        assert!(streams[2].is_adult); // "1"
        assert_eq!(
            streams[0].category_id,
            Some(CategoryId("2343".into())) // string category_id
        );
        assert_eq!(
            streams[2].category_id,
            Some(CategoryId("99".into())) // numeric category_id
        );
    }

    #[test]
    fn parses_real_vod_streams() {
        let movies: Vec<VodStream> =
            serde_json::from_str(include_str!("fixtures/vod_streams.json")).unwrap();
        assert_eq!(movies.len(), 3);
        assert_eq!(movies[0].stream_id, StreamId(2024998));
        assert_eq!(movies[0].container_extension.as_deref(), Some("mkv"));
        assert_eq!(movies[1].stream_id, StreamId(31341)); // string id
        assert_eq!(movies[1].category_id, Some(CategoryId("92".into()))); // numeric category
        assert!(movies[2].is_adult); // "1"
        assert_eq!(movies[2].container_extension, None); // null
    }

    #[test]
    fn parses_real_series_list() {
        let series: Vec<Series> =
            serde_json::from_str(include_str!("fixtures/series.json")).unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].series_id, 48173);
        assert_eq!(series[1].series_id, 555); // string id
    }

    #[test]
    fn series_info_episodes_flatten_and_sort() {
        let info: SeriesInfo =
            serde_json::from_str(include_str!("fixtures/series_info.json")).unwrap();
        let episodes = info.episodes();
        assert_eq!(episodes.len(), 3);
        // Sorted by (season, episode) even though season 2 came first.
        assert_eq!(episodes[0].id, 2066378);
        assert_eq!(episodes[0].season, Some(1));
        assert_eq!(episodes[1].episode_num, Some(2)); // "2" string
        assert_eq!(episodes[1].season, Some(1)); // "1" string
        assert_eq!(episodes[2].season, Some(2));
    }

    #[test]
    fn series_info_accepts_array_of_arrays_and_null() {
        let info: SeriesInfo = serde_json::from_str(
            r#"{"episodes": [[{"id": "10", "episode_num": 1, "season": 1}], [{"id": 20, "episode_num": 1, "season": 2}]]}"#,
        )
        .unwrap();
        let episodes = info.episodes();
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[1].id, 20);

        let empty: SeriesInfo = serde_json::from_str(r#"{"episodes": null}"#).unwrap();
        assert!(empty.episodes().is_empty());
        let missing: SeriesInfo = serde_json::from_str(r#"{"info": {}}"#).unwrap();
        assert!(missing.episodes().is_empty());
    }

    #[test]
    fn parses_real_short_epg_with_base64_fields() {
        let response: EpgResponse =
            serde_json::from_str(include_str!("fixtures/short_epg.json")).unwrap();
        let listings = &response.epg_listings;
        assert_eq!(listings.len(), 2);
        assert_eq!(
            listings[0].title.as_deref(),
            Some("Live: The Herd With Colin Cowherd")
        );
        assert_eq!(
            listings[0].description.as_deref(),
            Some("A thought-provoking journey.")
        );
        assert_eq!(listings[0].start_timestamp, Some(1784224800)); // string epoch
        assert_eq!(listings[1].description, None);
        // On-air check straddles the first listing's window.
        assert!(listings[0].is_on_air(1784230000));
        assert!(!listings[0].is_on_air(1784235600)); // stop is exclusive
        assert!(!listings[1].is_on_air(1784230000));
    }

    #[test]
    fn stream_format_parses_and_displays() {
        assert_eq!("m3u8".parse::<StreamFormat>().unwrap(), StreamFormat::M3u8);
        assert_eq!("TS".parse::<StreamFormat>().unwrap(), StreamFormat::Ts);
        assert!("avi".parse::<StreamFormat>().is_err());
        assert_eq!(StreamFormat::M3u8.to_string(), "m3u8");
    }
}
