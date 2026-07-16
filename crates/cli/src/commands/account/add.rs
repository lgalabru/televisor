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
        let username = self
            .username
            .clone()
            .or(host.username)
            .ok_or(ConfigError::MissingUsername)?;
        let password = match self.password.clone().map(Redacted::new).or(host.password) {
            Some(password) => password,
            None => {
                let typed =
                    rpassword::prompt_password("Password: ").map_err(|source| ConfigError::Io {
                        path: "<stdin>".into(),
                        source,
                    })?;
                Redacted::new(typed)
            }
        };

        let mut file = ConfigFile::open()?;
        file.add_account(
            &self.name,
            Account {
                host: host.host,
                username,
                password,
                user_agent: self.user_agent.clone(),
            },
        )?;
        if self.set_default {
            file.set_default(&self.name)?;
        }
        file.save()?;

        let is_default = file.config.default_account.as_deref() == Some(self.name.as_str());
        println!(
            "Added account '{}'{}",
            self.name,
            if is_default { " (default)" } else { "" }
        );
        Ok(())
    }
}
