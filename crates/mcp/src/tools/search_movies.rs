use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use televisor_core::catalog::{self, Catalog, ResolveOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Search text matched case-insensitively against movie names, e.g. \"dune\". Names carry panel prefixes like \"4K-TOP -\". Empty string lists everything in scope — only do that with a category filter."
    )]
    #[serde(default)]
    pub query: String,
    #[schemars(
        description = "Optional VOD category id or name to scope the search (categories differ from live TV's)."
    )]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(description = "Maximum results to return. Default 20.")]
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[schemars(description = "Include adult-flagged movies. Default false.")]
    #[serde(default)]
    pub include_adult: bool,
    #[schemars(
        description = "Set true to refetch the movie list from the panel (slow, tens of thousands of entries) instead of the local cache."
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
}

pub async fn run(params: Params) -> Result<CallToolResult, rmcp::ErrorData> {
    let (name, client) = match super::connect(params.account.as_deref()) {
        Ok(connected) => connected,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let catalog = match Catalog::new(&client, &name) {
        Ok(catalog) => catalog,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let category = match super::vod_category_for(&catalog, params.category.as_deref()).await {
        Ok(category) => category,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let movies = match catalog.movies(params.refresh).await {
        Ok(movies) => movies,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        category,
        ..Default::default()
    };
    let all = catalog::search(&movies, &params.query, &opts, usize::MAX);
    let matches: Vec<Match> = all
        .iter()
        .take(params.max_results.max(1))
        .map(|m| Match {
            stream_id: m.stream_id.0,
            name: m.name.clone(),
            category_id: m.category_id.as_ref().map(|c| c.0.clone()),
        })
        .collect();

    Ok(super::tool_json(&serde_json::json!({
        "query": params.query,
        "total_matches": all.len(),
        "returned": matches.len(),
        "matches": matches,
        "next_step": "Call play_movie with a stream_id (preferred) or an unambiguous name.",
    })))
}
