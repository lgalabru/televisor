use rmcp::model::CallToolResult;
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::state::{MediaKind, PlayRecord, record_play_best_effort};
use televisor_core::vlc::{self, OpenOptions};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    #[schemars(
        description = "Movie to play: a numeric stream_id from search_movies (preferred, unambiguous) or a movie name to fuzzy-match."
    )]
    pub movie: String,
    #[schemars(description = "Optional VOD category id or name to narrow name matching.")]
    #[serde(default)]
    pub category: Option<String>,
    #[schemars(description = "Start VLC in fullscreen. Default false.")]
    #[serde(default)]
    pub fullscreen: bool,
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
    let category = match super::vod_category_for(&catalog, params.category.as_deref()).await {
        Ok(category) => category,
        Err(message) => return Ok(super::tool_error(message)),
    };

    let opts = ResolveOptions {
        include_adult: params.include_adult,
        category,
        ..Default::default()
    };
    match catalog.resolve_movie(&params.movie, &opts).await {
        Ok(Resolution::One(movie)) => {
            // The playback URL embeds credentials — handed to VLC, never
            // returned to the model.
            let url = match source
                .xtream()
                .and_then(|c| c.movie_url(movie.stream_id, movie.container_extension.as_deref()))
            {
                Ok(url) => url,
                Err(err) => return Ok(super::tool_error(err.to_string())),
            };
            let open_opts = OpenOptions {
                fullscreen: params.fullscreen,
                title: Some(movie.name.clone()),
                ..Default::default()
            };
            match vlc::open_stream(url.as_str(), &open_opts) {
                Ok(pid) => {
                    record_play_best_effort(
                        &name,
                        PlayRecord::new(
                            MediaKind::Movie,
                            movie.stream_id.0,
                            &movie.name,
                            movie.container_extension.clone(),
                        ),
                    );
                    Ok(super::tool_json(&serde_json::json!({
                        "playing": movie.name,
                        "stream_id": movie.stream_id.0,
                        "vlc_pid": pid,
                        "note": "VLC was launched on the user's machine.",
                    })))
                }
                Err(err) => Ok(super::tool_error(err.to_string())),
            }
        }
        Ok(Resolution::Ambiguous(candidates)) => {
            let candidates: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| serde_json::json!({ "stream_id": c.stream_id.0, "name": c.name }))
                .collect();
            Ok(super::tool_json(&serde_json::json!({
                "error": format!(
                    "Ambiguous — {} movies match '{}'. Call play_movie again with one of these stream_ids.",
                    candidates.len(),
                    params.movie
                ),
                "candidates": candidates,
            })))
        }
        Ok(Resolution::NotFound) => Ok(super::tool_error(format!(
            "No movie matched '{}'. Call search_movies to find the right name or stream_id.",
            params.movie
        ))),
        Err(err) => Ok(super::tool_error(err.to_string())),
    }
}
