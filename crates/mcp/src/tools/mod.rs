//! One module per tool: each owns its `Params` and its logic. The server
//! layer in `server.rs` only dispatches.
//!
//! Conventions:
//! - Business/validation failures are returned **in the response body**
//!   (`{"error": ...}`), never as `Err(ErrorData)` — the model reads the
//!   message and self-corrects; a protocol error would make it give up.
//! - No response ever contains a password or a credentialed URL. Playback
//!   happens by launching VLC locally, not by handing the model a stream URL.

pub mod get_status;
pub mod list_accounts;
pub mod list_categories;
pub mod play_channel;
pub mod search_channels;

use rmcp::model::{CallToolResult, Content};
use serde::Serialize;
use televisor_core::catalog::Catalog;
use televisor_core::config::TelevisorConfig;
use televisor_core::xtream::{CategoryId, XtreamClient};

/// Successful result: the value as pretty JSON text.
pub(crate) fn tool_json<T: Serialize>(value: &T) -> CallToolResult {
    let json = serde_json::to_string_pretty(value).unwrap_or_default();
    CallToolResult::success(vec![Content::text(json)])
}

/// Business-level failure in the response body.
pub(crate) fn tool_error(message: impl Into<String>) -> CallToolResult {
    tool_json(&serde_json::json!({ "error": message.into() }))
}

/// Resolve the account and build a client. Error strings are safe to show
/// the model: every error type involved redacts credentials by construction.
pub(crate) fn connect(account: Option<&str>) -> Result<(String, XtreamClient), String> {
    let config = TelevisorConfig::load().map_err(|e| e.to_string())?;
    let (name, account) = config.resolve_account(account).map_err(|e| e.to_string())?;
    let client = XtreamClient::new(&name, account).map_err(|e| e.to_string())?;
    Ok((name, client))
}

/// Resolve an optional category id-or-name, with a self-correction hint.
pub(crate) async fn category_for(
    catalog: &Catalog<'_>,
    input: Option<&str>,
) -> Result<Option<CategoryId>, String> {
    match input {
        None => Ok(None),
        Some(input) => {
            let categories = catalog.categories(false).await.map_err(|e| e.to_string())?;
            televisor_core::catalog::resolve_category(&categories, input)
                .map(Some)
                .ok_or_else(|| {
                    format!(
                        "Category '{input}' not found. Call list_categories and pass one of the returned ids or names."
                    )
                })
        }
    }
}
