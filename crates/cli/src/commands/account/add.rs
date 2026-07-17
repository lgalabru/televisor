use clap::Args;
use televisor_core::Result;
use televisor_core::config::{self, Account, ConfigError, ConfigFile};
use televisor_core::redact::Redacted;

#[derive(Args)]
pub struct AddCommand {
    /// Account name (letters, digits, dashes, underscores)
    pub name: String,
    /// Panel host, e.g. http://host:port — a pasted playlist/player_api URL
    /// also works and its embedded username/password are extracted
    #[clap(long)]
    pub host: String,
    /// Treat --host as a plain M3U playlist URL (providers without a JSON
    /// API). Live TV only: no VOD, series, EPG, catch-up, or status.
    #[clap(long, conflicts_with_all = ["username", "password"])]
    pub m3u: bool,
    /// Panel username (or embed it in --host as a playlist URL)
    #[clap(long)]
    pub username: Option<String>,
    /// Panel password. Omit to be prompted — keeps it out of shell history.
    #[clap(long)]
    pub password: Option<String>,
    /// Override the HTTP User-Agent presented to the panel
    #[clap(long)]
    pub user_agent: Option<String>,
    /// Make this account the default (the first account always is)
    #[clap(long)]
    pub set_default: bool,
}

impl AddCommand {
    pub async fn execute(&self) -> Result<()> {
        let host = config::normalize_host(&self.host)?;
        let account = if self.m3u {
            Account {
                host: host.host,
                // Credentials live inside the playlist URL itself; keep the
                // username for display when the URL exposes one.
                username: host.username.unwrap_or_else(|| "m3u".to_string()),
                password: host.password.unwrap_or_else(|| Redacted::new("")),
                user_agent: self.user_agent.clone(),
                m3u_url: Some(Redacted::new(self.host.trim().to_string())),
            }
        } else {
            let username = self
                .username
                .clone()
                .or(host.username)
                .ok_or(ConfigError::MissingUsername)?;
            let password = match self.password.clone().map(Redacted::new).or(host.password) {
                Some(password) => password,
                None => {
                    crate::no_dna::require_interactive("password entry")
                        .map_err(ConfigError::Interactive)?;
                    let typed = rpassword::prompt_password("Password: ").map_err(|source| {
                        ConfigError::Io {
                            path: "<stdin>".into(),
                            source,
                        }
                    })?;
                    Redacted::new(typed)
                }
            };
            Account {
                host: host.host,
                username,
                password,
                user_agent: self.user_agent.clone(),
                m3u_url: None,
            }
        };

        let mut file = ConfigFile::open()?;
        file.add_account(&self.name, account)?;
        if self.set_default {
            file.set_default(&self.name)?;
        }
        file.save()?;

        let is_default = file.config.default_account.as_deref() == Some(self.name.as_str());
        println!(
            "Added {} account '{}'{}",
            if self.m3u { "M3U playlist" } else { "Xtream" },
            self.name,
            if is_default { " (default)" } else { "" }
        );
        Ok(())
    }
}
