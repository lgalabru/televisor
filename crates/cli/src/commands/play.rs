use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::vlc::{self, OpenOptions};
use televisor_core::xtream::{LiveStream, StreamFormat};

#[derive(Args)]
pub struct PlayCommand {
    /// Channel name or numeric stream id (no quotes needed: `televisor play fox sports 1`)
    #[clap(required = true)]
    pub query: Vec<String>,
    /// Treat the query strictly as a stream id (for scripts)
    #[clap(long)]
    pub id: bool,
    /// Restrict name matching to one category (id or name)
    #[clap(long)]
    pub category: Option<String>,
    /// Container format; try `ts` if m3u8 playback fails on this panel
    #[clap(long, default_value_t = StreamFormat::M3u8)]
    pub format: StreamFormat,
    /// Include adult-flagged channels in name matching
    #[clap(long)]
    pub include_adult: bool,
    /// Refetch the channel list before resolving
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
        let category =
            super::category_for(&catalog, self.category.as_deref(), self.refresh).await?;
        if self.refresh {
            catalog.streams(true).await?;
        }

        let opts = ResolveOptions {
            id_only: self.id,
            include_adult: self.include_adult,
            category,
            ..Default::default()
        };
        match catalog.resolve(&query, &opts).await? {
            Resolution::One(stream) => self.launch(&client, &stream),
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} channels match '{query}':", candidates.len());
                for candidate in &candidates {
                    eprintln!("{:>10}  {}", candidate.stream_id, candidate.name);
                }
                eprintln!("Re-run with a stream id: televisor play <id>");
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!(
                    "No channel matched '{query}'. Try `televisor channels {query}` or --refresh."
                );
                std::process::exit(1);
            }
        }
    }

    fn launch(
        &self,
        client: &televisor_core::xtream::XtreamClient,
        stream: &LiveStream,
    ) -> Result<()> {
        let url = client.live_stream_url(stream.stream_id, self.format)?;
        let opts = OpenOptions {
            fullscreen: self.fullscreen,
            caching_ms: self.caching,
            exit_on_end: self.exit_on_end,
            title: Some(stream.name.clone()),
        };
        vlc::open_stream(url.as_str(), &opts)?;
        println!("▶ {} (id {})", stream.name, stream.stream_id);
        Ok(())
    }
}
