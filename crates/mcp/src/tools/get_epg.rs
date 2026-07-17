use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Live channel: a numeric stream_id from search_channels (preferred) or a channel name to fuzzy-match."
    )]
    pub channel: String,
    #[schemars(description = "Number of upcoming programmes to return. Default 5.")]
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[schemars(
        description = "Set true for the channel's full schedule instead of now/next (can be hundreds of entries)."
    )]
    #[serde(default)]
    pub full: bool,
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
}

fn default_limit() -> u32 {
    5
}

#[derive(Serialize)]
struct Programme {
    title: Option<String>,
    description: Option<String>,
    start_unix: Option<i64>,
    stop_unix: Option<i64>,
    now_playing: bool,
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
    let (name, source) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let catalog = match Catalog::new(&source, &name) {
        Ok(catalog) => catalog,
        Err(err) => return Ok(super::tool_err(err)),
    };

    let stream = match catalog
        .resolve(&params.channel, &ResolveOptions::default())
        .await
    {
        Ok(Resolution::One(stream)) => stream,
        Ok(Resolution::Ambiguous(candidates)) => {
            let candidates: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| serde_json::json!({ "stream_id": c.stream_id.0, "name": c.name }))
                .collect();
            return Ok(super::tool_json(&serde_json::json!({
                "error": format!(
                    "Ambiguous — {} channels match '{}'. Call again with one of these stream_ids.",
                    candidates.len(),
                    params.channel
                ),
                "candidates": candidates,
            })));
        }
        Ok(Resolution::NotFound) => {
            return Ok(super::tool_error(format!(
                "No channel matched '{}'. Call search_channels first.",
                params.channel
            )));
        }
        Err(err) => return Ok(super::tool_err(err)),
    };

    let client = match source.xtream() {
        Ok(client) => client,
        Err(err) => return Ok(super::tool_err(err)),
    };
    let listings = if params.full {
        client.full_epg(stream.stream_id).await
    } else {
        client
            .short_epg(stream.stream_id, params.limit.max(1))
            .await
    };
    let listings = match listings {
        Ok(listings) => listings,
        Err(err) => return Ok(super::tool_err(err)),
    };
    if listings.is_empty() {
        return Ok(super::tool_error(format!(
            "No EPG data for '{}' — this channel isn't mapped to a guide on the panel. The channel itself may still play fine via play_channel.",
            stream.name
        )));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let programmes: Vec<Programme> = listings
        .iter()
        .map(|l| Programme {
            title: l.title.clone(),
            description: l.description.clone(),
            start_unix: l.start_timestamp,
            stop_unix: l.stop_timestamp,
            now_playing: l.is_on_air(now),
        })
        .collect();

    Ok(super::tool_json(&serde_json::json!({
        "channel": stream.name,
        "stream_id": stream.stream_id.0,
        "programmes": programmes,
        "note": "start_unix/stop_unix are epoch seconds; now_playing marks the programme airing right now.",
    })))
}
