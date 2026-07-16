//! `televisor setup` — interactive first-run: prompt for the panel account,
//! verify it against the panel, and install the MCP server into Claude Code,
//! Claude Desktop, and Codex (same process as `pay setup`).

use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, Password};
use televisor_core::config::{self, Account, ConfigError, ConfigFile};
use televisor_core::redact::Redacted;
use televisor_core::xtream::XtreamClient;
use televisor_core::{Result, TelevisorError};
use toml_edit::{Array, DocumentMut, Item, Table, Value, value};

#[derive(clap::Args)]
pub struct SetupCommand {
    /// Replace the existing account instead of aborting
    #[clap(long)]
    pub force: bool,

    /// Re-install the MCP configs without touching accounts
    #[clap(long)]
    pub update: bool,
}

impl SetupCommand {
    pub async fn execute(&self) -> Result<()> {
        if self.update {
            install_mcp_configs();
            return Ok(());
        }

        let mut file = ConfigFile::open()?;
        if !file.config.accounts.is_empty() && !self.force {
            let names: Vec<&str> = file.config.accounts.keys().map(String::as_str).collect();
            println!("Account already configured: {}", names.join(", "));
            println!(
                "Use `televisor setup --force` to replace it, or `televisor account add` for another.\n"
            );
            install_mcp_configs();
            return Ok(());
        }

        let account_name = self.prompt_account(&mut file).await?;
        install_mcp_configs();

        println!("Setup complete. Try:");
        println!("  televisor play <channel>");
        println!("  claude -p \"what's on {account_name}? find me a sports channel\"");
        Ok(())
    }

    /// Prompt for the account, verify it against the panel, and save it.
    async fn prompt_account(&self, file: &mut ConfigFile) -> Result<String> {
        let theme = ColorfulTheme::default();

        let name: String = Input::with_theme(&theme)
            .with_prompt("Account name")
            .default("home".to_string())
            .validate_with(|input: &String| {
                config::validate_account_name(input).map_err(|e| e.to_string())
            })
            .interact_text()
            .map_err(prompt_error)?;

        let host_raw: String = Input::with_theme(&theme)
            .with_prompt("Panel host or playlist URL (credentials in the URL are extracted)")
            .validate_with(|input: &String| {
                config::normalize_host(input)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .interact_text()
            .map_err(prompt_error)?;
        let host = config::normalize_host(&host_raw)?;

        let username = match host.username {
            Some(username) => {
                println!("  Username taken from URL: {username}");
                username
            }
            None => Input::with_theme(&theme)
                .with_prompt("Username")
                .interact_text()
                .map_err(prompt_error)?,
        };
        let password = match host.password {
            Some(password) => {
                println!("  Password taken from URL: [redacted]");
                password
            }
            None => Redacted::new(
                Password::with_theme(&theme)
                    .with_prompt("Password")
                    .interact()
                    .map_err(prompt_error)?,
            ),
        };

        let account = Account {
            host: host.host,
            username,
            password,
            user_agent: None,
        };

        println!("Verifying credentials against the panel…");
        let client = XtreamClient::new(&name, &account)?;
        match client.account_info().await {
            Ok(summary) => {
                println!(
                    "  ✔ {} — {} max connection(s)",
                    summary.user_info.status.as_deref().unwrap_or("OK"),
                    summary
                        .user_info
                        .max_connections
                        .map_or_else(|| "?".into(), |n| n.to_string()),
                );
            }
            Err(err) => {
                eprintln!("  ! Verification failed: {err}");
                let save_anyway = Confirm::with_theme(&theme)
                    .with_prompt("Save the account anyway?")
                    .default(false)
                    .interact()
                    .map_err(prompt_error)?;
                if !save_anyway {
                    return Err(TelevisorError::Xtream(err));
                }
            }
        }

        if self.force {
            let _ = file.remove_account(&name);
        }
        file.add_account(&name, account)?;
        file.save()?;
        println!("  ✔ Account '{name}' saved\n");
        Ok(name)
    }
}

fn prompt_error(err: dialoguer::Error) -> TelevisorError {
    let dialoguer::Error::IO(source) = err;
    ConfigError::Io {
        path: "<terminal>".into(),
        source,
    }
    .into()
}

// ── MCP config installation (same process as `pay setup`) ──────────────────

const MCP_KEY: &str = "televisor";

/// Resolve the televisor binary path for the MCP config.
fn televisor_command() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "televisor".to_string())
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(std::path::PathBuf::from)
}

/// Well-known MCP config locations (JSON) for each supported app.
fn mcp_config_targets() -> Vec<(&'static str, std::path::PathBuf)> {
    let mut targets = Vec::new();
    if let Some(home) = home_dir() {
        targets.push(("Claude Code", home.join(".claude.json")));
    }
    if let Some(path) = claude_desktop_config_path() {
        targets.push(("Claude Desktop", path));
    }
    targets
}

fn claude_desktop_config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|appdata| {
            std::path::PathBuf::from(appdata)
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(target_os = "linux")]
    {
        home_dir().map(|h| {
            h.join(".config")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
}

fn codex_config_path() -> Option<std::path::PathBuf> {
    home_dir().map(|h| h.join(".codex").join("config.toml"))
}

enum McpInstallResult {
    Added,
    AlreadyPresent,
    Updated,
}

/// Install the televisor MCP entry into every detected app config.
/// Purely additive and idempotent; failures are reported, never fatal.
pub fn install_mcp_configs() {
    let televisor_bin = televisor_command();
    let mut installed_any = false;

    for (app_name, config_path) in mcp_config_targets() {
        // Only install into apps the user actually has (config dir exists).
        if let Some(parent) = config_path.parent()
            && !parent.exists()
        {
            continue;
        }
        match add_mcp_entry(&config_path, &televisor_bin) {
            Ok(McpInstallResult::Added) => {
                println!("  ✔ televisor MCP added to {app_name}");
                installed_any = true;
            }
            Ok(McpInstallResult::AlreadyPresent) => {
                println!("  ✔ televisor MCP already configured in {app_name}");
                installed_any = true;
            }
            Ok(McpInstallResult::Updated) => {
                println!("  ✔ televisor MCP updated in {app_name}");
                installed_any = true;
            }
            Err(e) => eprintln!("  ! Failed to configure {app_name}: {e}"),
        }
    }

    if let Some(config_path) = codex_config_path()
        && config_path.parent().is_none_or(|parent| parent.exists())
    {
        match add_codex_mcp_entry(&config_path, &televisor_bin) {
            Ok(McpInstallResult::Added) => {
                println!("  ✔ televisor MCP added to Codex");
                installed_any = true;
            }
            Ok(McpInstallResult::AlreadyPresent) => {
                println!("  ✔ televisor MCP already configured in Codex");
                installed_any = true;
            }
            Ok(McpInstallResult::Updated) => {
                println!("  ✔ televisor MCP updated in Codex");
                installed_any = true;
            }
            Err(e) => eprintln!("  ! Failed to configure Codex: {e}"),
        }
    }

    if !installed_any {
        println!("  No supported apps found. Add the MCP server manually:");
        println!(
            "  {{ \"mcpServers\": {{ \"televisor\": {{ \"command\": \"televisor\", \"args\": [\"mcp\"] }} }} }}"
        );
    }
    println!();
}

/// Add or update the televisor entry in a `mcpServers` JSON config.
fn add_mcp_entry(
    config_path: &std::path::Path,
    televisor_bin: &str,
) -> std::result::Result<McpInstallResult, String> {
    let mut config: serde_json::Value = if config_path.exists() {
        let raw = std::fs::read_to_string(config_path)
            .map_err(|e| format!("read {}: {e}", config_path.display()))?;
        if raw.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&raw)
                .map_err(|e| format!("parse {}: {e}", config_path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let servers = config
        .as_object_mut()
        .ok_or("config is not a JSON object")?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    let new_entry = serde_json::json!({
        "command": televisor_bin,
        "args": ["mcp"]
    });

    let result = if let Some(existing) = servers.get(MCP_KEY) {
        if existing.get("command").and_then(|v| v.as_str()) == Some(televisor_bin)
            && existing.get("args") == Some(&serde_json::json!(["mcp"]))
        {
            McpInstallResult::AlreadyPresent
        } else {
            servers[MCP_KEY] = new_entry;
            McpInstallResult::Updated
        }
    } else {
        servers
            .as_object_mut()
            .ok_or("mcpServers is not an object")?
            .insert(MCP_KEY.to_string(), new_entry);
        McpInstallResult::Added
    };

    if !matches!(result, McpInstallResult::AlreadyPresent) {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(config_path, json + "\n")
            .map_err(|e| format!("write {}: {e}", config_path.display()))?;
    }
    Ok(result)
}

/// Add or update the televisor entry in Codex's TOML config, preserving all
/// existing content and formatting (`toml_edit`).
fn add_codex_mcp_entry(
    config_path: &std::path::Path,
    televisor_bin: &str,
) -> std::result::Result<McpInstallResult, String> {
    let raw = if config_path.exists() {
        std::fs::read_to_string(config_path)
            .map_err(|e| format!("read {}: {e}", config_path.display()))?
    } else {
        String::new()
    };
    let mut config = if raw.trim().is_empty() {
        DocumentMut::new()
    } else {
        raw.parse::<DocumentMut>()
            .map_err(|e| format!("parse {}: {e}", config_path.display()))?
    };

    let existing = config
        .get("mcp_servers")
        .and_then(Item::as_table)
        .and_then(|servers| servers.get(MCP_KEY));
    let existed = existing.is_some();
    let already_present = existing.and_then(Item::as_table).is_some_and(|entry| {
        entry
            .get("command")
            .and_then(Item::as_str)
            .is_some_and(|command| command == televisor_bin)
            && entry
                .get("args")
                .and_then(Item::as_array)
                .is_some_and(|args| {
                    args.iter().map(Value::as_str).collect::<Option<Vec<_>>>() == Some(vec!["mcp"])
                })
    });
    if already_present {
        return Ok(McpInstallResult::AlreadyPresent);
    }

    {
        let servers = ensure_toml_table(&mut config["mcp_servers"]);
        let entry = ensure_toml_table(&mut servers[MCP_KEY]);
        entry["command"] = value(televisor_bin);
        let mut args = Array::new();
        args.push("mcp");
        entry["args"] = Item::Value(Value::Array(args));
        entry["enabled"] = value(true);
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    std::fs::write(config_path, config.to_string())
        .map_err(|e| format!("write {}: {e}", config_path.display()))?;

    Ok(if existed {
        McpInstallResult::Updated
    } else {
        McpInstallResult::Added
    })
}

fn ensure_toml_table(item: &mut Item) -> &mut Table {
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    item.as_table_mut().expect("item was just set to a table")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_entry_creates_new_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claude.json");
        let result = add_mcp_entry(&path, "/usr/local/bin/televisor").unwrap();
        assert!(matches!(result, McpInstallResult::Added));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["televisor"]["command"],
            "/usr/local/bin/televisor"
        );
        assert_eq!(v["mcpServers"]["televisor"]["args"][0], "mcp");
    }

    #[test]
    fn mcp_entry_is_idempotent_and_updates_on_path_change() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claude.json");
        add_mcp_entry(&path, "/old/televisor").unwrap();
        assert!(matches!(
            add_mcp_entry(&path, "/old/televisor").unwrap(),
            McpInstallResult::AlreadyPresent
        ));
        assert!(matches!(
            add_mcp_entry(&path, "/new/televisor").unwrap(),
            McpInstallResult::Updated
        ));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["televisor"]["command"], "/new/televisor");
    }

    #[test]
    fn mcp_entry_preserves_other_servers_and_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claude.json");
        std::fs::write(
            &path,
            r#"{"theme":"dark","mcpServers":{"pay":{"command":"pay","args":["mcp"]}}}"#,
        )
        .unwrap();
        add_mcp_entry(&path, "televisor").unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["pay"]["command"], "pay");
        assert_eq!(v["mcpServers"]["televisor"]["command"], "televisor");
    }

    #[test]
    fn codex_entry_preserves_existing_content_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "model = \"gpt-5\"\n\n[mcp_servers.pay]\ncommand = \"pay\"\nargs = [\"mcp\"]\n",
        )
        .unwrap();

        assert!(matches!(
            add_codex_mcp_entry(&path, "/usr/local/bin/televisor").unwrap(),
            McpInstallResult::Added
        ));
        assert!(matches!(
            add_codex_mcp_entry(&path, "/usr/local/bin/televisor").unwrap(),
            McpInstallResult::AlreadyPresent
        ));
        assert!(matches!(
            add_codex_mcp_entry(&path, "/new/televisor").unwrap(),
            McpInstallResult::Updated
        ));

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("model = \"gpt-5\""));
        assert!(raw.contains("[mcp_servers.pay]"));
        assert!(raw.contains("[mcp_servers.televisor]"));
        assert!(raw.contains("command = \"/new/televisor\""));
        assert!(raw.contains("enabled = true"));
    }
}
