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
        description = "Optional VOD category id or case-insensitive name keyword to scope the search (categories differ from live TV's). Use query=\"\" with a category to list that category's movies."
    )]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(
        description = "Maximum results to return per page. Default 20. Ignored when all=true."
    )]
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[schemars(description = "Zero-based result offset for pagination. Default 0.")]
    #[serde(default)]
    pub offset: usize,
    #[schemars(
        description = "Return every match in one response. Default false. For a complete category listing, set category, query=\"\", and all=true."
    )]
    #[serde(default)]
    pub all: bool,
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
    /// 0–10 scale when the panel provides one.
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
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
    let category = match super::vod_category_for(&catalog, params.category.as_deref()).await {
        Ok(category) => category,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let movies = match category.as_ref() {
        Some(category) => catalog.movies_in_category(category, params.refresh).await,
        None => catalog.movies(params.refresh).await,
    };
    let movies = match movies {
        Ok(movies) => movies,
        Err(err) => return Ok(super::tool_err(err)),
    };

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        category: category.clone(),
        ..Default::default()
    };
    if params.all && params.query.trim().is_empty() && category.is_none() {
        return Ok(super::tool_error(
            "Refusing an unscoped full-catalog response. Set category to a VOD category id/name keyword, or provide a non-empty query before using all=true.",
        ));
    }

    let limit = if params.all {
        usize::MAX
    } else {
        params.max_results.max(1)
    };
    let page = catalog::search_page(&movies, &params.query, &opts, params.offset, limit);
    let total_matches = page.total;
    let matches: Vec<Match> = page
        .matches
        .into_iter()
        .map(|m| Match {
            stream_id: m.stream_id.0,
            name: m.name,
            category_id: m.category_id.map(|c| c.0),
            rating: m.rating,
        })
        .collect();
    let next_offset = params.offset.saturating_add(matches.len());
    let has_more = next_offset < total_matches;

    Ok(super::tool_json(&serde_json::json!({
        "query": params.query,
        "category": params.category,
        "total_matches": total_matches,
        "offset": params.offset,
        "returned": matches.len(),
        "has_more": has_more,
        "next_offset": has_more.then_some(next_offset),
        "matches": matches,
        "next_step": "Call get_movie_info for plot/cast/duration, or play_movie with a stream_id to start playback.",
    })))
}
