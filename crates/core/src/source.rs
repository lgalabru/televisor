//! Account-level media source: a full Xtream JSON API login, or a plain M3U
//! playlist URL. Live TV works over both; VOD, series, EPG, catch-up, and
//! account status need the Xtream API and fail with a clear error on M3U
//! accounts.

use url::Url;

use crate::config::Account;
use crate::m3u::M3uSource;
use crate::xtream::{Category, LiveStream, StreamFormat, XtreamClient, XtreamError};

pub enum MediaSource {
    Xtream(XtreamClient),
    M3u(M3uSource),
}

impl MediaSource {
    pub fn new(account_name: &str, account: &Account) -> Result<Self, XtreamError> {
        match &account.m3u_url {
            Some(url) => Ok(MediaSource::M3u(M3uSource::new(
                account_name,
                url.clone(),
                account.user_agent.clone(),
            )?)),
            None => Ok(MediaSource::Xtream(XtreamClient::new(
                account_name,
                account,
            )?)),
        }
    }

    pub async fn live_streams(&self) -> Result<Vec<LiveStream>, XtreamError> {
        match self {
            MediaSource::Xtream(client) => client.live_streams(None).await,
            MediaSource::M3u(source) => source.live_streams().await,
        }
    }

    pub async fn live_categories(&self) -> Result<Vec<Category>, XtreamError> {
        match self {
            MediaSource::Xtream(client) => client.live_categories().await,
            MediaSource::M3u(source) => source.live_categories().await,
        }
    }

    /// Playback URL for a live channel. Xtream builds it from the stream id;
    /// M3U entries carry theirs in `direct_source` (`format` doesn't apply).
    pub fn live_stream_url(
        &self,
        stream: &LiveStream,
        format: StreamFormat,
    ) -> Result<Url, XtreamError> {
        match self {
            MediaSource::Xtream(client) => client.live_stream_url(stream.stream_id, format),
            MediaSource::M3u(_) => {
                let direct = stream.direct_source.as_deref().ok_or(XtreamError::Decode {
                    context: "M3U entry",
                    detail: "entry has no playback URL".to_string(),
                })?;
                Url::parse(direct).map_err(|_| XtreamError::Decode {
                    context: "M3U entry",
                    detail: "entry URL is not a valid URL".to_string(),
                })
            }
        }
    }

    /// The Xtream client, for API-only features (VOD, series, EPG, catch-up,
    /// status). Errors with guidance on M3U accounts.
    pub fn xtream(&self) -> Result<&XtreamClient, XtreamError> {
        match self {
            MediaSource::Xtream(client) => Ok(client),
            MediaSource::M3u(source) => Err(XtreamError::M3uOnly {
                account: source.account_name().to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::Redacted;
    use crate::xtream::StreamId;

    fn m3u_source() -> MediaSource {
        MediaSource::new(
            "playlist",
            &Account {
                host: "http://host.example".to_string(),
                username: "m3u".to_string(),
                password: Redacted::new(""),
                user_agent: None,
                m3u_url: Some(Redacted::new(
                    "http://host.example/get.php?username=a&password=b&type=m3u_plus",
                )),
            },
        )
        .unwrap()
    }

    #[test]
    fn m3u_account_builds_m3u_source_and_rejects_xtream_features() {
        let source = m3u_source();
        assert!(matches!(source, MediaSource::M3u(_)));
        let Err(err) = source.xtream() else {
            panic!("expected M3uOnly error");
        };
        assert!(matches!(err, XtreamError::M3uOnly { .. }));
        assert!(err.to_string().contains("playlist"));
    }

    #[test]
    fn m3u_playback_uses_direct_source() {
        let source = m3u_source();
        let stream = LiveStream {
            stream_id: StreamId(1),
            name: "CH".to_string(),
            category_id: None,
            epg_channel_id: None,
            is_adult: false,
            added: None,
            tv_archive: false,
            tv_archive_duration: None,
            direct_source: Some("http://media.example/ch.ts".to_string()),
        };
        let url = source.live_stream_url(&stream, StreamFormat::M3u8).unwrap();
        assert_eq!(url.as_str(), "http://media.example/ch.ts");

        let broken = LiveStream {
            direct_source: None,
            ..stream
        };
        assert!(source.live_stream_url(&broken, StreamFormat::M3u8).is_err());
    }
}
