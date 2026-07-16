use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{self, Catalog, ResolveOptions};

#[derive(Args)]
pub struct MoviesCommand {
    /// Search terms, matched case-insensitively against movie names
    pub query: Vec<String>,
    /// Restrict to one VOD category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// List everything even without a filter (tens of thousands of lines)
    #[clap(long)]
    pub all: bool,
    /// Include adult-flagged movies
    #[clap(long)]
    pub include_adult: bool,
    /// Refetch from the panel instead of the local cache
    #[clap(long)]
    pub refresh: bool,
    /// Print JSON instead of a table
    #[clap(long)]
    pub json: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl MoviesCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, source) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&source, &name)?;
        let query = self.query.join(" ");
        let category = super::category_for(
            &catalog,
            super::CategoryKind::Vod,
            self.category.as_deref(),
            self.refresh,
        )
        .await?;
        let movies = catalog.movies(self.refresh).await?;

        if query.is_empty() && category.is_none() && !self.all {
            println!(
                "{} movies available — pass a search term, --category, or --all to list them.",
                movies.len()
            );
            return Ok(());
        }

        let opts = ResolveOptions {
            include_adult: self.include_adult,
            category,
            ..Default::default()
        };
        let results = catalog::search(&movies, &query, &opts, usize::MAX);

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&results).expect("movies serialize")
            );
        } else if results.is_empty() {
            println!("No movies matched.");
        } else {
            for movie in &results {
                println!("{:>10}  {}", movie.stream_id, movie.name);
            }
        }
        Ok(())
    }
}
