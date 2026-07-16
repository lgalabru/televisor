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
        description = "Series: a numeric series_id from search_series (preferred) or a show name to fuzzy-match."
    )]
    pub series: String,
    #[schemars(description = "Season number, e.g. 1.")]
    pub season: i64,
    #[schemars(description = "Episode number within the season, e.g. 2.")]
    pub episode: i64,
    #[schemars(description = "Start VLC in fullscreen. Default false.")]
    #[serde(default)]
    pub fullscreen: bool,
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
                "No series matched '{}'. Call search_series first.",
                params.series
            )));
        }
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };

    let client = match source.xtream() {
        Ok(client) => client,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let episodes = match client.series_info(show.series_id).await {
        Ok(info) => info.episodes(),
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let Some(episode) = episodes
        .iter()
        .find(|e| e.season == Some(params.season) && e.episode_num == Some(params.episode))
    else {
        let available: Vec<String> = episodes
            .iter()
            .map(|e| {
                format!(
                    "S{:02}E{:02}",
                    e.season.unwrap_or(0),
                    e.episode_num.unwrap_or(0)
                )
            })
            .collect();
        return Ok(super::tool_json(&serde_json::json!({
            "error": format!(
                "'{}' has no S{:02}E{:02}. Available episodes are listed below.",
                show.name, params.season, params.episode
            ),
            "available": available,
        })));
    };

    // The playback URL embeds credentials — handed to VLC, never returned.
    let url = match client.episode_url(episode.id, episode.container_extension.as_deref()) {
        Ok(url) => url,
        Err(err) => return Ok(super::tool_error(err.to_string())),
    };
    let title = format!("{} S{:02}E{:02}", show.name, params.season, params.episode);
    let open_opts = OpenOptions {
        fullscreen: params.fullscreen,
        title: Some(title.clone()),
        ..Default::default()
    };
    match vlc::open_stream(url.as_str(), &open_opts) {
        Ok(pid) => {
            record_play_best_effort(
                &name,
                PlayRecord::new(
                    MediaKind::Episode,
                    episode.id,
                    &title,
                    episode.container_extension.clone(),
                ),
            );
            Ok(super::tool_json(&serde_json::json!({
                "playing": title,
                "series_id": show.series_id,
                "episode_title": episode.title,
                "vlc_pid": pid,
                "note": "VLC was launched on the user's machine.",
            })))
        }
        Err(err) => Ok(super::tool_error(err.to_string())),
    }
}
