//! HTTP client for the Xtream Codes `player_api.php` API.

use std::time::Duration;

use serde::de::DeserializeOwned;
use thiserror::Error;
use tracing::debug;
use url::Url;

use super::types::{
    AccountSummary, Category, CategoryId, EpgListing, EpgResponse, LiveStream, Series, SeriesInfo,
    StreamFormat, StreamId, VodStream,
};
use crate::config::Account;
use crate::redact::{Redacted, redact_url};

/// Presented instead of reqwest's default — many panels 403 unknown agents.
/// The okhttp string is what mainstream IPTV apps send.
pub const DEFAULT_USER_AGENT: &str = "okhttp/4.9.3";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum XtreamError {
    // Constructed only through `From`, which strips the credentialed URL.
    #[error("HTTP request failed: {0}")]
    Http(reqwest::Error),

    #[error("Server returned HTTP {code}")]
    Status { code: u16 },

    #[error(
        "Authentication failed for account '{account}' — check the host, username, and password"
    )]
    AuthFailed { account: String },

    #[error(
        "Failed to decode {context} response ({detail}) — the panel may be down or the host URL wrong"
    )]
    Decode {
        context: &'static str,
        detail: String,
    },

    #[error("Invalid host URL for account '{account}' — expected e.g. http://host:port")]
    InvalidHost { account: String },
}

impl From<reqwest::Error> for XtreamError {
    fn from(err: reqwest::Error) -> Self {
        // reqwest errors Display their URL, which carries ?username=&password=.
        XtreamError::Http(err.without_url())
    }
}

/// Container extensions come from the panel and are occasionally missing,
/// empty, or dot-prefixed; VOD is overwhelmingly mp4/mkv, so default to mp4.
fn sanitize_ext(ext: Option<&str>) -> &str {
    match ext.map(|e| e.trim().trim_start_matches('.')) {
        Some(e) if !e.is_empty() => e,
        _ => "mp4",
    }
}

/// Parse a user-supplied catch-up start time ("YYYY-MM-DD HH:MM", `T`
/// separator also accepted) into the panel's `YYYY-MM-DD:HH-MM` format.
/// The value is passed through verbatim otherwise — panels interpret it in
/// their own (server) timezone, so no timezone conversion is attempted.
pub fn timeshift_start(input: &str) -> Result<String, String> {
    let normalized = input.trim().replace('T', " ");
    let parsed: jiff::civil::DateTime = normalized.parse().map_err(|_| {
        format!("invalid start time '{input}' — expected e.g. \"2026-07-15 20:00\"")
    })?;
    Ok(parsed.strftime("%Y-%m-%d:%H-%M").to_string())
}

pub struct XtreamClient {
    http: reqwest::Client,
    base: Url,
    /// Account name, used in error messages (never the credentials).
    account: String,
    username: String,
    password: Redacted,
}

impl XtreamClient {
    pub fn new(account_name: &str, account: &Account) -> Result<Self, XtreamError> {
        let base = Url::parse(&account.host).map_err(|_| XtreamError::InvalidHost {
            account: account_name.to_string(),
        })?;
        let user_agent = account
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(CONNECT_TIMEOUT)
            // No total timeout: the full stream list is 10–20 MB and may
            // legitimately take a while; read_timeout catches real stalls.
            .read_timeout(READ_TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            base,
            account: account_name.to_string(),
            username: account.username.clone(),
            password: account.password.clone(),
        })
    }

    /// Account + server metadata; fails with [`XtreamError::AuthFailed`] when
    /// the panel answers HTTP 200 with `auth: 0` (its way of rejecting creds).
    pub async fn account_info(&self) -> Result<AccountSummary, XtreamError> {
        let summary: AccountSummary = self.get_json("account info", None, &[]).await?;
        if !summary.user_info.auth {
            return Err(XtreamError::AuthFailed {
                account: self.account.clone(),
            });
        }
        Ok(summary)
    }

    pub async fn live_categories(&self) -> Result<Vec<Category>, XtreamError> {
        self.get_json("live categories", Some("get_live_categories"), &[])
            .await
    }

    pub async fn live_streams(
        &self,
        category: Option<&CategoryId>,
    ) -> Result<Vec<LiveStream>, XtreamError> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(category) = category {
            params.push(("category_id", category.0.as_str()));
        }
        self.get_json("live streams", Some("get_live_streams"), &params)
            .await
    }

    pub async fn vod_categories(&self) -> Result<Vec<Category>, XtreamError> {
        self.get_json("VOD categories", Some("get_vod_categories"), &[])
            .await
    }

    pub async fn vod_streams(
        &self,
        category: Option<&CategoryId>,
    ) -> Result<Vec<VodStream>, XtreamError> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(category) = category {
            params.push(("category_id", category.0.as_str()));
        }
        self.get_json("VOD streams", Some("get_vod_streams"), &params)
            .await
    }

    pub async fn series_categories(&self) -> Result<Vec<Category>, XtreamError> {
        self.get_json("series categories", Some("get_series_categories"), &[])
            .await
    }

    pub async fn series_list(
        &self,
        category: Option<&CategoryId>,
    ) -> Result<Vec<Series>, XtreamError> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(category) = category {
            params.push(("category_id", category.0.as_str()));
        }
        self.get_json("series list", Some("get_series"), &params)
            .await
    }

    /// Now/next programmes for a live channel (`get_short_epg`).
    pub async fn short_epg(
        &self,
        stream: StreamId,
        limit: u32,
    ) -> Result<Vec<EpgListing>, XtreamError> {
        let response: EpgResponse = self
            .get_json(
                "short EPG",
                Some("get_short_epg"),
                &[
                    ("stream_id", &stream.to_string()),
                    ("limit", &limit.to_string()),
                ],
            )
            .await?;
        Ok(response.epg_listings)
    }

    /// Full schedule for a live channel (`get_simple_data_table`).
    pub async fn full_epg(&self, stream: StreamId) -> Result<Vec<EpgListing>, XtreamError> {
        let response: EpgResponse = self
            .get_json(
                "full EPG",
                Some("get_simple_data_table"),
                &[("stream_id", &stream.to_string())],
            )
            .await?;
        Ok(response.epg_listings)
    }

    pub async fn series_info(&self, series_id: u64) -> Result<SeriesInfo, XtreamError> {
        self.get_json(
            "series info",
            Some("get_series_info"),
            &[("series_id", &series_id.to_string())],
        )
        .await
    }

    /// Playback URL for a live stream. The credentials become path segments,
    /// percent-encoded, so any username/password characters are safe.
    pub fn live_stream_url(
        &self,
        stream: StreamId,
        format: StreamFormat,
    ) -> Result<Url, XtreamError> {
        self.media_url("live", stream.0, format.ext())
    }

    /// Catch-up playback URL:
    /// `/timeshift/USER/PASS/DURATION_MIN/YYYY-MM-DD:HH-MM/ID.EXT`.
    /// `start` must already be in panel format (see [`timeshift_start`]) and
    /// is interpreted in the **panel's** timezone (`server_info.timezone`).
    pub fn timeshift_url(
        &self,
        stream: StreamId,
        start: &str,
        duration_minutes: u32,
        format: StreamFormat,
    ) -> Result<Url, XtreamError> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| XtreamError::InvalidHost {
                    account: self.account.clone(),
                })?;
            path.clear()
                .push("timeshift")
                .push(&self.username)
                .push(self.password.expose())
                .push(&duration_minutes.to_string())
                .push(start)
                .push(&format!("{}.{}", stream, format.ext()));
        }
        Ok(url)
    }

    /// Playback URL for a movie: `/movie/USER/PASS/ID.EXT`, where EXT is the
    /// `container_extension` from the VOD listing.
    pub fn movie_url(&self, stream: StreamId, ext: Option<&str>) -> Result<Url, XtreamError> {
        self.media_url("movie", stream.0, sanitize_ext(ext))
    }

    /// Playback URL for a series episode: `/series/USER/PASS/EPISODE_ID.EXT`.
    pub fn episode_url(&self, episode_id: u64, ext: Option<&str>) -> Result<Url, XtreamError> {
        self.media_url("series", episode_id, sanitize_ext(ext))
    }

    fn media_url(&self, prefix: &str, id: u64, ext: &str) -> Result<Url, XtreamError> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| XtreamError::InvalidHost {
                    account: self.account.clone(),
                })?;
            path.clear()
                .push(prefix)
                .push(&self.username)
                .push(self.password.expose())
                .push(&format!("{id}.{ext}"));
        }
        Ok(url)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        context: &'static str,
        action: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<T, XtreamError> {
        let mut url = self
            .base
            .join("player_api.php")
            .map_err(|_| XtreamError::InvalidHost {
                account: self.account.clone(),
            })?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("username", &self.username);
            query.append_pair("password", self.password.expose());
            if let Some(action) = action {
                query.append_pair("action", action);
            }
            for (key, value) in extra {
                query.append_pair(key, value);
            }
        }
        debug!(url = %redact_url(&url), "Xtream API request");

        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(XtreamError::Status {
                code: status.as_u16(),
            });
        }
        // Text first, then parse: panels return HTML error pages with HTTP
        // 200, and `.json()` would discard the evidence.
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(|err| XtreamError::Decode {
            context,
            // Only the error category and position — serde messages can echo
            // body values, and this body embeds account state.
            detail: format!(
                "{:?} error at line {} column {}",
                err.classify(),
                err.line(),
                err.column()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Account;

    fn account(host: &str) -> Account {
        Account {
            host: host.to_string(),
            username: "alice".to_string(),
            password: Redacted::new("hunter2"),
            user_agent: None,
        }
    }

    fn client(host: &str) -> XtreamClient {
        XtreamClient::new("test", &account(host)).unwrap()
    }

    #[test]
    fn stream_url_embeds_and_encodes_credentials() {
        let client = XtreamClient::new(
            "test",
            &Account {
                host: "http://host.example:8080".to_string(),
                username: "al ice".to_string(),
                password: Redacted::new("p@ss/w0rd"),
                user_agent: None,
            },
        )
        .unwrap();
        let url = client
            .live_stream_url(StreamId(2012600), StreamFormat::M3u8)
            .unwrap();
        assert_eq!(
            url.as_str(),
            "http://host.example:8080/live/al%20ice/p@ss%2Fw0rd/2012600.m3u8"
        );
        let ts = client
            .live_stream_url(StreamId(7), StreamFormat::Ts)
            .unwrap();
        assert!(ts.as_str().ends_with("/7.ts"));
    }

    #[test]
    fn timeshift_url_and_start_parsing() {
        let client = client("http://host.example:8080");
        let start = timeshift_start("2026-07-15 20:00").unwrap();
        assert_eq!(start, "2026-07-15:20-00");
        assert_eq!(
            timeshift_start("2026-07-15T20:00").unwrap(),
            "2026-07-15:20-00"
        );
        assert!(timeshift_start("yesterday 8pm").is_err());

        let url = client
            .timeshift_url(StreamId(497001), &start, 120, StreamFormat::M3u8)
            .unwrap();
        assert_eq!(
            url.as_str(),
            "http://host.example:8080/timeshift/alice/hunter2/120/2026-07-15:20-00/497001.m3u8"
        );
    }

    #[test]
    fn movie_and_episode_urls_use_container_extension() {
        let client = client("http://host.example:8080");
        let movie = client.movie_url(StreamId(31341), Some("mkv")).unwrap();
        assert_eq!(
            movie.as_str(),
            "http://host.example:8080/movie/alice/hunter2/31341.mkv"
        );
        let episode = client.episode_url(2066378, Some(".avi")).unwrap();
        assert!(
            episode
                .as_str()
                .ends_with("/series/alice/hunter2/2066378.avi")
        );
        // Missing/empty extensions default to mp4.
        let fallback = client.movie_url(StreamId(7), None).unwrap();
        assert!(fallback.as_str().ends_with("/movie/alice/hunter2/7.mp4"));
        let empty = client.movie_url(StreamId(7), Some("  ")).unwrap();
        assert!(empty.as_str().ends_with("/7.mp4"));
    }

    #[tokio::test]
    async fn account_info_happy_path() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/player_api.php")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("username".into(), "alice".into()),
                mockito::Matcher::UrlEncoded("password".into(), "hunter2".into()),
            ]))
            .with_body(include_str!("fixtures/user_info.json"))
            .create_async()
            .await;

        let info = client(&server.url()).account_info().await.unwrap();
        assert!(info.user_info.auth);
        assert_eq!(info.user_info.max_connections, Some(1));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn auth_zero_is_auth_failed_and_error_hides_credentials() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/player_api.php")
            .match_query(mockito::Matcher::Any)
            .with_body(include_str!("fixtures/user_info_auth_failed.json"))
            .create_async()
            .await;

        let err = client(&server.url()).account_info().await.unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, XtreamError::AuthFailed { .. }));
        assert!(message.contains("test")); // names the account…
        assert!(!message.contains("hunter2")); // …never the password
        assert!(!message.contains("alice"));
    }

    #[tokio::test]
    async fn html_body_with_200_is_a_decode_error_without_body_echo() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/player_api.php")
            .match_query(mockito::Matcher::Any)
            .with_body("<html><body>secret-panel-state hunter2</body></html>")
            .create_async()
            .await;

        let err = client(&server.url()).account_info().await.unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, XtreamError::Decode { .. }));
        assert!(!message.contains("hunter2"));
        assert!(!message.contains("secret-panel-state"));
    }

    #[tokio::test]
    async fn non_2xx_maps_to_status() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/player_api.php")
            .match_query(mockito::Matcher::Any)
            .with_status(512)
            .create_async()
            .await;

        let err = client(&server.url()).account_info().await.unwrap_err();
        assert!(matches!(err, XtreamError::Status { code: 512 }));
    }

    #[tokio::test]
    async fn connect_error_display_has_no_credentials() {
        // Unroutable port: reqwest error would normally Display the URL.
        let err = client("http://127.0.0.1:1")
            .account_info()
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(!message.contains("hunter2"));
        assert!(!message.contains("alice"));
    }

    #[tokio::test]
    async fn category_filter_is_forwarded() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/player_api.php")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("action".into(), "get_live_streams".into()),
                mockito::Matcher::UrlEncoded("category_id".into(), "2343".into()),
            ]))
            .with_body(include_str!("fixtures/live_streams.json"))
            .create_async()
            .await;

        let streams = client(&server.url())
            .live_streams(Some(&CategoryId("2343".into())))
            .await
            .unwrap();
        assert_eq!(streams.len(), 3);
        mock.assert_async().await;
    }
}

#[cfg(all(test, feature = "test-network"))]
mod network_tests {
    use super::*;
    use crate::config::Account;

    /// Smoke test against a real panel. Run with:
    /// `TELEVISOR_TEST_HOST=… TELEVISOR_TEST_USERNAME=… TELEVISOR_TEST_PASSWORD=… \
    ///  cargo test --features test-network -- --ignored`
    #[tokio::test]
    #[ignore = "requires TELEVISOR_TEST_* env vars and network access"]
    async fn real_panel_account_info() {
        let account = Account {
            host: std::env::var("TELEVISOR_TEST_HOST").expect("TELEVISOR_TEST_HOST"),
            username: std::env::var("TELEVISOR_TEST_USERNAME").expect("TELEVISOR_TEST_USERNAME"),
            password: Redacted::new(
                std::env::var("TELEVISOR_TEST_PASSWORD").expect("TELEVISOR_TEST_PASSWORD"),
            ),
            user_agent: None,
        };
        let client = XtreamClient::new("smoke", &account).unwrap();
        let info = client.account_info().await.unwrap();
        assert!(info.user_info.auth);
    }
}
