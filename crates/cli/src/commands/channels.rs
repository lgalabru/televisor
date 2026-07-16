use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{self, Catalog, ResolveOptions};

#[derive(Args)]
pub struct ChannelsCommand {
    /// Search terms, matched case-insensitively against channel names
    pub query: Vec<String>,
    /// Restrict to one category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// List everything even without a filter (tens of thousands of lines)
    #[clap(long)]
    pub all: bool,
    /// Only channels with catch-up/replay archive
    #[clap(long)]
    pub archive: bool,
    /// Include adult-flagged channels
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

impl ChannelsCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, source) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&source, &name)?;
        let query = self.query.join(" ");
        let category = super::category_for(
            &catalog,
            super::CategoryKind::Live,
            self.category.as_deref(),
            self.refresh,
        )
        .await?;
        let streams = catalog.streams(self.refresh).await?;

        if query.is_empty() && category.is_none() && !self.all && !self.archive {
            println!(
                "{} channels available — pass a search term, --category, or --all to list them.",
                streams.len()
            );
            return Ok(());
        }

        let opts = ResolveOptions {
            include_adult: self.include_adult,
            category,
            ..Default::default()
        };
        let mut results = catalog::search(&streams, &query, &opts, usize::MAX);
        if self.archive {
            results.retain(|s| s.tv_archive);
        }

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&results).expect("streams serialize")
            );
        } else if results.is_empty() {
            println!("No channels matched.");
        } else {
            for stream in &results {
                let archive = match (stream.tv_archive, stream.tv_archive_duration) {
                    (true, Some(days)) => format!("  [replay {days}d]"),
                    (true, None) => "  [replay]".to_string(),
                    _ => String::new(),
                };
                println!("{:>10}  {}{archive}", stream.stream_id, stream.name);
            }
        }
        Ok(())
    }
}
