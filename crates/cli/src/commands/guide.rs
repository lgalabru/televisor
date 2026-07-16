use clap::Args;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::xtream::EpgListing;

#[derive(Args)]
pub struct GuideCommand {
    /// Channel name or numeric stream id
    #[clap(required = true)]
    pub query: Vec<String>,
    /// Show the full schedule instead of now/next
    #[clap(long)]
    pub full: bool,
    /// Number of programmes to show (with now/next)
    #[clap(long, default_value_t = 8)]
    pub limit: u32,
    /// Restrict channel matching to one category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// Print JSON instead of a table
    #[clap(long)]
    pub json: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl GuideCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, client) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&client, &name)?;
        let query = self.query.join(" ");
        let category = super::category_for(
            &catalog,
            super::CategoryKind::Live,
            self.category.as_deref(),
            false,
        )
        .await?;

        let opts = ResolveOptions {
            category,
            ..Default::default()
        };
        let stream = match catalog.resolve(&query, &opts).await? {
            Resolution::One(stream) => stream,
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} channels match '{query}':", candidates.len());
                for candidate in &candidates {
                    eprintln!("{:>10}  {}", candidate.stream_id, candidate.name);
                }
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!("No channel matched '{query}'. Try `televisor channels {query}`.");
                std::process::exit(1);
            }
        };

        let listings = if self.full {
            client.full_epg(stream.stream_id).await?
        } else {
            client.short_epg(stream.stream_id, self.limit).await?
        };

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&listings).expect("listings serialize")
            );
            return Ok(());
        }

        if listings.is_empty() {
            println!(
                "No EPG data for {} (id {}) — many channels simply aren't mapped to a guide.",
                stream.name, stream.stream_id
            );
            return Ok(());
        }

        println!("{} (id {})", stream.name, stream.stream_id);
        let now = Timestamp::now().as_second();
        let tz = TimeZone::system();
        for listing in &listings {
            let marker = if listing.is_on_air(now) { "▶" } else { " " };
            println!(
                "{marker} {}  {}",
                format_window(listing, &tz),
                listing.title.as_deref().unwrap_or("(untitled)")
            );
        }
        Ok(())
    }
}

/// "07-16 14:00–17:00" in the local timezone (date shown once per window).
fn format_window(listing: &EpgListing, tz: &TimeZone) -> String {
    let fmt = |epoch: Option<i64>, with_date: bool| -> String {
        let Some(epoch) = epoch else {
            return "??:??".to_string();
        };
        match Timestamp::from_second(epoch) {
            Ok(ts) => {
                let zoned = ts.to_zoned(tz.clone());
                if with_date {
                    zoned.strftime("%m-%d %H:%M").to_string()
                } else {
                    zoned.strftime("%H:%M").to_string()
                }
            }
            Err(_) => "??:??".to_string(),
        }
    };
    format!(
        "{}–{}",
        fmt(listing.start_timestamp, true),
        fmt(listing.stop_timestamp, false)
    )
}
