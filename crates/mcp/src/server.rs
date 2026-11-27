//! MCP server — thin dispatch layer.
//!
//! Each tool's logic and params live in `tools/<name>.rs`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};

use crate::tools;

const INSTRUCTIONS: &str = "televisor controls IPTV playback through VLC on the user's machine, \
covering live TV (with EPG), movies (VOD), and series. Typical flows: search_channels → \
play_channel for live TV (get_epg shows what's airing now/next on a channel); \
search_movies → play_movie for films; search_series → get_series_episodes → \
play_episode for shows. Live TV, movies, and series each have their own category lists. \
Playback URLs embed the user's panel credentials, so tools launch VLC directly and never \
return stream URLs; account credentials can only be added via the `televisor account add` CLI.";

pub struct TelevisorMcp {
    #[allow(dead_code)]
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl Default for TelevisorMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl TelevisorMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List the configured IPTV panel accounts (name, host, default marker). \
Call this first when a tool reports the account is missing or ambiguous. Accounts can only \
be added or removed by the user via the `televisor account add/remove` CLI — never through MCP."
    )]
    async fn list_accounts(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::list_accounts::run().await
    }

    #[tool(
        description = "Check an IPTV account's subscription status: active/expired, expiry \
epoch (null = unlimited), trial flag, and active/max concurrent connections. Use this to \
diagnose auth failures or to tell the user when their subscription runs out."
    )]
    async fn get_status(
        &self,
        Parameters(params): Parameters<tools::get_status::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::get_status::run(params).await
    }

    #[tool(
        description = "List the panel's live-TV categories (id + name), e.g. sports, news, \
per-country groups. Real panels have hundreds of categories and tens of thousands of \
channels — use a category to scope search_channels when a name query alone is too broad."
    )]
    async fn list_categories(
        &self,
        Parameters(params): Parameters<tools::list_categories::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::list_categories::run(params).await
    }

    #[tool(
        description = "Search live channels by name substring, optionally scoped to a \
category. Returns ranked matches with stream_ids plus the total match count. This is the \
discovery step before play_channel. Results come from a local cache of the panel's channel \
list (~24h TTL); pass refresh=true if the lineup seems stale."
    )]
    async fn search_channels(
        &self,
        Parameters(params): Parameters<tools::search_channels::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::search_channels::run(params).await
    }

    #[tool(
        description = "Resolve a channel (numeric stream_id preferred, or a name to \
fuzzy-match) and start playing it in VLC on the user's machine. On an ambiguous name the \
response lists candidate stream_ids — pick one and call again. Playback runs locally; no \
stream URL is returned. If the user reports no video, retry once with format=\"ts\"."
    )]
    async fn play_channel(
        &self,
        Parameters(params): Parameters<tools::play_channel::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::play_channel::run(params).await
    }

    #[tool(
        description = "Search movies (VOD) by name substring, optionally scoped to a VOD \
category id or name keyword. Set query=\"\" with a category to list movies in that category; \
set all=true for the complete category or use offset/max_results to page through it. Returns \
ranked matches with stream_ids plus the total match count. Category searches fetch/cache only \
that category for speed. This is the discovery step before play_movie."
    )]
    async fn search_movies(
        &self,
        Parameters(params): Parameters<tools::search_movies::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::search_movies::run(params).await
    }

    #[tool(
        description = "Play a movie in VLC on the user's machine (numeric stream_id from \
search_movies preferred, or a name to fuzzy-match). On an ambiguous name the response lists \
candidate stream_ids — pick one and call again. No stream URL is returned."
    )]
    async fn play_movie(
        &self,
        Parameters(params): Parameters<tools::play_movie::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::play_movie::run(params).await
    }

    #[tool(
        description = "Full details for one movie: plot, genre, rating, duration, director, \
cast, release date, trailer. Use after search_movies when the user is choosing what to watch \
(e.g. \"is it any good?\", \"what's it about?\")."
    )]
    async fn get_movie_info(
        &self,
        Parameters(params): Parameters<tools::get_movie_info::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::get_movie_info::run(params).await
    }

    #[tool(
        description = "Search series (TV shows) by name substring, optionally scoped to a \
series category. Returns ranked matches with series_ids. A series is not directly playable — \
follow up with get_series_episodes, then play_episode."
    )]
    async fn search_series(
        &self,
        Parameters(params): Parameters<tools::search_series::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::search_series::run(params).await
    }

    #[tool(
        description = "List a series' episodes (season + episode numbers and titles), \
resolved from a series_id (preferred) or show name. Call this between search_series and \
play_episode to know which seasons/episodes exist."
    )]
    async fn get_series_episodes(
        &self,
        Parameters(params): Parameters<tools::get_series_episodes::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::get_series_episodes::run(params).await
    }

    #[tool(
        description = "Play one episode of a series in VLC on the user's machine. Takes the \
series (series_id preferred, or name), a season number, and an episode number. If the \
episode doesn't exist the response lists what's available. No stream URL is returned."
    )]
    async fn play_episode(
        &self,
        Parameters(params): Parameters<tools::play_episode::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::play_episode::run(params).await
    }

    #[tool(
        description = "Show what's on a live channel: now/next programmes (or the full schedule \
with full=true) with titles, descriptions, epoch timestamps, and a now_playing marker. Use this \
to answer questions like \"what's on X right now\" or to pick a channel before play_channel. \
Many channels have no guide data — that is normal and doesn't affect playback."
    )]
    async fn get_epg(
        &self,
        Parameters(params): Parameters<tools::get_epg::Params>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        tools::get_epg::run(params).await
    }
}

#[tool_handler]
impl ServerHandler for TelevisorMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation::from_build_env(),
            instructions: Some(INSTRUCTIONS.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_info_advertises_tools_and_instructions() {
        let server = TelevisorMcp::new();
        let info = server.get_info();
        assert!(info.capabilities.tools.is_some());
        let instructions = info.instructions.unwrap();
        assert!(instructions.contains("search_channels"));
        assert!(instructions.contains("play_channel"));
    }
}
