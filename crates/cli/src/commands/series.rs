use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{self, Catalog, ResolveOptions};

#[derive(Args)]
pub struct SeriesCommand {
    /// Search terms, matched case-insensitively against series names
    pub query: Vec<String>,
    /// Restrict to one series category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// List everything even without a filter (thousands of lines)
    #[clap(long)]
    pub all: bool,
    /// Show the most recently updated first (default 25, see --limit)
    #[clap(long)]
    pub recent: bool,
    /// Max results with --recent
    #[clap(long, default_value_t = 25)]
    pub limit: usize,
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

impl SeriesCommand {
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
        let series = catalog.series_list(self.refresh).await?;

        if query.is_empty() && category.is_none() && !self.all && !self.recent {
            println!(
                "{} series available — pass a search term, --category, or --all to list them.",
                series.len()
            );
            return Ok(());
        }

        let opts = ResolveOptions {
            category,
            ..Default::default()
        };
        let mut results = catalog::search(&series, &query, &opts, usize::MAX);
        if self.recent {
            results.sort_by_key(|s| std::cmp::Reverse(s.last_modified.unwrap_or(0)));
            results.truncate(self.limit);
        }

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&results).expect("series serialize")
            );
        } else if results.is_empty() {
            println!("No series matched.");
        } else {
            for show in &results {
                println!("{:>10}  {}", show.series_id, show.name);
            }
        }
        Ok(())
    }
}
