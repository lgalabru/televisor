use clap::Args;
use televisor_core::Result;
use televisor_core::config::ConfigFile;

#[derive(Args)]
pub struct ListCommand {}

impl ListCommand {
    pub async fn execute(&self) -> Result<()> {
        let file = ConfigFile::open()?;
        // Never emit passwords — names, hosts, kind, and default marker only.
        if crate::no_dna::is_agent() {
            let accounts: Vec<_> = file
                .config
                .accounts
                .iter()
                .map(|(name, account)| {
                    serde_json::json!({
                        "name": name,
                        "host": account.host,
                        "username": account.username,
                        "is_default": file.config.default_account.as_deref() == Some(name.as_str()),
                        "kind": if account.m3u_url.is_some() { "m3u" } else { "xtream" },
                    })
                })
                .collect();
            crate::no_dna::emit_json(&accounts);
            return Ok(());
        }
        if file.config.accounts.is_empty() {
            println!("No accounts configured — run `televisor account add <name> --host <url>`.");
            return Ok(());
        }
        for (name, account) in &file.config.accounts {
            let marker = if file.config.default_account.as_deref() == Some(name.as_str()) {
                " (default)"
            } else {
                ""
            };
            let kind = if account.m3u_url.is_some() {
                " [m3u]"
            } else {
                ""
            };
            println!("{name}{marker}{kind}");
            println!("    host: {}", account.host);
            println!("    user: {}", account.username);
        }
        Ok(())
    }
}
