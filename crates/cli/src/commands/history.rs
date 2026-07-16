use clap::Args;
use televisor_core::Result;
use televisor_core::state::StateFile;

#[derive(Args)]
pub struct HistoryCommand {
    /// Entries to show
    #[clap(long, default_value_t = 15)]
    pub limit: usize,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl HistoryCommand {
    pub async fn execute(&self) -> Result<()> {
        let (account, _source) = super::client_for(self.account.as_deref())?;
        let file = StateFile::open()?;
        let history = file.history(&account);
        if history.is_empty() {
            println!("Nothing played yet.");
            return Ok(());
        }
        for record in history.iter().take(self.limit) {
            println!(
                "{}  {:>10}  [{}]  {}",
                relative(record.played_at),
                record.id,
                record.kind,
                record.name
            );
        }
        println!("\nReplay the most recent: televisor play --last");
        Ok(())
    }
}

fn relative(epoch: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ago = now.saturating_sub(epoch);
    match ago {
        0..60 => "  just now".to_string(),
        60..3_600 => format!("{:>6}m ago", ago / 60),
        3_600..86_400 => format!("{:>6}h ago", ago / 3_600),
        _ => format!("{:>6}d ago", ago / 86_400),
    }
}
