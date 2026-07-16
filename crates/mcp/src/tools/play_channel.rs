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
    let (name, client) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let catalog = match Catalog::new(&client, &name) {
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
            let url = match client.live_stream_url(stream.stream_id, format) {
                Ok(url) => url,
                Err(err) => return Ok(super::tool_error(err.to_string())),
            };
            let open_opts = OpenOptions {
                fullscreen: params.fullscreen,
                title: Some(stream.name.clone()),
                ..Default::default()
            };
            match vlc::open_stream(url.as_str(), &open_opts) {
                Ok(pid) => Ok(super::tool_json(&serde_json::json!({
                    "playing": stream.name,
                    "stream_id": stream.stream_id.0,
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
