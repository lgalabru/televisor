//! Account configuration: `~/.config/televisor/config.toml`.
//!
//! Two deliberately separate paths:
//! - [`TelevisorConfig::load`] — read-only merged view (file + `TELEVISOR_*` env).
//! - [`ConfigFile`] — direct file read → mutate → atomic write, used by
//!   `account add/remove/set-default`. Mutations never go through the merged
//!   view, otherwise env-provided secrets would get baked into the file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Format, Toml};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::redact::Redacted;

/// Name under which the `TELEVISOR_HOST`/`TELEVISOR_USERNAME`/`TELEVISOR_PASSWORD`
/// env triple is exposed.
pub const ENV_ACCOUNT: &str = "env";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not determine the config directory (HOME is not set)")]
    NoConfigDir,

    #[error("No accounts configured — run `televisor account add <name> --host <url>`")]
    NoAccounts,

    #[error("Account '{name}' not found. Available accounts: {}", available.join(", "))]
    AccountNotFound {
        name: String,
        available: Vec<String>,
    },

    #[error(
        "Multiple accounts configured but no default set — pass --account <name> or run `televisor account set-default <name>`. Available: {}",
        available.join(", ")
    )]
    NoDefaultAccount { available: Vec<String> },

    #[error("Account '{0}' already exists — remove it first or pick another name")]
    AccountExists(String),

    #[error("No username — pass --username or paste a playlist URL that embeds it")]
    MissingUsername,

    #[error("{0}")]
    Interactive(String),

    #[error("Invalid account name '{0}' — use letters, digits, dashes, and underscores only")]
    InvalidAccountName(String),

    // The raw input is never echoed: pasted playlist URLs embed credentials.
    #[error("Invalid host URL ({reason}) — expected something like http://host:port")]
    InvalidHost { reason: &'static str },

    #[error("Config I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // Summarized location only — parser messages can echo file values.
    #[error("Config file is invalid: {0}")]
    Parse(String),
}

type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Panel origin, e.g. `http://host:port` — no path, no credentials.
    pub host: String,
    pub username: String,
    pub password: Redacted,
    /// Overrides [`crate::xtream::DEFAULT_USER_AGENT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Full playlist URL for plain-M3U providers (no JSON API). When set,
    /// the account is live-TV-only. Embeds credentials — hence `Redacted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m3u_url: Option<Redacted>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelevisorConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_account: Option<String>,
    /// Media player: "vlc" (default), "mpv", "iina", "ffplay", "system", or a
    /// custom command template with `{url}`/`{title}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
    /// True when the `env` account was synthesized from `TELEVISOR_HOST/...`.
    #[serde(skip)]
    env_account: bool,
}

impl TelevisorConfig {
    /// Merged read-only view: config file, then env.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let mut config: TelevisorConfig = Figment::new()
            .merge(Toml::file(path))
            .extract()
            .map_err(summarize_figment_error)?;

        if let (Ok(host), Ok(username), Ok(password)) = (
            std::env::var("TELEVISOR_HOST"),
            std::env::var("TELEVISOR_USERNAME"),
            std::env::var("TELEVISOR_PASSWORD"),
        ) {
            let normalized = normalize_host(&host)?;
            config.accounts.insert(
                ENV_ACCOUNT.to_string(),
                Account {
                    host: normalized.host,
                    username,
                    password: Redacted::new(password),
                    user_agent: std::env::var("TELEVISOR_USER_AGENT").ok(),
                    m3u_url: None,
                },
            );
            config.env_account = true;
        }
        Ok(config)
    }

    /// Pick an account. Precedence: explicit `--account` flag > env triple >
    /// `TELEVISOR_ACCOUNT` > `default_account` > sole configured account.
    pub fn resolve_account(&self, explicit: Option<&str>) -> Result<(String, &Account)> {
        if let Some(name) = explicit {
            return self.get(name);
        }
        if self.env_account {
            return self.get(ENV_ACCOUNT);
        }
        if let Ok(name) = std::env::var("TELEVISOR_ACCOUNT") {
            return self.get(&name);
        }
        if let Some(name) = &self.default_account {
            return self.get(name);
        }
        match self.accounts.len() {
            0 => Err(ConfigError::NoAccounts),
            1 => {
                let (name, account) = self.accounts.iter().next().expect("len checked");
                Ok((name.clone(), account))
            }
            _ => Err(ConfigError::NoDefaultAccount {
                available: self.accounts.keys().cloned().collect(),
            }),
        }
    }

    fn get(&self, name: &str) -> Result<(String, &Account)> {
        self.accounts
            .get(name)
            .map(|account| (name.to_string(), account))
            .ok_or_else(|| ConfigError::AccountNotFound {
                name: name.to_string(),
                available: self.accounts.keys().cloned().collect(),
            })
    }
}

/// Read-modify-write handle on the config file itself (no env merging).
pub struct ConfigFile {
    path: PathBuf,
    pub config: TelevisorConfig,
}

impl ConfigFile {
    pub fn open() -> Result<Self> {
        Self::open_at(config_path()?)
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let config = match std::fs::read_to_string(&path) {
            Ok(raw) => toml::from_str(&raw).map_err(summarize_toml_error)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => TelevisorConfig::default(),
            Err(source) => return Err(ConfigError::Io { path, source }),
        };
        Ok(Self { path, config })
    }

    pub fn add_account(&mut self, name: &str, account: Account) -> Result<()> {
        validate_account_name(name)?;
        if self.config.accounts.contains_key(name) {
            return Err(ConfigError::AccountExists(name.to_string()));
        }
        // First account becomes the default automatically.
        if self.config.accounts.is_empty() {
            self.config.default_account = Some(name.to_string());
        }
        self.config.accounts.insert(name.to_string(), account);
        Ok(())
    }

    pub fn remove_account(&mut self, name: &str) -> Result<()> {
        if self.config.accounts.remove(name).is_none() {
            return Err(ConfigError::AccountNotFound {
                name: name.to_string(),
                available: self.config.accounts.keys().cloned().collect(),
            });
        }
        if self.config.default_account.as_deref() == Some(name) {
            self.config.default_account = None;
        }
        Ok(())
    }

    pub fn set_default(&mut self, name: &str) -> Result<()> {
        if !self.config.accounts.contains_key(name) {
            return Err(ConfigError::AccountNotFound {
                name: name.to_string(),
                available: self.config.accounts.keys().cloned().collect(),
            });
        }
        self.config.default_account = Some(name.to_string());
        Ok(())
    }

    /// Atomic save: temp file in the same directory, chmod 0600, rename.
    /// A crash mid-write must never destroy the existing accounts.
    pub fn save(&self) -> Result<()> {
        let dir = self.path.parent().ok_or(ConfigError::NoConfigDir)?;
        std::fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        restrict_permissions(dir, 0o700)?;

        let raw = toml::to_string_pretty(&self.config)
            .map_err(|err| ConfigError::Parse(err.to_string()))?;

        let tmp = self.path.with_extension("toml.tmp");
        let io_err = |source| ConfigError::Io {
            path: tmp.clone(),
            source,
        };
        let mut file = std::fs::File::create(&tmp).map_err(io_err)?;
        restrict_permissions(&tmp, 0o600)?;
        file.write_all(raw.as_bytes()).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        drop(file);
        std::fs::rename(&tmp, &self.path).map_err(|source| ConfigError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }
}

/// Host input accepted by `account add`: a bare host, an origin, or a full
/// provider URL (`player_api.php?...`/`get.php?...`) whose embedded
/// credentials are extracted — people paste their M3U link constantly.
#[derive(Debug)]
pub struct HostInput {
    pub host: String,
    pub username: Option<String>,
    pub password: Option<Redacted>,
}

pub fn normalize_host(raw: &str) -> Result<HostInput> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ConfigError::InvalidHost { reason: "empty" });
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let url = Url::parse(&with_scheme).map_err(|_| ConfigError::InvalidHost {
        reason: "not a valid URL",
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConfigError::InvalidHost {
            reason: "scheme must be http or https",
        });
    }
    if url.host_str().is_none() {
        return Err(ConfigError::InvalidHost {
            reason: "missing hostname",
        });
    }

    let mut username = None;
    let mut password = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "username" => username = Some(value.into_owned()),
            "password" => password = Some(Redacted::new(value.into_owned())),
            _ => {}
        }
    }
    if !url.username().is_empty() {
        username.get_or_insert_with(|| url.username().to_string());
        if let Some(pass) = url.password() {
            password.get_or_insert_with(|| Redacted::new(pass.to_string()));
        }
    }

    Ok(HostInput {
        host: url.origin().ascii_serialization(),
        username,
        password,
    })
}

pub fn validate_account_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(ConfigError::InvalidAccountName(name.to_string()))
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// `$XDG_CONFIG_HOME/televisor`, else `~/.config/televisor`. Deliberately not
/// `dirs::config_dir()` on macOS (that's `~/Library/Application Support`);
/// only Windows falls back to the platform dir.
pub fn config_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return dirs::config_dir()
            .map(|dir| dir.join("televisor"))
            .ok_or(ConfigError::NoConfigDir);
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or(ConfigError::NoConfigDir)?;
        Ok(base.join("televisor"))
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|source| {
        ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// Location only — figment messages can echo offending values.
fn summarize_figment_error(err: figment::Error) -> ConfigError {
    let keys: Vec<String> = err
        .into_iter()
        .map(|e| {
            if e.path.is_empty() {
                "<root>".to_string()
            } else {
                e.path.join(".")
            }
        })
        .collect();
    ConfigError::Parse(format!("problem at key(s): {}", keys.join(", ")))
}

/// Location only — toml messages include a snippet of the file, which may
/// contain the password line.
fn summarize_toml_error(err: toml::de::Error) -> ConfigError {
    match err.span() {
        Some(span) => ConfigError::Parse(format!("invalid TOML near byte {}", span.start)),
        None => ConfigError::Parse("invalid TOML".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account() -> Account {
        Account {
            host: "http://panel.example:8080".to_string(),
            username: "alice".to_string(),
            password: Redacted::new("hunter2"),
            user_agent: None,
            m3u_url: None,
        }
    }

    #[test]
    fn normalize_bare_host_gets_http_scheme() {
        let input = normalize_host("panel.example:8080").unwrap();
        assert_eq!(input.host, "http://panel.example:8080");
        assert!(input.username.is_none());
    }

    #[test]
    fn normalize_strips_trailing_slash_and_path() {
        let input = normalize_host("http://panel.example/").unwrap();
        assert_eq!(input.host, "http://panel.example");
    }

    #[test]
    fn normalize_extracts_credentials_from_pasted_playlist_url() {
        let input = normalize_host(
            "http://panel.example:8080/get.php?username=alice&password=hunter2&type=m3u_plus",
        )
        .unwrap();
        assert_eq!(input.host, "http://panel.example:8080");
        assert_eq!(input.username.as_deref(), Some("alice"));
        assert_eq!(input.password.unwrap().expose(), "hunter2");
    }

    #[test]
    fn normalize_rejects_bad_input_without_echoing_it() {
        let err = normalize_host("ftp://panel.example").unwrap_err();
        assert!(!err.to_string().contains("panel.example"));
        assert!(normalize_host("").is_err());
    }

    #[test]
    fn config_error_messages_never_contain_password() {
        // The host-parse failure path receives strings that may embed creds.
        let err =
            normalize_host("http://panel:__invalid__host__/get.php?password=hunter2").unwrap_err();
        assert!(!err.to_string().contains("hunter2"));
    }

    #[test]
    fn add_first_account_becomes_default_and_saves_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut file = ConfigFile::open_at(path.clone()).unwrap();
        file.add_account("home", sample_account()).unwrap();
        file.save().unwrap();

        assert!(!path.with_extension("toml.tmp").exists());
        let reloaded = ConfigFile::open_at(path.clone()).unwrap();
        assert_eq!(reloaded.config.default_account.as_deref(), Some("home"));
        assert_eq!(
            reloaded.config.accounts["home"].password.expose(),
            "hunter2"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn duplicate_account_and_bad_names_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::open_at(dir.path().join("config.toml")).unwrap();
        file.add_account("home", sample_account()).unwrap();
        assert!(matches!(
            file.add_account("home", sample_account()),
            Err(ConfigError::AccountExists(_))
        ));
        assert!(matches!(
            file.add_account("bad name", sample_account()),
            Err(ConfigError::InvalidAccountName(_))
        ));
        assert!(matches!(
            file.add_account("bad/name", sample_account()),
            Err(ConfigError::InvalidAccountName(_))
        ));
    }

    #[test]
    fn remove_account_clears_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = ConfigFile::open_at(dir.path().join("config.toml")).unwrap();
        file.add_account("home", sample_account()).unwrap();
        file.remove_account("home").unwrap();
        assert!(file.config.default_account.is_none());
        assert!(matches!(
            file.remove_account("home"),
            Err(ConfigError::AccountNotFound { .. })
        ));
    }

    #[test]
    fn resolve_account_precedence() {
        let mut config = TelevisorConfig::default();
        config.accounts.insert("home".to_string(), sample_account());

        // Sole account works without a default.
        assert_eq!(config.resolve_account(None).unwrap().0, "home");
        // Explicit flag wins and unknown names list what exists.
        assert_eq!(config.resolve_account(Some("home")).unwrap().0, "home");
        let err = config.resolve_account(Some("nope")).unwrap_err();
        assert!(err.to_string().contains("home"));

        // Two accounts and no default → actionable error.
        config.accounts.insert("work".to_string(), sample_account());
        assert!(matches!(
            config.resolve_account(None),
            Err(ConfigError::NoDefaultAccount { .. })
        ));
        config.default_account = Some("work".to_string());
        assert_eq!(config.resolve_account(None).unwrap().0, "work");
    }

    #[test]
    fn config_debug_never_prints_password() {
        let mut config = TelevisorConfig::default();
        config.accounts.insert("home".to_string(), sample_account());
        assert!(!format!("{config:?}").contains("hunter2"));
    }
}
