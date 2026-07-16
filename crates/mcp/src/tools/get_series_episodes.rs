use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Series to inspect: a numeric series_id from search_series (preferred) or a show name to fuzzy-match."
    )]
    pub series: String,
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Serialize)]
struct EpisodeEntry {
    season: Option<i64>,
    episode: Option<i64>,
    title: Option<String>,
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

    let show = match catalog
        .resolve_series(&params.series, &ResolveOptions::default())
        .await
    {
        Ok(Resolution::One(show)) => show,
        Ok(Resolution::Ambiguous(candidates)) => {
            let candidates: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| serde_json::json!({ "series_id": c.series_id, "name": c.name }))
                .collect();
            return Ok(super::tool_json(&serde_json::json!({
                "error": format!(
                    "Ambiguous — {} series match '{}'. Call again with one of these series_ids.",
                    candidates.len(),
                    params.series
                ),
                "candidates": candidates,
            })));
        }
        Ok(Resolution::NotFound) => {
            return Ok(super::tool_error(format!(
                "No series matched '{}'. Call search_series to find the right name or series_id.",
                params.series
            )));
        }
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let episodes = match client.series_info(show.series_id).await {
        Ok(info) => info.episodes(),
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let entries: Vec<EpisodeEntry> = episodes
        .iter()
        .map(|e| EpisodeEntry {
            season: e.season,
            episode: e.episode_num,
            title: e.title.clone(),
        })
        .collect();

    Ok(super::tool_json(&serde_json::json!({
        "series": show.name,
        "series_id": show.series_id,
        "episode_count": entries.len(),
        "episodes": entries,
        "next_step": "Call play_episode with this series_id plus a season and episode number.",
    })))
}
