use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::xtream::Episode;

#[derive(Args)]
pub struct EpisodesCommand {
    /// Series name or numeric series id
    #[clap(required = true)]
    pub query: Vec<String>,
    /// Restrict name matching to one series category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// Refetch the series list before resolving
    #[clap(long)]
    pub refresh: bool,
    /// Print JSON instead of a table
    #[clap(long)]
    pub json: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl EpisodesCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, source) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&source, &name)?;
        let query = self.query.join(" ");
        let category = super::category_for(
            &catalog,
            super::CategoryKind::Series,
            self.category.as_deref(),
            self.refresh,
        )
        .await?;
        if self.refresh {
            catalog.series_list(true).await?;
        }

        let opts = ResolveOptions {
            category,
            ..Default::default()
        };
        let show = match catalog.resolve_series(&query, &opts).await? {
            Resolution::One(show) => show,
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} series match '{query}':", candidates.len());
                for candidate in &candidates {
                    eprintln!("{:>10}  {}", candidate.series_id, candidate.name);
                }
                eprintln!("Re-run with a series id: televisor episodes <id>");
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!("No series matched '{query}'. Try `televisor series {query}`.");
                std::process::exit(1);
            }
        };

        let episodes = source
            .xtream()?
            .series_info(show.series_id)
            .await?
            .episodes();
        if crate::no_dna::should_json(self.json) {
            crate::no_dna::emit_json(&serde_json::json!({
                "series": show.name,
                "series_id": show.series_id,
                "episodes": episodes,
            }));
            return Ok(());
        }

        println!(
            "{} (id {}) — {} episodes",
            show.name,
            show.series_id,
            episodes.len()
        );
        for episode in &episodes {
            println!("{}  {:>10}  {}", label(episode), episode.id, title(episode));
        }
        println!(
            "\nPlay one: televisor play --series {} --season <N> --episode <N>",
            show.series_id
        );
        Ok(())
    }
}

pub(crate) fn label(episode: &Episode) -> String {
    format!(
        "S{:02}E{:02}",
        episode.season.unwrap_or(0),
        episode.episode_num.unwrap_or(0)
    )
}

pub(crate) fn title(episode: &Episode) -> &str {
    episode.title.as_deref().unwrap_or("(untitled)")
}
