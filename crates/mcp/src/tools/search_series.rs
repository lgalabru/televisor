use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use televisor_core::catalog::{self, Catalog, ResolveOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Search text matched case-insensitively against series (show) names, e.g. \"breaking bad\". Empty string lists everything in scope — only do that with a category filter."
    )]
    #[serde(default)]
    pub query: String,
    #[schemars(
        description = "Optional series category id or name to scope the search (categories differ from live TV's and VOD's)."
    )]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(description = "Maximum results to return. Default 20.")]
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[schemars(
        description = "Set true to refetch the series list from the panel instead of the local cache."
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

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

#[derive(Serialize)]
struct Match {
    series_id: u64,
    name: String,
    category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    genre: Option<String>,
    /// First ~200 chars — call the CLI or a richer tool for the full plot.
    #[serde(skip_serializing_if = "Option::is_none")]
    plot: Option<String>,
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
    let category = match super::series_category_for(&catalog, params.category.as_deref()).await {
        Ok(category) => category,
        Err(message) => return Ok(super::tool_error(message)),
    };
    let series = match catalog.series_list(params.refresh).await {
        Ok(series) => series,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let opts = ResolveOptions {
        category,
        ..Default::default()
    };
    let all = catalog::search(&series, &params.query, &opts, usize::MAX);
    let matches: Vec<Match> = all
        .iter()
        .take(params.max_results.max(1))
        .map(|s| Match {
            series_id: s.series_id,
            name: s.name.clone(),
            category_id: s.category_id.as_ref().map(|c| c.0.clone()),
            rating: s.rating,
            genre: s.genre.clone(),
            plot: s.plot.as_ref().map(|p| truncate(p, 200)),
        })
        .collect();

    Ok(super::tool_json(&serde_json::json!({
        "query": params.query,
        "total_matches": all.len(),
        "returned": matches.len(),
        "matches": matches,
        "next_step": "Call get_series_episodes with a series_id to see seasons/episodes, then play_episode.",
    })))
}
