use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::vlc::{self, OpenOptions};
use televisor_core::xtream::{StreamFormat, XtreamClient};
use url::Url;

use super::CategoryKind;

#[derive(Args)]
pub struct PlayCommand {
    /// Name or numeric id (no quotes needed: `televisor play fox sports 1`)
    #[clap(required = true)]
    pub query: Vec<String>,
    /// Play a movie (VOD) instead of a live channel
    #[clap(long, conflicts_with = "series")]
    pub movie: bool,
    /// Play a series episode; pair with --season/--episode
    #[clap(long)]
    pub series: bool,
    /// Season number (with --series)
    #[clap(long, short = 's', requires = "series")]
    pub season: Option<i64>,
    /// Episode number (with --series)
    #[clap(long, short = 'e', requires = "series")]
    pub episode: Option<i64>,
    /// Treat the query strictly as an id (for scripts)
    #[clap(long)]
    pub id: bool,
    /// Restrict name matching to one category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// Live container format; try `ts` if m3u8 playback fails on this panel
    #[clap(long, default_value_t = StreamFormat::M3u8)]
    pub format: StreamFormat,
    /// Include adult-flagged entries in name matching
    #[clap(long)]
    pub include_adult: bool,
    /// Refetch the catalog before resolving
    #[clap(long)]
    pub refresh: bool,
    /// Start playback in fullscreen
    #[clap(long, short)]
    pub fullscreen: bool,
    /// Network caching in milliseconds
    #[clap(long)]
    pub caching: Option<u32>,
    /// Quit VLC when the stream ends
    #[clap(long)]
    pub exit_on_end: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl PlayCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, client) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&client, &name)?;
        let query = self.query.join(" ");
        let kind = if self.movie {
            CategoryKind::Vod
        } else if self.series {
            CategoryKind::Series
        } else {
            CategoryKind::Live
        };
        let category =
            super::category_for(&catalog, kind, self.category.as_deref(), self.refresh).await?;

        let opts = ResolveOptions {
            id_only: self.id,
            include_adult: self.include_adult,
            category,
            ..Default::default()
        };
        if self.movie {
            self.play_movie(&client, &catalog, &query, &opts).await
        } else if self.series {
            self.play_episode(&client, &catalog, &query, &opts).await
        } else {
            self.play_live(&client, &catalog, &query, &opts).await
        }
    }

    async fn play_live(
        &self,
        client: &XtreamClient,
        catalog: &Catalog<'_>,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<()> {
        if self.refresh {
            catalog.streams(true).await?;
        }
        match catalog.resolve(query, opts).await? {
            Resolution::One(stream) => {
                let url = client.live_stream_url(stream.stream_id, self.format)?;
                self.launch(&url, &stream.name, stream.stream_id.0)
            }
            Resolution::Ambiguous(candidates) => ambiguous(
                "channels",
                query,
                candidates.iter().map(|c| (c.stream_id.0, c.name.as_str())),
            ),
            Resolution::NotFound => not_found("channel", query, "televisor channels"),
        }
    }

    async fn play_movie(
        &self,
        client: &XtreamClient,
        catalog: &Catalog<'_>,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<()> {
        if self.refresh {
            catalog.movies(true).await?;
        }
        match catalog.resolve_movie(query, opts).await? {
            Resolution::One(movie) => {
                let url =
                    client.movie_url(movie.stream_id, movie.container_extension.as_deref())?;
                self.launch(&url, &movie.name, movie.stream_id.0)
            }
            Resolution::Ambiguous(candidates) => ambiguous(
                "movies",
                query,
                candidates.iter().map(|c| (c.stream_id.0, c.name.as_str())),
            ),
            Resolution::NotFound => not_found("movie", query, "televisor movies"),
        }
    }

    async fn play_episode(
        &self,
        client: &XtreamClient,
        catalog: &Catalog<'_>,
        query: &str,
        opts: &ResolveOptions,
    ) -> Result<()> {
        if self.refresh {
            catalog.series_list(true).await?;
        }
        let show = match catalog.resolve_series(query, opts).await? {
            Resolution::One(show) => show,
            Resolution::Ambiguous(candidates) => {
                return ambiguous(
                    "series",
                    query,
                    candidates.iter().map(|c| (c.series_id, c.name.as_str())),
                );
            }
            Resolution::NotFound => return not_found("series", query, "televisor series"),
        };

        let episodes = client.series_info(show.series_id).await?.episodes();
        let (Some(season), Some(episode_num)) = (self.season, self.episode) else {
            // No episode selected — show what's available instead of guessing.
            println!(
                "{} (id {}) — {} episodes",
                show.name,
                show.series_id,
                episodes.len()
            );
            for episode in &episodes {
                println!(
                    "{}  {:>10}  {}",
                    super::episodes::label(episode),
                    episode.id,
                    super::episodes::title(episode),
                );
            }
            eprintln!(
                "\nPick one: televisor play --series {} --season <N> --episode <N>",
                show.series_id
            );
            std::process::exit(2);
        };

        let Some(episode) = episodes
            .iter()
            .find(|e| e.season == Some(season) && e.episode_num == Some(episode_num))
        else {
            eprintln!(
                "No S{season:02}E{episode_num:02} in '{}' ({} episodes). Run `televisor episodes {}` to list them.",
                show.name,
                episodes.len(),
                show.series_id
            );
            std::process::exit(1);
        };

        let url = client.episode_url(episode.id, episode.container_extension.as_deref())?;
        let title = format!("{} {}", show.name, super::episodes::label(episode));
        self.launch(&url, &title, episode.id)
    }

    fn launch(&self, url: &Url, title: &str, id: u64) -> Result<()> {
        let opts = OpenOptions {
            fullscreen: self.fullscreen,
            caching_ms: self.caching,
            exit_on_end: self.exit_on_end,
            title: Some(title.to_string()),
        };
        vlc::open_stream(url.as_str(), &opts)?;
        println!("▶ {title} (id {id})");
        Ok(())
    }
}

fn ambiguous<'a>(
    what: &str,
    query: &str,
    candidates: impl ExactSizeIterator<Item = (u64, &'a str)>,
) -> Result<()> {
    eprintln!("Ambiguous — {} {what} match '{query}':", candidates.len());
    for (id, name) in candidates {
        eprintln!("{id:>10}  {name}");
    }
    eprintln!("Re-run with a numeric id.");
    std::process::exit(2);
}

fn not_found(what: &str, query: &str, list_command: &str) -> Result<()> {
    eprintln!("No {what} matched '{query}'. Try `{list_command} {query}` or --refresh.");
    std::process::exit(1);
}
