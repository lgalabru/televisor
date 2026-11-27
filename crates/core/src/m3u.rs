//! Plain M3U playlist accounts.
//!
//! Some providers hand out only a playlist URL (`get.php?...&type=m3u_plus`)
//! with no working JSON API. This module fetches and parses that playlist
//! into the same [`LiveStream`] shape the Xtream path produces, so search,
//! caching, and playback all work unchanged: entries get a stable synthetic
//! `stream_id` (FNV-1a of their URL) and carry their playback URL in
//! `direct_source`. Only live TV is available in this mode — VOD, series,
//! EPG, and account status need the JSON API.

use tracing::debug;
use url::Url;

use crate::redact::{Redacted, redact_url_str};
use crate::xtream::{Category, CategoryId, DEFAULT_USER_AGENT, LiveStream, StreamId, XtreamError};

const UNCATEGORIZED: &str = "Uncategorized";

/// Fetches and parses a provider's M3U playlist.
pub struct M3uSource {
    http: reqwest::Client,
    /// Full playlist URL — typically embeds the credentials.
    url: Redacted,
    account: String,
}

impl M3uSource {
    pub fn new(
        account_name: &str,
        url: Redacted,
        user_agent: Option<String>,
    ) -> Result<Self, XtreamError> {
        Url::parse(url.expose()).map_err(|_| XtreamError::InvalidHost {
            account: account_name.to_string(),
        })?;
        let http = reqwest::Client::builder()
            .user_agent(user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()))
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            url,
            account: account_name.to_string(),
        })
    }

    pub fn account_name(&self) -> &str {
        &self.account
    }

    pub async fn live_streams(&self) -> Result<Vec<LiveStream>, XtreamError> {
        debug!(url = %redact_url_str(self.url.expose()), account = %self.account, "Fetching M3U playlist");
        let response = self.http.get(self.url.expose()).send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::PAYMENT_REQUIRED {
            return Err(crate::xtream::client::payment_required(response).await);
        }
        if !status.is_success() {
            return Err(XtreamError::Status {
                code: status.as_u16(),
            });
        }
        let body = response.text().await?;
        let streams = parse_m3u(&body);
        if streams.is_empty() && !body.trim_start().starts_with("#EXTM3U") {
            return Err(XtreamError::Decode {
                context: "M3U playlist",
                detail: "response is not an M3U playlist".to_string(),
            });
        }
        Ok(streams)
    }

    /// Categories derived from `group-title` attributes, in playlist order.
    pub async fn live_categories(&self) -> Result<Vec<Category>, XtreamError> {
        let streams = self.live_streams().await?;
        Ok(categories_of(&streams))
    }
}

/// Unique categories of a parsed playlist, in first-seen order.
pub fn categories_of(streams: &[LiveStream]) -> Vec<Category> {
    let mut seen = std::collections::HashSet::new();
    let mut categories = Vec::new();
    for stream in streams {
        let Some(category) = &stream.category_id else {
            continue;
        };
        if seen.insert(category.clone()) {
            categories.push(Category {
                category_id: category.clone(),
                category_name: category.0.clone(),
                parent_id: None,
            });
        }
    }
    categories
}

/// Parse `#EXTM3U` text into [`LiveStream`]s. Unknown/malformed lines are
/// skipped — provider playlists are as sloppy as their JSON.
pub fn parse_m3u(text: &str) -> Vec<LiveStream> {
    let mut streams = Vec::new();
    let mut pending: Option<Extinf> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(extinf) = line.strip_prefix("#EXTINF:") {
            let name = extinf
                .rsplit_once(',')
                .map(|(_, name)| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "(unnamed)".to_string());
            let group = attr(extinf, "group-title");
            let tvg_id = attr(extinf, "tvg-id");
            let tvg_logo = attr(extinf, "tvg-logo");
            pending = Some(Extinf {
                name,
                group,
                tvg_id,
                tvg_logo,
            });
        } else if line.starts_with('#') {
            // #EXTGRP, #EXTVLCOPT, comments — ignored.
            continue;
        } else if let Some(Extinf {
            name,
            group,
            tvg_id,
            tvg_logo,
        }) = pending.take()
        {
            let group = group.unwrap_or_else(|| UNCATEGORIZED.to_string());
            let adult = is_adult_heuristic(&name, &group);
            streams.push(LiveStream {
                // Stable across refreshes as long as the URL is stable.
                stream_id: StreamId(fnv1a64(line)),
                name,
                category_id: Some(CategoryId(group)),
                epg_channel_id: tvg_id,
                is_adult: adult,
                added: None,
                tv_archive: false,
                tv_archive_duration: None,
                direct_source: Some(line.to_string()),
                stream_icon: tvg_logo,
            });
        }
    }
    streams
}

/// The last `#EXTINF` line, waiting for the URL line that completes it.
struct Extinf {
    name: String,
    group: Option<String>,
    tvg_id: Option<String>,
    tvg_logo: Option<String>,
}

/// `key="value"` attribute lookup inside an `#EXTINF` line.
fn attr(extinf: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=\"");
    let start = extinf.find(&marker)? + marker.len();
    let rest = &extinf[start..];
    let end = rest.find('"')?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// M3U has no `is_adult` flag — approximate from the group/channel name so
/// the default matching behavior stays consistent with Xtream accounts.
fn is_adult_heuristic(name: &str, group: &str) -> bool {
    let haystack = format!("{name} {group}").to_lowercase();
    ["xxx", "adult", "porn", "18+"]
        .iter()
        .any(|marker| haystack.contains(marker))
}

/// FNV-1a 64-bit: tiny, deterministic across runs (unlike `DefaultHasher`'s
/// documented instability), collision-safe enough for a channel list.
fn fnv1a64(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = r#"#EXTM3U
#EXTINF:-1 tvg-id="FoxSports1.us" tvg-name="FS1" tvg-logo="http://img/fs1.png" group-title="US Sports",US: FOX SPORTS 1
http://host.example/live/alice/hunter2/429586.ts
#EXTVLCOPT:http-user-agent=okhttp
#EXTINF:-1 group-title="XXX Night",Spicy Channel
http://host.example/live/alice/hunter2/777.ts
#EXTINF:-1,Bare Channel, With Comma
http://host.example/live/alice/hunter2/1.ts
#EXTINF:-1 group-title="US Sports",Orphaned entry without URL
"#;

    #[test]
    fn parses_entries_attributes_and_groups() {
        let streams = parse_m3u(PLAYLIST);
        assert_eq!(streams.len(), 3); // orphaned EXTINF dropped
        assert_eq!(streams[0].name, "US: FOX SPORTS 1");
        assert_eq!(streams[0].epg_channel_id.as_deref(), Some("FoxSports1.us"));
        assert_eq!(
            streams[0].stream_icon.as_deref(),
            Some("http://img/fs1.png")
        );
        assert_eq!(streams[1].stream_icon, None);
        assert_eq!(streams[0].category_id, Some(CategoryId("US Sports".into())));
        assert_eq!(
            streams[0].direct_source.as_deref(),
            Some("http://host.example/live/alice/hunter2/429586.ts")
        );
        // Name is everything after the LAST comma.
        assert_eq!(streams[2].name, "With Comma");
        assert_eq!(
            streams[2].category_id,
            Some(CategoryId(UNCATEGORIZED.into()))
        );
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        let first = parse_m3u(PLAYLIST);
        let second = parse_m3u(PLAYLIST);
        assert_eq!(first[0].stream_id, second[0].stream_id);
        assert_ne!(first[0].stream_id, first[1].stream_id);
    }

    #[test]
    fn adult_heuristic_flags_group_names() {
        let streams = parse_m3u(PLAYLIST);
        assert!(!streams[0].is_adult);
        assert!(streams[1].is_adult); // "XXX Night" group
    }

    #[test]
    fn categories_derive_in_first_seen_order() {
        let streams = parse_m3u(PLAYLIST);
        let categories = categories_of(&streams);
        assert_eq!(categories.len(), 3);
        assert_eq!(categories[0].category_name, "US Sports");
        assert_eq!(categories[1].category_name, "XXX Night");
    }

    #[test]
    fn non_playlist_text_yields_nothing() {
        assert!(parse_m3u("<html>login page</html>").is_empty());
    }

    #[tokio::test]
    async fn fetches_and_parses_playlist_over_http() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/get.php")
            .match_query(mockito::Matcher::Any)
            .with_body(PLAYLIST)
            .create_async()
            .await;

        let source = M3uSource::new(
            "playlist",
            Redacted::new(format!(
                "{}/get.php?username=alice&password=hunter2&type=m3u_plus",
                server.url()
            )),
            None,
        )
        .unwrap();
        let streams = source.live_streams().await.unwrap();
        assert_eq!(streams.len(), 3);
        let categories = source.live_categories().await.unwrap();
        assert_eq!(categories.len(), 3);
    }

    #[tokio::test]
    async fn html_response_is_a_decode_error_without_body_echo() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/get.php")
            .match_query(mockito::Matcher::Any)
            .with_body("<html>wrong credentials hunter2</html>")
            .create_async()
            .await;

        let source = M3uSource::new(
            "playlist",
            Redacted::new(format!("{}/get.php?password=hunter2", server.url())),
            None,
        )
        .unwrap();
        let err = source.live_streams().await.unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, XtreamError::Decode { .. }));
        assert!(!message.contains("hunter2"));
    }
}
