use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::vlc::{self, OpenOptions};
use televisor_core::xtream::StreamFormat;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Channel to play: a numeric stream_id from search_channels (preferred, unambiguous) or a channel name to fuzzy-match, e.g. \"fox sports 1\"."
    )]
    pub channel: String,
    #[schemars(
        description = "Optional category id or name to narrow name matching (see list_categories)."
    )]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(
        description = "Container format: \"m3u8\" (default) or \"ts\". Retry with \"ts\" if the user reports playback failing."
    )]
    #[serde(default)]
    pub format: Option<String>,
    #[schemars(
        description = "Catch-up replay start, \"YYYY-MM-DD HH:MM\", interpreted in the PANEL's timezone (get_status → server_info.timezone). Only meaningful for channels with archive=true in search_channels results."
    )]
    #[serde(default)]
    pub at: Option<String>,
    #[schemars(description = "Catch-up clip length in minutes (with `at`). Default 120.")]
    #[serde(default)]
    pub duration_minutes: Option<u32>,
    #[schemars(description = "Start VLC in fullscreen. Default false.")]
    #[serde(default)]
    pub fullscreen: bool,
    #[schemars(description = "Include adult-flagged channels in name matching. Default false.")]
    #[serde(default)]
    pub include_adult: bool,
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
    let format = match params.format.as_deref() {
        None => StreamFormat::M3u8,
        Some(raw) => match raw.parse::<StreamFormat>() {
            Ok(format) => format,
            Err(message) => {
                return Ok(super::tool_error(format!(
                    "Invalid format '{raw}': {message}. Valid values: \"m3u8\", \"ts\"."
                )));
            }
        },
    };
    let (name, source) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let catalog = match Catalog::new(&source, &name) {
        Ok(catalog) => catalog,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let category = match super::category_for(&catalog, params.category.as_deref()).await {
        Ok(category) => category,
        Err(message) => return Ok(super::tool_error(message)),
    };

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        category,
        ..Default::default()
    };
    let resolution = match catalog.resolve(&params.channel, &opts).await {
        Ok(resolution) => resolution,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    match resolution {
        Resolution::One(stream) => {
            // The playback URL embeds credentials — it is handed to VLC and
            // never returned to the model.
            let (url, title, mode) = match &params.at {
                Some(at) => {
                    let start = match televisor_core::xtream::client::timeshift_start(at) {
                        Ok(start) => start,
                        Err(message) => return Ok(super::tool_error(message)),
                    };
                    if !stream.tv_archive {
                        return Ok(super::tool_error(format!(
                            "'{}' has no catch-up archive on this panel. Pick a channel with archive=true from search_channels, or call play_channel without `at` for live playback.",
                            stream.name
                        )));
                    }
                    let duration = params.duration_minutes.unwrap_or(120);
                    let url = match source
                        .xtream()
                        .and_then(|c| c.timeshift_url(stream.stream_id, &start, duration, format))
                    {
                        Ok(url) => url,
                        Err(err) => return Ok(super::tool_error(err.to_string())),
                    };
                    (url, format!("{} @ {at}", stream.name), "catch-up")
                }
                None => {
                    let url = match source.live_stream_url(&stream, format) {
                        Ok(url) => url,
                        Err(err) => return Ok(super::tool_error(err.to_string())),
                    };
                    (url, stream.name.clone(), "live")
                }
            };
            let open_opts = OpenOptions {
                fullscreen: params.fullscreen,
                title: Some(title.clone()),
                ..Default::default()
            };
            match vlc::open_stream(url.as_str(), &open_opts) {
                Ok(pid) => Ok(super::tool_json(&serde_json::json!({
                    "playing": title,
                    "stream_id": stream.stream_id.0,
                    "mode": mode,
                    "format": format.ext(),
                    "vlc_pid": pid,
                    "note": "VLC was launched on the user's machine.",
                }))),
                Err(err) => Ok(super::tool_error(err.to_string())),
            }
        }
        Resolution::Ambiguous(candidates) => {
            let candidates: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| serde_json::json!({ "stream_id": c.stream_id.0, "name": c.name }))
                .collect();
            Ok(super::tool_json(&serde_json::json!({
                "error": format!(
                    "Ambiguous — {} channels match '{}'. Call play_channel again with one of these stream_ids.",
                    candidates.len(),
                    params.channel
                ),
                "candidates": candidates,
            })))
        }
        Resolution::NotFound => Ok(super::tool_error(format!(
            "No channel matched '{}'. Call search_channels to find the right name or stream_id.",
            params.channel
        ))),
    }
}
