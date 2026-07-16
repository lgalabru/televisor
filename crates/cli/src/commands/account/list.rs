use clap::Args;
use televisor_core::Result;
use televisor_core::config::ConfigFile;

#[derive(Args)]
pub struct ListCommand {}

impl ListCommand {
    pub async fn execute(&self) -> Result<()> {
        let file = ConfigFile::open()?;
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
