use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use televisor_core::catalog::{self, Catalog, ResolveOptions};
use televisor_core::xtream::LiveStream;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Search text matched case-insensitively against channel names, e.g. \"fox sports\". Channel names are prefixed by panel conventions like \"US:\" or \"FR|\". Empty string lists everything in scope — only do that with a category filter."
    )]
    #[serde(default)]
    pub query: String,
    #[schemars(
        description = "Optional category id or name to scope the search (see list_categories)."
    )]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(description = "Maximum results to return. Default 20.")]
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[schemars(description = "Include adult-flagged channels. Default false.")]
    #[serde(default)]
    pub include_adult: bool,
    #[schemars(
        description = "Set true to refetch the channel list from the panel (slow, ~40k entries) instead of the local cache."
    )]
    #[serde(default)]
    pub refresh: bool,
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
}

fn default_max_results() -> usize {
    20
}

#[derive(Serialize)]
struct Match {
    stream_id: u64,
    name: String,
    category_id: Option<String>,
    /// Catch-up replay available (usable with play_channel's `at` param).
    archive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_days: Option<i64>,
}

impl From<&LiveStream> for Match {
    fn from(stream: &LiveStream) -> Self {
        Match {
            stream_id: stream.stream_id.0,
            name: stream.name.clone(),
            category_id: stream.category_id.as_ref().map(|c| c.0.clone()),
            archive: stream.tv_archive,
            archive_days: stream.tv_archive_duration.filter(|_| stream.tv_archive),
        }
    }
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
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
    let streams = match catalog.streams(params.refresh).await {
        Ok(streams) => streams,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        category,
        ..Default::default()
    };
    let all = catalog::search(&streams, &params.query, &opts, usize::MAX);
    let matches: Vec<Match> = all
        .iter()
        .take(params.max_results.max(1))
        .map(Match::from)
        .collect();

    Ok(super::tool_json(&serde_json::json!({
        "query": params.query,
        "total_matches": all.len(),
        "returned": matches.len(),
        "matches": matches,
        "next_step": "Call play_channel with a stream_id (preferred) or an unambiguous name to start playback in VLC.",
    })))
}
