//! One module per tool: each owns its `Params` and its logic. The server
//! layer in `server.rs` only dispatches.
//!
//! Conventions:
//! - Business/validation failures are returned **in the response body**
//!   (`{"error": ...}`), never as `Err(ErrorData)` — the model reads the
//!   message and self-corrects; a protocol error would make it give up.
//! - No response ever contains a password or a credentialed URL. Playback
//!   happens by launching VLC locally, not by handing the model a stream URL.

pub mod get_epg;
pub mod get_movie_info;
pub mod get_series_episodes;
pub mod get_status;
pub mod list_accounts;
pub mod list_categories;
pub mod play_channel;
pub mod play_episode;
pub mod play_movie;
pub mod search_channels;
pub mod search_movies;
pub mod search_series;

use rmcp::model::{CallToolResult, Content};
use serde::Serialize;
use televisor_core::catalog::Catalog;
use televisor_core::config::TelevisorConfig;
use televisor_core::source::MediaSource;
use televisor_core::xtream::CategoryId;

/// Successful result: the value as pretty JSON text.
pub(crate) fn tool_json<T: Serialize>(value: &T) -> CallToolResult {
    let json = serde_json::to_string_pretty(value).unwrap_or_default();
    CallToolResult::success(vec![Content::text(json)])
}

/// Business-level failure in the response body.
pub(crate) fn tool_error(message: impl Into<String>) -> CallToolResult {
    tool_json(&serde_json::json!({ "error": message.into() }))
}

/// Failure from a typed error, attaching the parsed 402 challenge when the
/// gateway is stablecoin-gated so the agent can hand it to pay.
pub(crate) fn tool_err<E>(err: E) -> CallToolResult
where
    E: std::fmt::Display + televisor_core::payment::PaymentChallengeExt,
{
    match err.payment_challenge() {
        Some(challenge) => tool_json(&serde_json::json!({
            "error": err.to_string(),
            "payment_required": challenge,
        })),
        None => tool_error(err.to_string()),
    }
}

/// Resolve the account and build a client. Error strings are safe to show
/// the model: every error type involved redacts credentials by construction.
pub(crate) fn connect(account: Option<&str>) -> Result<(String, MediaSource), String> {
    let config = TelevisorConfig::load().map_err(|e| e.to_string())?;
    let (name, account) = config.resolve_account(account).map_err(|e| e.to_string())?;
    let source = MediaSource::new(&name, account).map_err(|e| e.to_string())?;
    Ok((name, source))
}

/// Resolve an optional live category id-or-name, with a self-correction hint.
pub(crate) async fn category_for(
    catalog: &Catalog<'_>,
    input: Option<&str>,
) -> Result<Option<CategoryId>, String> {
    let categories = match input {
        None => return Ok(None),
        Some(_) => catalog.categories(false).await.map_err(|e| e.to_string())?,
    };
    lookup_category(&categories, input.expect("checked above"))
}

/// Same, against the VOD category list.
pub(crate) async fn vod_category_for(
    catalog: &Catalog<'_>,
    input: Option<&str>,
) -> Result<Option<CategoryId>, String> {
    let categories = match input {
        None => return Ok(None),
        Some(_) => catalog
            .vod_categories(false)
            .await
            .map_err(|e| e.to_string())?,
    };
    lookup_category(&categories, input.expect("checked above"))
}

/// Same, against the series category list.
pub(crate) async fn series_category_for(
    catalog: &Catalog<'_>,
    input: Option<&str>,
) -> Result<Option<CategoryId>, String> {
    let categories = match input {
        None => return Ok(None),
        Some(_) => catalog
            .series_categories(false)
            .await
            .map_err(|e| e.to_string())?,
    };
    lookup_category(&categories, input.expect("checked above"))
}

fn lookup_category(
    categories: &[televisor_core::xtream::Category],
    input: &str,
) -> Result<Option<CategoryId>, String> {
    televisor_core::catalog::resolve_category(categories, input)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Category '{input}' not found in this section. Note that live TV, movies, and series each have their own category lists."
            )
        })
}
