use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Movie: a numeric stream_id from search_movies (preferred) or a movie name to fuzzy-match."
    )]
    pub movie: String,
    #[schemars(description = "Include adult-flagged movies in name matching. Default false.")]
    #[serde(default)]
    pub include_adult: bool,
    #[schemars(description = "Account name from list_accounts. Omit to use the default account.")]
    #[serde(default)]
    pub account: Option<String>,
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

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        ..Default::default()
    };
    let movie = match catalog.resolve_movie(&params.movie, &opts).await {
        Ok(Resolution::One(movie)) => movie,
        Ok(Resolution::Ambiguous(candidates)) => {
            let candidates: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| serde_json::json!({ "stream_id": c.stream_id.0, "name": c.name }))
                .collect();
            return Ok(super::tool_json(&serde_json::json!({
                "error": format!(
                    "Ambiguous — {} movies match '{}'. Call again with one of these stream_ids.",
                    candidates.len(),
                    params.movie
                ),
                "candidates": candidates,
            })));
        }
        Ok(Resolution::NotFound) => {
            return Ok(super::tool_error(format!(
                "No movie matched '{}'. Call search_movies first.",
                params.movie
            )));
        }
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let client = match source.xtream() {
        Ok(client) => client,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let info = match client.vod_info(movie.stream_id).await {
        Ok(info) => info,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let released = info.released().map(str::to_string);
    Ok(super::tool_json(&serde_json::json!({
        "name": movie.name,
        "stream_id": movie.stream_id.0,
        "rating": info.rating.or(movie.rating),
        "genre": info.genre,
        "released": released,
        "duration": info.duration,
        "director": info.director,
        "cast": info.cast,
        "plot": info.plot,
        "youtube_trailer": info.youtube_trailer,
        "next_step": "Call play_movie with this stream_id to start playback.",
    })))
}
